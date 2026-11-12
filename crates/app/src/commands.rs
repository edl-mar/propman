//! The commands: actions with a name, which are typed in the command line and bound to keys.
//!
//! A command is a name, a label and what it does: a message at once, or a tree of choices that
//! leads to a message (`encoding Windows-1252`, `sel locale de`). The choices are made from the
//! state when they are needed, so they can be what is there (the locales of the files). The names
//! are what the user types and what a settings file will use. The candidates for what is typed are
//! the entries of the current level of the tree (at first the commands) whose name contains the
//! input as a subsequence, without regard to case, and that can be done now. They are ordered the
//! same way every time, so that the fingers can learn it: first the names that begin with the
//! input, then those whose words begin with the letters of the input (`pd` for `preview-down`,
//! the words may be skipped), then any other subsequence; within one group in the order of the
//! list.

use std::collections::BTreeMap;

use store::{BundleId, TextId};
use types::Encoding;

use crate::overview::relative_folder;
use crate::selection::applicable_in_line;
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

/// What a file has that a condition can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predicate {
    /// The locale of the file; `None` is the default file of a bundle.
    Locale(Option<TextId>),
    Bundle(BundleId),
    Encoding(Encoding),
}

/// One choice of a level: its name, what it is, and where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    pub label: String,
    pub then: Then,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// The choice completes the line: this is what it stands for.
    Run(Message),
    /// More has to be chosen.
    Choose(Vec<Choice>),
}

#[derive(Debug)]
pub enum Action {
    /// The command is done at once.
    Message(Message),
    /// The command needs choices, made from the state.
    Choose(fn(&AppState) -> Vec<Choice>),
}

#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub label: &'static str,
    pub action: Action,
}

const ENCODINGS: [(Encoding, &str); 4] = [
    (Encoding::Utf8, "UTF-8, without a byte order mark"),
    (Encoding::Utf8Bom, "UTF-8 with a byte order mark"),
    (Encoding::Windows1252, "Windows-1252 (cp1252)"),
    (Encoding::Latin1, "ISO-8859-1 (latin1)"),
];

fn encoding_choices(_state: &AppState) -> Vec<Choice> {
    ENCODINGS
        .iter()
        .map(|&(encoding, label)| Choice {
            name: encoding_name(encoding).to_owned(),
            label: label.to_owned(),
            then: Then::Run(Message::SetEncoding(encoding)),
        })
        .collect()
}

fn excluded_choices(_state: &AppState) -> Vec<Choice> {
    [
        ("yes", "leave it out of the session", true),
        ("no", "take it into the session", false),
    ]
    .into_iter()
    .map(|(name, label, excluded)| Choice {
        name: name.to_owned(),
        label: label.to_owned(),
        then: Then::Run(Message::SetExcluded(excluded)),
    })
    .collect()
}

/// "1 file" or "3 files".
fn files(count: usize) -> String {
    format!("{count} {}", if count == 1 { "file" } else { "files" })
}

fn select(predicate: Predicate) -> Then {
    Then::Run(Message::Select(predicate))
}

/// The locales that the files have, the default file first, then by name; each with the number
/// of files.
fn locale_choices(state: &AppState) -> Vec<Choice> {
    let store = state.session().store();
    let mut counts: BTreeMap<Option<String>, (Option<TextId>, usize)> = BTreeMap::new();
    for file in store.files() {
        let Some(assignment) = store.file_assignment(file) else {
            continue;
        };
        let name = assignment
            .locale
            .and_then(|locale| store.text(locale))
            .map(str::to_owned);
        counts.entry(name).or_insert((assignment.locale, 0)).1 += 1;
    }
    counts
        .into_iter()
        .map(|(name, (locale, count))| Choice {
            name: name.unwrap_or_else(|| "(default)".to_owned()),
            label: files(count),
            then: select(Predicate::Locale(locale)),
        })
        .collect()
}

/// The bundles by name; two bundles with one name are told apart by their folders.
fn bundle_choices(state: &AppState) -> Vec<Choice> {
    let store = state.session().store();
    let mut found: Vec<(String, String, BundleId)> = store
        .bundles()
        .into_iter()
        .filter_map(|bundle| {
            let name = store.bundle_name(bundle)?.to_owned();
            let folder = relative_folder(state.session().root(), store.bundle_folder(bundle)?);
            Some((name, folder, bundle))
        })
        .collect();
    found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let repeated: Vec<String> = found
        .iter()
        .filter(|(name, _, _)| found.iter().filter(|(other, _, _)| other == name).count() > 1)
        .map(|(name, _, _)| name.clone())
        .collect();
    found
        .into_iter()
        .map(|(name, folder, bundle)| {
            let count = store.files_of(bundle).len();
            Choice {
                name: if repeated.contains(&name) {
                    format!("{name} ({folder})")
                } else {
                    name
                },
                label: files(count),
                then: select(Predicate::Bundle(bundle)),
            }
        })
        .collect()
}

/// The encodings that files have, in the order of the encodings.
fn present_encoding_choices(state: &AppState) -> Vec<Choice> {
    let store = state.session().store();
    ENCODINGS
        .iter()
        .filter_map(|&(encoding, _)| {
            let count = store
                .files()
                .into_iter()
                .filter(|&file| store.file_encoding(file) == Some(encoding))
                .count();
            (count > 0).then(|| Choice {
                name: encoding_name(encoding).to_owned(),
                label: files(count),
                then: select(Predicate::Encoding(encoding)),
            })
        })
        .collect()
}

fn sel_choices(state: &AppState) -> Vec<Choice> {
    vec![
        Choice {
            name: "locale".to_owned(),
            label: "the files of one locale".to_owned(),
            then: Then::Choose(locale_choices(state)),
        },
        Choice {
            name: "bundle".to_owned(),
            label: "the files of one bundle".to_owned(),
            then: Then::Choose(bundle_choices(state)),
        },
        Choice {
            name: "encoding".to_owned(),
            label: "the files with one encoding".to_owned(),
            then: Then::Choose(present_encoding_choices(state)),
        },
    ]
}

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
        name: "sel",
        label: "select files by what they have",
        action: Action::Choose(sel_choices),
    },
    Command {
        name: "clear-selection",
        label: "clear the marks and the condition",
        action: Action::Message(Message::ClearSelection),
    },
    Command {
        name: "encoding",
        label: "set the encoding of the selected files",
        action: Action::Choose(encoding_choices),
    },
    Command {
        name: "excluded",
        label: "set whether the selected bundles are left out",
        action: Action::Choose(excluded_choices),
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

/// What a candidate is.
#[derive(Debug, Clone, Copy)]
pub enum Entry {
    Command(&'static Command),
    /// A choice below a command: it completes the line with this message, or leads on if there
    /// is none.
    Choice {
        complete: Option<Message>,
    },
}

/// An entry that fits what is typed, with the places (in characters of its name) of the letters
/// that were hit.
#[derive(Debug)]
pub struct Candidate {
    pub name: String,
    pub label: String,
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

/// The message of the first choice that completes the line, to ask whether it can be done.
fn first_message(choices: &[Choice]) -> Option<Message> {
    choices.iter().find_map(|choice| match &choice.then {
        Then::Run(message) => Some(*message),
        Then::Choose(more) => first_message(more),
    })
}

/// Whether the command can be done now: for one with choices, with any of them.
fn available(state: &AppState, command: &Command) -> bool {
    match &command.action {
        Action::Message(message) => applicable_in_line(state, *message),
        Action::Choose(choices) => {
            first_message(&choices(state)).is_some_and(|message| applicable_in_line(state, message))
        }
    }
}

/// The choices below the segments that were chosen, if they lead somewhere.
pub(crate) fn choices_below(state: &AppState, segments: &[String]) -> Option<Vec<Choice>> {
    let (first, rest) = segments.split_first()?;
    let command = commands().iter().find(|command| command.name == *first)?;
    let Action::Choose(make) = command.action else {
        return None;
    };
    let mut level = make(state);
    for name in rest {
        let next = level.into_iter().find(|choice| choice.name == *name)?;
        match next.then {
            Then::Choose(more) => level = more,
            Then::Run(_) => return None,
        }
    }
    Some(level)
}

/// The candidates for the input of the command line, in order; none if it is closed. Before a
/// command is chosen they are the commands, after one the choices below it.
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
    let entries: Vec<(String, String, Entry)> = if line.segments().is_empty() {
        commands()
            .iter()
            .filter(|command| available(state, command))
            .map(|command| {
                (
                    command.name.to_owned(),
                    command.label.to_owned(),
                    Entry::Command(command),
                )
            })
            .collect()
    } else {
        choices_below(state, line.segments())
            .unwrap_or_default()
            .into_iter()
            .map(|choice| {
                let complete = match choice.then {
                    Then::Run(message) => Some(message),
                    Then::Choose(_) => None,
                };
                (choice.name, choice.label, Entry::Choice { complete })
            })
            .collect()
    };
    let mut fitting: Vec<(u8, Candidate)> = entries
        .into_iter()
        .filter_map(|(name, label, entry)| {
            let (group, matched) = fit(&name, &input)?;
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
    // the sort is stable: within a group the order of the list stays
    fitting.sort_by_key(|(group, _)| *group);
    fitting
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}
