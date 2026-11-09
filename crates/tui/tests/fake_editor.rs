//! The keys of the fake editor that is used to try the failure cases by hand.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::EditorOutcome;
use ratatui::crossterm::event::KeyCode;
use tui::outcome_for_key;

#[test]
fn every_key_gives_one_way_for_the_editor_to_end() {
    let cases = [
        (KeyCode::Char('1'), EditorOutcome::NoEditor),
        (KeyCode::Char('2'), EditorOutcome::CouldNotStart),
        (KeyCode::Char('3'), EditorOutcome::Failed(Some(1))),
        (KeyCode::Char('4'), EditorOutcome::Failed(None)),
        (KeyCode::Char('5'), EditorOutcome::Done),
        (KeyCode::Esc, EditorOutcome::Done),
    ];
    for (code, outcome) in cases {
        assert_eq!(outcome_for_key(code), Some(outcome), "{code:?}");
    }
}

#[test]
fn other_keys_do_nothing() {
    for code in [
        KeyCode::Char('0'),
        KeyCode::Char('6'),
        KeyCode::Char('q'),
        KeyCode::Enter,
    ] {
        assert_eq!(outcome_for_key(code), None, "{code:?}");
    }
}
