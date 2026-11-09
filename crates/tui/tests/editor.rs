//! Specification of the command that runs the editor of the user: which variable counts, how the
//! value is split and where the path goes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsString;
use std::path::Path;

use tui::{EditorCommand, editor_command};

fn command(visual: Option<&str>, editor: Option<&str>) -> Option<EditorCommand> {
    editor_command(visual, editor, Path::new("some dir/a.properties"))
}

fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn without_a_variable_there_is_no_editor() {
    assert_eq!(command(None, None), None);
}

#[test]
fn an_empty_variable_counts_as_not_set() {
    assert_eq!(command(Some(""), None), None);
    assert_eq!(command(Some("  \t"), Some("")), None);
    assert_eq!(command(None, Some("   ")), None);
}

#[test]
fn a_program_gets_the_path_as_its_only_argument() {
    let command = command(None, Some("vim")).unwrap();
    assert_eq!(command.program, "vim");
    assert_eq!(command.args, args(&["some dir/a.properties"]));
}

#[test]
fn visual_wins_over_editor() {
    assert_eq!(command(Some("hx"), Some("vim")).unwrap().program, "hx");
}

#[test]
fn an_empty_visual_does_not_hide_editor() {
    assert_eq!(command(Some(" "), Some("vim")).unwrap().program, "vim");
}

#[test]
fn arguments_in_the_variable_come_before_the_path() {
    let command = command(None, Some("code -w --new-window")).unwrap();
    assert_eq!(command.program, "code");
    assert_eq!(
        command.args,
        args(&["-w", "--new-window", "some dir/a.properties"])
    );
}

#[test]
fn any_white_space_separates_and_does_not_make_empty_arguments() {
    let command = command(Some("  ed \t -v   -s "), None).unwrap();
    assert_eq!(command.program, "ed");
    assert_eq!(command.args, args(&["-v", "-s", "some dir/a.properties"]));
}

#[test]
fn the_path_stays_one_argument_even_with_spaces() {
    let command =
        editor_command(None, Some("vim"), Path::new("C:/my files/a b.properties")).unwrap();
    assert_eq!(command.args, args(&["C:/my files/a b.properties"]));
}
