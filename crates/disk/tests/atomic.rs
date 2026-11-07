//! Specification of atomic file replacement (see `disk::atomic`).

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use common::TempDir;
use disk::atomic::atomic_write;

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

// ── Content ──────────────────────────────────────────────────────────────────

#[test]
fn creates_a_missing_file_with_the_exact_bytes() {
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    atomic_write(&target, b"a=1\n").unwrap();
    assert_eq!(read(&target), b"a=1\n");
}

#[test]
fn replaces_an_existing_file_completely() {
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    std::fs::write(
        &target,
        b"a much longer old content that must not leave a tail",
    )
    .unwrap();
    atomic_write(&target, b"short").unwrap();
    assert_eq!(read(&target), b"short");

    atomic_write(&target, b"now it is longer than before, again").unwrap();
    assert_eq!(read(&target), b"now it is longer than before, again");
}

#[test]
fn writes_arbitrary_bytes_unchanged() {
    let dir = TempDir::new();
    let target = dir.path().join("bytes.bin");
    let mut bytes: Vec<u8> = (0..=255u8).collect();
    bytes.extend_from_slice(b"\r\n\r\n\r\n\n\r");
    atomic_write(&target, &bytes).unwrap();
    assert_eq!(read(&target), bytes);
}

#[test]
fn writes_empty_content_over_existing_content() {
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    std::fs::write(&target, b"old").unwrap();
    atomic_write(&target, b"").unwrap();
    assert_eq!(read(&target), b"");
}

#[test]
fn writes_large_content() {
    let dir = TempDir::new();
    let target = dir.path().join("big.properties");
    let bytes: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    atomic_write(&target, &bytes).unwrap();
    assert_eq!(read(&target), bytes);
}

// ── No traces ────────────────────────────────────────────────────────────────

#[test]
fn leaves_no_other_file_behind_after_success() {
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    atomic_write(&target, b"first").unwrap();
    assert_eq!(dir.entries(), ["a.properties"]);
    atomic_write(&target, b"second").unwrap();
    assert_eq!(dir.entries(), ["a.properties"]);
}

#[test]
fn does_not_touch_other_files_in_the_directory() {
    let dir = TempDir::new();
    dir.write("other.properties", b"keep me");
    atomic_write(&dir.path().join("a.properties"), b"x").unwrap();
    assert_eq!(read(&dir.path().join("other.properties")), b"keep me");
    assert_eq!(dir.entries(), ["a.properties", "other.properties"]);
}

// ── Failures leave the target alone ──────────────────────────────────────────

#[test]
fn a_missing_parent_directory_is_an_error_and_nothing_is_created() {
    let dir = TempDir::new();
    let target = dir.path().join("missing").join("a.properties");
    assert!(atomic_write(&target, b"x").is_err());
    assert!(dir.entries().is_empty());
}

#[test]
fn a_directory_as_target_is_an_error_and_stays_untouched() {
    let dir = TempDir::new();
    dir.write("target/inside.txt", b"inside");
    let target = dir.path().join("target");
    assert!(atomic_write(&target, b"x").is_err());
    assert!(target.is_dir());
    assert_eq!(read(&target.join("inside.txt")), b"inside");
    assert_eq!(dir.entries(), ["target"]);
}

#[test]
fn a_read_only_target_is_an_error_and_stays_unchanged() {
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    std::fs::write(&target, b"original").unwrap();
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&target, permissions).unwrap();

    let result = atomic_write(&target, b"changed");

    // restore first, so that the temp dir can be removed even if an assertion fails
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&target, permissions).unwrap();

    assert!(result.is_err());
    assert_eq!(read(&target), b"original");
    assert_eq!(dir.entries(), ["a.properties"]);
}

#[cfg(unix)]
#[test]
fn keeps_the_permissions_of_the_replaced_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let target = dir.path().join("a.properties");
    std::fs::write(&target, b"old").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    atomic_write(&target, b"new").unwrap();
    let mode = std::fs::metadata(&target).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

// ── The actual guarantee ─────────────────────────────────────────────────────

/// Content `i` is `SIZE + i` bytes, all with the value `i`: any mix of two contents or any
/// truncation is recognisable.
fn content(i: u8) -> Vec<u8> {
    vec![i; 128 * 1024 + usize::from(i)]
}

fn is_a_complete_content(bytes: &[u8], writers: u8) -> bool {
    (0..writers).any(|i| bytes == content(i).as_slice())
}

#[test]
fn concurrent_writers_never_expose_partial_content() {
    const WRITERS: u8 = 4;
    const ROUNDS: usize = 15;
    let dir = TempDir::new();
    let target = dir.path().join("shared.properties");
    std::fs::write(&target, content(0)).unwrap();
    let done = AtomicBool::new(false);

    let (successes, partial) = std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut partial = 0usize;
                    while !done.load(Ordering::SeqCst) {
                        // Opening can fail briefly on some platforms while the file is replaced;
                        // that is not a violation. What can be read must be complete.
                        if let Ok(bytes) = std::fs::read(&target)
                            && !is_a_complete_content(&bytes, WRITERS)
                        {
                            partial += 1;
                        }
                    }
                    partial
                })
            })
            .collect();
        let writers: Vec<_> = (0..WRITERS)
            .map(|i| {
                let target = &target;
                scope.spawn(move || {
                    (0..ROUNDS)
                        .filter(|_| atomic_write(target, &content(i)).is_ok())
                        .count()
                })
            })
            .collect();
        // Stop the readers before looking at any writer result: a panicking writer must not
        // leave the readers spinning forever.
        let writer_results: Vec<_> = writers.into_iter().map(|w| w.join()).collect();
        done.store(true, Ordering::SeqCst);
        let partial: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
        let successes: usize = writer_results.into_iter().map(|r| r.unwrap()).sum();
        (successes, partial)
    });

    assert_eq!(partial, 0, "a reader saw partial or mixed content");
    assert!(successes > 0, "no write succeeded at all");
    assert!(is_a_complete_content(&read(&target), WRITERS));
    assert_eq!(
        dir.entries(),
        ["shared.properties"],
        "temporary files left behind"
    );
}
