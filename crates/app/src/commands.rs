//! The commands: actions with a name, which are typed in the command line and bound to keys.
//!
//! A command is a name, a label and the message it stands for. The names are what the user types
//! and what a settings file will use. The candidates for what is typed are the commands whose
//! name contains the input as a subsequence, without regard to case, and that can be done now.
//! They are ordered the same way every time, so that the fingers can learn it: first the names
//! that begin with the input, then those whose words begin with the letters of the input (`pd`
//! for `preview-down`, the words may be skipped), then any other subsequence; within one group
//! in the order of the table.

use crate::selection::applicable;
use crate::state::{AppState, Message};

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub name: &'static str,
    pub label: &'static str,
    pub message: Message,
}

/// The commands, in the order in which they are listed.
pub fn commands() -> &'static [Command] {
    &[
        Command {
            name: "quit",
            label: "quit propman",
            message: Message::Quit,
        },
        Command {
            name: "edit",
            label: "edit the file under the cursor",
            message: Message::Edit,
        },
        Command {
            name: "mark",
            label: "mark or unmark the item under the cursor",
            message: Message::ToggleMark,
        },
        Command {
            name: "clear-marks",
            label: "clear all marks",
            message: Message::ClearMarks,
        },
        Command {
            name: "preview",
            label: "show or hide the preview",
            message: Message::TogglePreview,
        },
        Command {
            name: "preview-up",
            label: "scroll the preview up by one line",
            message: Message::PreviewUp,
        },
        Command {
            name: "preview-down",
            label: "scroll the preview down by one line",
            message: Message::PreviewDown,
        },
        Command {
            name: "preview-half-up",
            label: "scroll the preview up by half a page",
            message: Message::PreviewHalfPageUp,
        },
        Command {
            name: "preview-half-down",
            label: "scroll the preview down by half a page",
            message: Message::PreviewHalfPageDown,
        },
        Command {
            name: "up",
            label: "move the cursor up",
            message: Message::Up,
        },
        Command {
            name: "down",
            label: "move the cursor down",
            message: Message::Down,
        },
        Command {
            name: "page-up",
            label: "move the cursor up by a page",
            message: Message::PageUp,
        },
        Command {
            name: "page-down",
            label: "move the cursor down by a page",
            message: Message::PageDown,
        },
        Command {
            name: "first",
            label: "move the cursor to the first item",
            message: Message::First,
        },
        Command {
            name: "last",
            label: "move the cursor to the last item",
            message: Message::Last,
        },
    ]
}

/// A command that fits what is typed, with the places (in characters of its name) of the letters
/// that were hit.
#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    pub command: &'static Command,
    pub matched: Vec<usize>,
}

/// How a name fits the input: the group (0, 1 or 2, see above) and the places of the letters.
fn fit(name: &str, input: &[char]) -> Option<(u8, Vec<usize>)> {
    let name: Vec<char> = name.chars().collect();
    if input.is_empty() {
        return Some((0, Vec::new()));
    }
    if name.starts_with(input) {
        return Some((0, (0..input.len()).collect()));
    }
    // the beginnings of the words, which may be skipped
    let starts: Vec<usize> = (0..name.len())
        .filter(|&index| index == 0 || name[index - 1] == '-')
        .collect();
    if let Some(places) = subsequence(&name, input, Some(&starts)) {
        return Some((1, places));
    }
    subsequence(&name, input, None).map(|places| (2, places))
}

/// The places of the leftmost hits of the input in order, among the allowed places if there are
/// any.
fn subsequence(name: &[char], input: &[char], allowed: Option<&[usize]>) -> Option<Vec<usize>> {
    let mut places = Vec::new();
    let mut from = 0;
    for wanted in input {
        let place = (from..name.len())
            .filter(|index| allowed.is_none_or(|allowed| allowed.contains(index)))
            .find(|&index| name[index] == *wanted)?;
        places.push(place);
        from = place + 1;
    }
    Some(places)
}

/// The candidates for the input of the command line, in order; none if it is closed.
pub fn candidates(state: &AppState) -> Vec<Candidate> {
    let Some(line) = state.command_line() else {
        return Vec::new();
    };
    let input: Vec<char> = line
        .field()
        .text()
        .chars()
        .flat_map(char::to_lowercase)
        .collect();
    let mut fitting: Vec<(u8, Candidate)> = commands()
        .iter()
        .filter(|command| applicable(state, command.message))
        .filter_map(|command| {
            let (group, matched) = fit(command.name, &input)?;
            Some((group, Candidate { command, matched }))
        })
        .collect();
    // the sort is stable: within a group the order of the table stays
    fitting.sort_by_key(|(group, _)| *group);
    fitting
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}
