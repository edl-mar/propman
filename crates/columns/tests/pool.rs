//! Specification of the pool (see `columns::Pool`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use columns::{Generation, HistoryColumn, Idx, Pool};

// ── Ids owned by the code that uses the pools, as the store will ─────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextId(Generation, usize);

impl Idx for TextId {
    fn idx(self) -> usize {
        self.1
    }

    fn generation(self) -> Generation {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BytesId(Generation, usize);

impl Idx for BytesId {
    fn idx(self) -> usize {
        self.1
    }

    fn generation(self) -> Generation {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathId(Generation, usize);

impl Idx for PathId {
    fn idx(self) -> usize {
        self.1
    }

    fn generation(self) -> Generation {
        self.0
    }
}

type Text = Pool<TextId, str>;

fn new_text() -> Text {
    Pool::new(Generation::next())
}

/// The owner of the pool: only it turns an index into a `TextId`.
fn add(pool: &mut Text, value: &str) -> TextId {
    let generation = pool.generation();
    TextId(generation, pool.add(value))
}

// ── Adding and deduplicating ─────────────────────────────────────────────────

#[test]
fn a_new_pool_is_empty() {
    let pool = new_text();
    assert_eq!(pool.len(), 0);
    assert!(pool.is_empty());
}

#[test]
fn new_content_gets_ascending_indices() {
    let mut pool = new_text();
    assert_eq!(pool.add("a"), 0);
    assert_eq!(pool.add("b"), 1);
    assert_eq!(pool.add("c"), 2);
    assert_eq!(pool.len(), 3);
    assert!(!pool.is_empty());
}

#[test]
fn equal_content_gets_the_same_index_and_is_stored_once() {
    let mut pool = new_text();
    assert_eq!(pool.add("a"), 0);
    assert_eq!(pool.add("a"), 0);
    assert_eq!(pool.add("b"), 1);
    assert_eq!(pool.add("a"), 0);
    assert_eq!(pool.len(), 2);
}

#[test]
fn different_content_gets_a_different_index() {
    let mut pool = new_text();
    let values = [
        "a", "A", "ab", "abc", " a", "a ", "a\n", "a\r\n", "\u{e9}", "e\u{301}",
    ];
    let indices: Vec<usize> = values.iter().map(|v| pool.add(v)).collect();
    let mut distinct = indices.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), values.len(), "{indices:?}");
}

#[test]
fn the_empty_value_is_a_normal_value() {
    let mut pool = new_text();
    let empty = add(&mut pool, "");
    assert_eq!(empty.idx(), 0);
    assert_eq!(add(&mut pool, ""), empty);
    assert_eq!(add(&mut pool, "x").idx(), 1);
    assert_eq!(pool.get(empty), Some(""));
    assert_eq!(pool.len(), 2);
}

#[test]
fn ids_of_equal_content_are_equal_and_ids_of_different_content_are_not() {
    let mut pool = new_text();
    let first = add(&mut pool, "app.title");
    let again = add(&mut pool, "app.title");
    let other = add(&mut pool, "app.version");
    assert_eq!(first, again);
    assert_ne!(first, other);
}

// ── Finding and reading ──────────────────────────────────────────────────────

#[test]
fn find_looks_up_without_adding() {
    let mut pool = new_text();
    assert_eq!(pool.find("a"), None);
    assert_eq!(pool.len(), 0);

    pool.add("a");
    assert_eq!(pool.find("a"), Some(0));
    assert_eq!(pool.find("zzz"), None);
    assert_eq!(pool.len(), 1);
}

#[test]
fn get_returns_the_content_of_an_id() {
    let mut pool = new_text();
    let hello = add(&mut pool, "hello");
    let world = add(&mut pool, "w\u{f6}rld");
    assert_eq!(pool.get(hello), Some("hello"));
    assert_eq!(pool.get(world), Some("w\u{f6}rld"));
}

#[test]
fn an_unknown_index_gives_none() {
    let mut pool = new_text();
    add(&mut pool, "a");
    assert_eq!(pool.get(TextId(pool.generation(), 99)), None);
}

// ── Stable ids, nothing removed ──────────────────────────────────────────────

#[test]
fn ids_stay_valid_and_unchanged_after_many_more_additions() {
    let mut pool = new_text();
    let first = add(&mut pool, "first");
    let second = add(&mut pool, "second");
    for i in 0..2000 {
        add(&mut pool, &format!("filler {i}"));
    }
    assert_eq!(first.idx(), 0);
    assert_eq!(second.idx(), 1);
    assert_eq!(pool.get(first), Some("first"));
    assert_eq!(pool.get(second), Some("second"));
    assert_eq!(add(&mut pool, "first"), first);
}

#[test]
fn many_different_values_all_keep_their_own_index_and_content() {
    let mut pool = new_text();
    let values: Vec<String> = (0..10_000).map(|i| format!("k{i}")).collect();
    let ids: Vec<TextId> = values.iter().map(|v| add(&mut pool, v)).collect();

    assert_eq!(pool.len(), values.len());
    for (id, value) in ids.iter().zip(&values) {
        assert_eq!(pool.get(*id), Some(value.as_str()));
    }
    // adding all of them again changes nothing
    let again: Vec<TextId> = values.iter().map(|v| add(&mut pool, v)).collect();
    assert_eq!(again, ids);
    assert_eq!(pool.len(), values.len());
}

// ── Generations ──────────────────────────────────────────────────────────────

#[test]
fn a_pool_has_the_generation_it_was_made_with() {
    let generation = Generation::next();
    let pool: Text = Pool::new(generation);
    assert_eq!(pool.generation(), generation);
}

#[test]
fn an_id_of_another_generation_gives_none_even_if_the_index_exists() {
    let mut mine = new_text();
    let mut other = new_text();
    let own = add(&mut mine, "mine");
    let foreign = add(&mut other, "theirs");
    assert_eq!(own.idx(), foreign.idx());

    assert_eq!(mine.get(foreign), None);
    assert_eq!(mine.get(own), Some("mine"));
    assert_eq!(other.get(own), None);
}

#[test]
fn after_a_rebuild_old_ids_do_not_find_the_new_values() {
    let mut before = new_text();
    let old = add(&mut before, "old");

    let mut after = new_text();
    let new = add(&mut after, "new");
    assert_eq!(old.idx(), new.idx());

    assert_eq!(after.get(old), None);
    assert_eq!(after.get(new), Some("new"));
}

// ── Other kinds of values ────────────────────────────────────────────────────

#[test]
fn bytes_are_values_whatever_they_contain() {
    let mut pool: Pool<BytesId, [u8]> = Pool::new(Generation::next());
    let generation = pool.generation();
    let nulls = BytesId(generation, pool.add(&[0u8, 1, 0][..]));
    let invalid = BytesId(generation, pool.add(&[0xff, 0xfe][..]));
    let zero = BytesId(generation, pool.add(&[0u8][..]));
    let two_zeros = BytesId(generation, pool.add(&[0u8, 0][..]));
    let empty = BytesId(generation, pool.add(&[][..]));

    assert_eq!(pool.len(), 5);
    assert_eq!(pool.add(&[0u8, 1, 0][..]), nulls.idx());
    assert_eq!(pool.get(nulls), Some(&[0u8, 1, 0][..]));
    assert_eq!(pool.get(invalid), Some(&[0xff, 0xfe][..]));
    assert_eq!(pool.get(zero), Some(&[0u8][..]));
    assert_eq!(pool.get(two_zeros), Some(&[0u8, 0][..]));
    assert_eq!(pool.get(empty), Some(&[][..]));
}

#[test]
fn large_values_are_stored_once() {
    let mut pool: Pool<BytesId, [u8]> = Pool::new(Generation::next());
    let big = vec![7u8; 5 * 1024 * 1024];
    let first = pool.add(&big);
    let second = pool.add(&big);
    assert_eq!(first, second);
    assert_eq!(pool.len(), 1);

    let mut other = big.clone();
    *other.last_mut().unwrap() = 8;
    assert_ne!(pool.add(&other), first);
    assert_eq!(pool.len(), 2);
}

#[test]
fn paths_are_values_and_follow_the_equality_of_path() {
    let mut pool: Pool<PathId, Path> = Pool::new(Generation::next());
    let generation = pool.generation();
    let file = PathId(generation, pool.add(Path::new("a/b.properties")));
    let other = PathId(generation, pool.add(Path::new("a/c.properties")));

    assert_ne!(file, other);
    assert_eq!(pool.add(Path::new("a/b.properties")), file.idx());
    assert_eq!(pool.get(file), Some(Path::new("a/b.properties")));
    // `Path` ignores a trailing separator, so these are the same value
    assert_eq!(pool.add(Path::new("a/")), pool.add(Path::new("a")));
}

// ── Pooled values in a column ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FileId(Generation, usize);

impl Idx for FileId {
    fn idx(self) -> usize {
        self.1
    }

    fn generation(self) -> Generation {
        self.0
    }
}

#[test]
fn a_column_can_hold_pooled_values_and_compares_them_by_id() {
    let generation = Generation::next();
    let mut pool: Text = Pool::new(generation);
    let mut names: HistoryColumn<FileId, TextId, ()> = HistoryColumn::new(generation);

    let first_name = TextId(generation, pool.add("messages_de"));
    let file = FileId(generation, names.push_row(first_name, ()));

    // typing the same name again gives the same id, so the column sees "no change"
    let again = TextId(generation, pool.add("messages_de"));
    assert_eq!(names.current(file), Some(again));

    let renamed = TextId(generation, pool.add("messages_fr"));
    names.append(file, renamed, ());
    assert_ne!(names.current(file), Some(first_name));
    assert_eq!(pool.get(names.current(file).unwrap()), Some("messages_fr"));
}
