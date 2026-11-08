//! The views get what they show from `app` (`overview`, the counts of the session) and never
//! read the store themselves, so that ids and the internals of `store` stay out of the drawing
//! code. Cargo cannot enforce this because `Session::store` is public for the tests of `app`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn the_views_do_not_read_the_store() {
    let mut found = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(!found.is_empty(), "no sources found");
    for (path, text) in found {
        assert!(!text.contains(".store()"), "{path} reads the store");
        assert!(!text.contains("Store"), "{path} names the store type");
    }
}
