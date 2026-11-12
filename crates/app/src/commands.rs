//! The commands: actions with a name, which are typed in the command line and bound to keys.
//!
//! A command is a name, a label and what it does: a message at once, or a message that needs one
//! value chosen from a list first (`encoding Windows-1252`). The names are what the user types and
//! what a settings file will use. The candidates for what is typed are the entries (commands, and
//! after a command with a list its values) whose name contains the input as a subsequence,
//! without regard to case, and that can be done now. They are ordered the same way every time, so
//! that the fingers can learn it: first the names that begin with the input, then those whose
//! words begin with the letters of the input (`pd` for `preview-down`, the words may be skipped),
//! then any other subsequence; within one group in the order of the table.

use types::Encoding;

use crate::selection::applicable;
use crate::state::{AppState, Message};

/// The name under which an encoding is shown and typed.
pub fn encoding_name(encoding: Encoding) -> &'static str {
    match encoding {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf8Bom => "UTF-8+BOM",
        Encoding::Windows1252 => "Windows-1252",
        Encoding::Latin1 => "Latin-1",
    }
}

/// One of the values that a command lets the user choose from, and the message it stands for.
#[derive(Debug, PartialEq, Eq)]
pub struct Value {
    pub name: &'static str,
    pub label: &'static str,
    pub message: Message,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// The command is done at once.
    Message(Message),
    /// The command needs one of these values.
    Choose(&'static [Value]),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub name: &'static str,
    pub label: &'static str,
    pub action: Action,
}

const ENCODINGS: [Value; 4] = [
    Value {
        name: "UTF-8",
        label: "UTF-8, without a byte order mark",
        message: Message::SetEncoding(Encoding::Utf8),
    },
    Value {
        name: "UTF-8+BOM",
        label: "UTF-8 with a byte order mark",
        message: Message::SetEncoding(Encoding::Utf8Bom),
    },
    Value {
        name: "Windows-1252",
        label: "Windows-1252 (cp1252)",
        message: Message::SetEncoding(Encoding::Windows1252),
    },
    Value {
        name: "Latin-1",
        label: "ISO-8859-1 (latin1)",
        message: Message::SetEncoding(Encoding::Latin1),
    },
];

const YES_NO: [Value; 2] = [
    Value {
        name: "yes",
        label: "leave it out of the session",
        message: Message::SetExcluded(true),
    },
    Value {
        name: "no",
        label: "take it into the session",
        message: Message::SetExcluded(false),
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "quit",
        label: "quit propman",
        action: Action::Message(Message::Quit),
    },
    Command {
        name: "edit",
        label: "edit the file under the cursor",
        action: Action::Message(Message::Edit),
    },
    Command {
        name: "mark",
        label: "mark or unmark the item under the cursor",
        action: Action::Message(Message::ToggleMark),
    },
    Command {
        name: "clear-marks",
        label: "clear all marks",
        action: Action::Message(Message::ClearMarks),
    },
    Command {
        name: "encoding",
        label: "set the encoding of the selected files",
        action: Action::Choose(&ENCODINGS),
    },
    Command {
        name: "excluded",
        label: "set whether the selected bundles are left out",
        action: Action::Choose(&YES_NO),
    },
    Command {
        name: "preview",
        label: "show or hide the preview",
        action: Action::Message(Message::TogglePreview),
    },
    Command {
        name: "preview-up",
        label: "scroll the preview up by one line",
        action: Action::Message(Message::PreviewUp),
    },
    Command {
        name: "preview-down",
        label: "scroll the preview down by one line",
        action: Action::Message(Message::PreviewDown),
    },
    Command {
        name: "preview-half-up",
        label: "scroll the preview up by half a page",
        action: Action::Message(Message::PreviewHalfPageUp),
    },
    Command {
        name: "preview-half-down",
        label: "scroll the preview down by half a page",
        action: Action::Message(Message::PreviewHalfPageDown),
    },
    Command {
        name: "up",
        label: "move the cursor up",
        action: Action::Message(Message::Up),
    },
    Command {
        name: "down",
        label: "move the cursor down",
        action: Action::Message(Message::Down),
    },
    Command {
        name: "page-up",
        label: "move the cursor up by a page",
        action: Action::Message(Message::PageUp),
    },
    Command {
        name: "page-down",
        label: "move the cursor down by a page",
        action: Action::Message(Message::PageDown),
    },
    Command {
        name: "first",
        label: "move the cursor to the first item",
        action: Action::Message(Message::First),
    },
    Command {
        name: "last",
        label: "move the cursor to the last item",
        action: Action::Message(Message::Last),
    },
];

/// The commands, in the order in which they are listed.
pub fn commands() -> &'static [Command] {
    COMMANDS
}

/// What a candidate is: a command, or a value of the command that was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Command(&'static Command),
    Value(&'static Value),
}

/// An entry that fits what is typed, with the places (in characters of its name) of the letters
/// that were hit.
#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: &'static str,
    pub label: &'static str,
    pub matched: Vec<usize>,
    pub entry: Entry,
}

/// How a name fits the input: the group (0, 1 or 2, see above) and the places of the letters.
fn fit(name: &str, input: &[char]) -> Option<(u8, Vec<usize>)> {
    let name: Vec<char> = name.to_lowercase().chars().collect();
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

/// Whether the command can be done now: for one with values, with any of them.
fn available(state: &AppState, command: &Command) -> bool {
    match &command.action {
        Action::Message(message) => applicable(state, *message),
        Action::Choose(values) => values
            .first()
            .is_some_and(|value| applicable(state, value.message)),
    }
}

/// The candidates for the input of the command line, in order; none if it is closed. Before a
/// command is chosen they are the commands, after one with values its values.
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
    let entries: Vec<(&'static str, &'static str, Entry)> = match line.chosen() {
        None => commands()
            .iter()
            .filter(|command| available(state, command))
            .map(|command| (command.name, command.label, Entry::Command(command)))
            .collect(),
        Some(Command {
            action: Action::Choose(values),
            ..
        }) => values
            .iter()
            .map(|value| (value.name, value.label, Entry::Value(value)))
            .collect(),
        Some(_) => Vec::new(),
    };
    let mut fitting: Vec<(u8, Candidate)> = entries
        .into_iter()
        .filter_map(|(name, label, entry)| {
            let (group, matched) = fit(name, &input)?;
            Some((
                group,
                Candidate {
                    name,
                    label,
                    matched,
                    entry,
                },
            ))
        })
        .collect();
    // the sort is stable: within a group the order of the table stays
    fitting.sort_by_key(|(group, _)| *group);
    fitting
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}
