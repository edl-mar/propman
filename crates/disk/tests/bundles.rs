//! Specification of bundle and locale detection (see `disk::bundles`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use Confidence::{Certain, Uncertain};
use disk::bundles::{Classified, Confidence, classify_files};

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

fn classify(names: &[&str]) -> Vec<Classified> {
    classify_files(&paths(names))
}

/// Classifies all `names` together and checks the result for `file`.
#[track_caller]
fn expect(names: &[&str], file: &str, bundle: &str, locale: Option<&str>, confidence: Confidence) {
    let all = classify(names);
    let got = all
        .iter()
        .find(|c| c.path.as_path() == std::path::Path::new(file))
        .unwrap_or_else(|| panic!("{file} missing from result"));
    assert_eq!(
        (got.bundle.as_str(), got.locale.as_deref(), got.confidence),
        (bundle, locale, confidence),
        "{file}"
    );
}

// ── A file without a locale suffix ───────────────────────────────────────────

#[test]
fn a_name_without_a_well_formed_suffix_is_a_default_file() {
    expect(
        &["some_translations.properties"],
        "some_translations.properties",
        "some_translations",
        None,
        Certain,
    );
    expect(
        &["another_single_file.properties"],
        "another_single_file.properties",
        "another_single_file",
        None,
        Certain,
    );
    expect(
        &["messages.properties"],
        "messages.properties",
        "messages",
        None,
        Certain,
    );
}

#[test]
fn a_dot_in_the_name_is_not_a_java_locale_separator() {
    expect(
        &["messages.de.properties"],
        "messages.de.properties",
        "messages.de",
        None,
        Certain,
    );
}

#[test]
fn default_is_not_special_just_a_base_name() {
    let names = ["testbundle.properties", "testbundle_default.properties"];
    expect(
        &names,
        "testbundle_default.properties",
        "testbundle_default",
        None,
        Certain,
    );
    expect(&names, "testbundle.properties", "testbundle", None, Certain);
}

// ── Locale suffix with evidence ──────────────────────────────────────────────

#[test]
fn a_known_locale_next_to_the_default_file_is_certain() {
    let names = [
        "some_translations.properties",
        "some_translations_de.properties",
    ];
    expect(
        &names,
        "some_translations_de.properties",
        "some_translations",
        Some("de"),
        Certain,
    );
    expect(
        &names,
        "some_translations.properties",
        "some_translations",
        None,
        Certain,
    );
}

#[test]
fn several_locales_of_the_same_base_are_evidence_for_each_other() {
    let names = ["app_de.properties", "app_fr.properties"];
    expect(&names, "app_de.properties", "app", Some("de"), Certain);
    expect(&names, "app_fr.properties", "app", Some("fr"), Certain);
}

#[test]
fn language_with_country_script_and_variant() {
    let base = "messages.properties";
    expect(
        &[base, "messages_de_DE.properties"],
        "messages_de_DE.properties",
        "messages",
        Some("de_DE"),
        Certain,
    );
    expect(
        &[base, "messages_zh_Hans_CN.properties"],
        "messages_zh_Hans_CN.properties",
        "messages",
        Some("zh_Hans_CN"),
        Certain,
    );
    expect(
        &[base, "messages_de_DE_POSIX.properties"],
        "messages_de_DE_POSIX.properties",
        "messages",
        Some("de_DE_POSIX"),
        Certain,
    );
    expect(
        &[base, "messages_sr_XK.properties"],
        "messages_sr_XK.properties",
        "messages",
        Some("sr_XK"),
        Certain,
    );
}

#[test]
fn three_letter_language_codes_are_accepted() {
    expect(
        &["messages.properties", "messages_fil.properties"],
        "messages_fil.properties",
        "messages",
        Some("fil"),
        Certain,
    );
}

#[test]
fn the_longest_canonical_suffix_wins_and_the_rest_is_the_base_name() {
    // `app` is not read as a language code with country `en` and variant `US`.
    let names = ["my_app.properties", "my_app_en_US.properties"];
    expect(
        &names,
        "my_app_en_US.properties",
        "my_app",
        Some("en_US"),
        Certain,
    );
}

#[test]
fn underscores_in_the_base_name_are_fine() {
    let names = ["error_messages.properties", "error_messages_de.properties"];
    expect(
        &names,
        "error_messages_de.properties",
        "error_messages",
        Some("de"),
        Certain,
    );
}

// ── Locale suffix without enough evidence ────────────────────────────────────

#[test]
fn a_known_locale_without_evidence_is_the_likely_reading_but_uncertain() {
    expect(
        &["single_file_de.properties"],
        "single_file_de.properties",
        "single_file",
        Some("de"),
        Uncertain,
    );
    expect(
        &["a_b_de.properties"],
        "a_b_de.properties",
        "a_b",
        Some("de"),
        Uncertain,
    );
    // `in` is an old Java language code (Indonesian)
    expect(
        &["log_in.properties"],
        "log_in.properties",
        "log",
        Some("in"),
        Uncertain,
    );
}

#[test]
fn an_unknown_code_with_evidence_is_a_locale_but_uncertain() {
    let names = ["messages.properties", "messages_xx.properties"];
    expect(
        &names,
        "messages_xx.properties",
        "messages",
        Some("xx"),
        Uncertain,
    );
}

#[test]
fn an_unknown_code_without_evidence_is_part_of_the_base_name() {
    expect(
        &["messages_xx.properties"],
        "messages_xx.properties",
        "messages_xx",
        None,
        Certain,
    );
}

#[test]
fn non_canonical_case_is_never_certain() {
    expect(
        &["messages_DE.properties"],
        "messages_DE.properties",
        "messages",
        Some("DE"),
        Uncertain,
    );
    let names = ["messages.properties", "messages_DE.properties"];
    expect(
        &names,
        "messages_DE.properties",
        "messages",
        Some("DE"),
        Uncertain,
    );
}

// ── A suffix-less file that is the base of another file ──────────────────────

#[test]
fn a_file_that_is_the_base_of_another_is_a_default_file_even_if_it_looks_like_a_locale() {
    // `a_id` could be read as base `a` with locale `id`, but `a_id_de` uses `a_id` as base.
    let names = ["a_id.properties", "a_id_de.properties"];
    expect(&names, "a_id.properties", "a_id", None, Certain);
    expect(&names, "a_id_de.properties", "a_id", Some("de"), Certain);
}

// ── Folders ──────────────────────────────────────────────────────────────────

#[test]
fn the_same_base_name_in_two_folders_stays_separate() {
    let names = [
        "a/messages.properties",
        "a/messages_de.properties",
        "b/messages.properties",
        "b/messages_fr.properties",
    ];
    expect(
        &names,
        "a/messages_de.properties",
        "messages",
        Some("de"),
        Certain,
    );
    expect(
        &names,
        "b/messages_fr.properties",
        "messages",
        Some("fr"),
        Certain,
    );
    expect(&names, "a/messages.properties", "messages", None, Certain);
}

#[test]
fn evidence_from_another_folder_does_not_count() {
    let names = ["a/messages_de.properties", "b/messages.properties"];
    expect(
        &names,
        "a/messages_de.properties",
        "messages",
        Some("de"),
        Uncertain,
    );
}

// ── Result shape and determinism ─────────────────────────────────────────────

#[test]
fn the_result_has_one_entry_per_path_sorted_by_path() {
    let result = classify(&["b/x.properties", "a/y_de.properties", "a/y.properties"]);
    let got: Vec<&std::path::Path> = result.iter().map(|c| c.path.as_path()).collect();
    assert_eq!(
        got,
        vec![
            std::path::Path::new("a/y.properties"),
            std::path::Path::new("a/y_de.properties"),
            std::path::Path::new("b/x.properties"),
        ]
    );
}

#[test]
fn the_input_order_does_not_change_the_result() {
    let names = [
        "messages.properties",
        "messages_de.properties",
        "a_id.properties",
        "a_id_de.properties",
        "single_de.properties",
    ];
    let expected = classify(&names);
    // all 120 permutations (Heap's algorithm)
    let mut items: Vec<&str> = names.to_vec();
    let n = items.len();
    let mut counters = vec![0usize; n];
    let mut i = 0;
    while i < n {
        if counters[i] < i {
            let j = if i % 2 == 0 { 0 } else { counters[i] };
            items.swap(j, i);
            assert_eq!(classify(&items), expected, "order = {items:?}");
            counters[i] += 1;
            i = 0;
        } else {
            counters[i] = 0;
            i += 1;
        }
    }
}

// ── Real fixtures ────────────────────────────────────────────────────────────

fn files_under(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn classified_fixtures(sub: &str) -> Vec<(String, String, Option<String>, Confidence)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(sub);
    let mut files = Vec::new();
    files_under(&root, &mut files);
    let mut got: Vec<_> = classify_files(&files)
        .into_iter()
        .map(|c| {
            let dir = c.path.parent().unwrap().strip_prefix(&root).unwrap();
            let dir = dir.display().to_string().replace('\\', "/");
            (dir, c.bundle, c.locale, c.confidence)
        })
        .collect();
    sort_rows(&mut got);
    got
}

type Row = (String, String, Option<String>, Confidence);

/// `Confidence` has no ordering; rows are sorted by (folder, bundle, locale).
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
}

fn row(
    dir: &str,
    bundle: &str,
    locale: Option<&str>,
) -> (String, String, Option<String>, Confidence) {
    (
        dir.to_owned(),
        bundle.to_owned(),
        locale.map(str::to_owned),
        Certain,
    )
}

#[test]
fn the_project_fixtures_are_classified_as_intended() {
    let mut expected = vec![
        row("", "messages", None),
        row("", "messages", Some("de")),
        row("", "messages", Some("fr")),
        row("", "messages", Some("it")),
        row("", "stripping", None),
        row("", "testbundle", None),
        row("", "testbundle", Some("de")),
        row("", "testbundle", Some("fr")),
        row("", "testbundle", Some("it")),
        row("sub_folder", "translations", None),
        row("sub_folder", "translations", Some("de")),
        row("sub_folder", "translations", Some("si")),
    ];
    sort_rows(&mut expected);
    assert_eq!(classified_fixtures("project"), expected);
}

#[test]
fn the_encoding_fixtures_are_classified_as_intended() {
    let mut expected = vec![
        row("encodings", "messages", None),
        row("encodings", "messages", Some("de")),
        row("encodings", "messages", Some("fr")),
        row("encodings", "messages", Some("it")),
        row("encodings", "messages", Some("pl")),
    ];
    sort_rows(&mut expected);
    assert_eq!(classified_fixtures("edge"), expected);
}
