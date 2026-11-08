//! Specification of the command line (see `propman::cli`).

use std::ffi::OsString;
use std::path::PathBuf;

use propman::cli::{UsageError, parse, usage};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn without_an_argument_the_current_folder_is_opened() {
    assert_eq!(parse(args(&[])), Ok(PathBuf::from(".")));
}

#[test]
fn one_argument_is_the_folder() {
    assert_eq!(
        parse(args(&["some/folder"])),
        Ok(PathBuf::from("some/folder"))
    );
}

#[test]
fn a_folder_with_spaces_stays_one_argument() {
    assert_eq!(parse(args(&["my folder"])), Ok(PathBuf::from("my folder")));
}

#[test]
fn more_than_one_argument_is_too_many() {
    assert_eq!(parse(args(&["a", "b"])), Err(UsageError::TooManyArguments));
}

#[test]
fn an_argument_starting_with_a_dash_is_an_unknown_option() {
    assert_eq!(
        parse(args(&["--help"])),
        Err(UsageError::UnknownOption(OsString::from("--help")))
    );
    assert_eq!(
        parse(args(&["-x"])),
        Err(UsageError::UnknownOption(OsString::from("-x")))
    );
}

#[test]
fn the_usage_names_the_program_and_the_folder() {
    let text = usage();
    assert!(text.contains("propman"), "{text}");
    assert!(text.contains("FOLDER"), "{text}");
}
