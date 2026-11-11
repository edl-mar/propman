//! The state of the user interface and how messages change it, without any terminal library.
//!
//! The structure is inspired by The Elm Architecture: a message says what happened, `update`
//! takes the state by value and returns the new one together with at most one effect (see
//! `Effect`), and the view only reads the state. The
//! state holds the session and what went wrong while loading it; what is shown and chosen (the
//! current view, the cursor) belongs here too and never in the store.
//!
//! The bundle overview is the first view. Its cursor is the id of an item (`ItemId`), found again
//! in the list that `overview` gives after every message, so that it stays on its item when the
//! items change their order (`ListState` does this). The height of the visible part of the list
//! is told by a message, because only the terminal knows it; until then it is 0, which means
//! unknown.
//!
//! The preview of the file under the cursor is part of the overview state: whether it is on and how far
//! it is scrolled. The scroll position starts at the top again when the cursor moves to another item and
//! never goes past the last window of the file. What the preview shows is `preview`.

use store::FileId;

use crate::effect::{EditorOutcome, Effect};
use crate::list::{ListState, Movement};
use crate::overview::{ItemId, OverviewItem, overview};
use crate::selection::selection;
use crate::session::{LoadProblem, ReloadError, Session};

pub struct AppState {
    session: Session,
    problems: Vec<LoadProblem>,
    quit: bool,
    alert: Option<String>,
    view: View,
}

impl AppState {
    pub fn new(session: Session, problems: Vec<LoadProblem>) -> Self {
        Self {
            session,
            problems,
            quit: false,
            alert: None,
            view: View::Overview(OverviewState::default()),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub(crate) fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// What went wrong while the session was loaded.
    pub fn problems(&self) -> &[LoadProblem] {
        &self.problems
    }

    /// What went wrong, as a text that the user has to dismiss before going on.
    pub fn alert(&self) -> Option<&str> {
        self.alert.as_deref()
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn view(&self) -> &View {
        &self.view
    }
}

/// The views of the program. For now there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Overview(OverviewState),
}

/// The cursor and the scroll position in the list of the bundle overview, and whether the preview
/// of the file under the cursor is on and how far it is scrolled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewState {
    list: ListState<ItemId>,
    marks: Vec<ItemId>,
    preview: bool,
    preview_scroll: usize,
}

impl Default for OverviewState {
    fn default() -> Self {
        Self {
            list: ListState::new(),
            marks: Vec::new(),
            preview: true,
            preview_scroll: 0,
        }
    }
}

fn ids_of(items: &[OverviewItem]) -> Vec<ItemId> {
    items.iter().map(OverviewItem::id).collect()
}

/// The number of lines of the file at an index of the list, `None` for any other item.
fn lines_at(items: &[OverviewItem], index: usize) -> Option<usize> {
    match items.get(index) {
        Some(OverviewItem::File { lines, .. }) => Some(*lines),
        _ => None,
    }
}

impl OverviewState {
    /// Applies a message about the cursor or the preview, for the items as they are now.
    fn apply(&mut self, message: Message, items: &[OverviewItem]) {
        let ids = ids_of(items);
        self.list.settle(&ids);
        let before = self.list.cursor().cloned();
        let half = (self.preview_rows() / 2).max(1);
        match message {
            Message::Up => self.list.apply(Movement::Up, &ids),
            Message::Down => self.list.apply(Movement::Down, &ids),
            Message::PageUp => self.list.apply(Movement::PageUp, &ids),
            Message::PageDown => self.list.apply(Movement::PageDown, &ids),
            Message::First => self.list.apply(Movement::First, &ids),
            Message::Last => self.list.apply(Movement::Last, &ids),
            Message::Viewport(height) => self.list.set_height(height, &ids),
            Message::TogglePreview => self.preview = !self.preview,
            Message::PreviewUp => self.preview_scroll = self.preview_scroll.saturating_sub(1),
            Message::PreviewDown => self.preview_scroll += 1,
            Message::PreviewHalfPageUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(half);
            }
            Message::PreviewHalfPageDown => self.preview_scroll += half,
            Message::ToggleMark => self.toggle_mark(),
            Message::ClearMarks => self.marks.clear(),
            Message::Quit
            | Message::DismissAlert
            | Message::Edit
            | Message::EditorClosed { .. }
            | Message::Reloaded { .. } => {}
        }
        if self.list.cursor() != before.as_ref() {
            self.preview_scroll = 0;
        }
        self.marks.retain(|mark| ids.contains(mark));
        self.keep_preview_inside(items);
    }

    /// Marks the item under the cursor or takes its mark away. The marks are of one kind: an
    /// item of another kind than the marked ones starts a new set.
    fn toggle_mark(&mut self) {
        let Some(id) = self.list.cursor().cloned() else {
            return;
        };
        if let Some(position) = self.marks.iter().position(|mark| *mark == id) {
            self.marks.remove(position);
            return;
        }
        let other_kind = self
            .marks
            .first()
            .is_some_and(|mark| std::mem::discriminant(mark) != std::mem::discriminant(&id));
        if other_kind {
            self.marks.clear();
        }
        self.marks.push(id);
    }

    /// Finds the cursor among the items again after they changed, and keeps the preview inside
    /// what there is.
    fn settle(&mut self, items: &[OverviewItem]) {
        let ids = ids_of(items);
        self.list.settle(&ids);
        self.marks.retain(|mark| ids.contains(mark));
        self.keep_preview_inside(items);
    }

    fn keep_preview_inside(&mut self, items: &[OverviewItem]) {
        let furthest = lines_at(items, self.list.index())
            .map_or(0, |lines| lines.saturating_sub(self.preview_rows()));
        self.preview_scroll = self.preview_scroll.min(furthest);
    }

    /// The rows of the preview that are lines of the file: the height of the list without the
    /// row of the header.
    pub(crate) fn preview_rows(&self) -> usize {
        self.list.height().saturating_sub(1)
    }

    /// The index of the item under the cursor.
    pub fn index(&self) -> usize {
        self.list.index()
    }

    /// The index of the first item that is shown.
    pub fn scroll(&self) -> usize {
        self.list.scroll()
    }

    /// The number of items that fit; 0 as long as it is not known.
    pub fn height(&self) -> usize {
        self.list.height()
    }

    /// The items that were marked by hand.
    pub fn marked(&self) -> &[ItemId] {
        &self.marks
    }

    pub fn is_marked(&self, id: &ItemId) -> bool {
        self.marks.contains(id)
    }

    /// Whether the preview is switched on (the view may still not draw it on a narrow screen).
    pub fn preview_on(&self) -> bool {
        self.preview
    }

    /// The index of the first line of the file that the preview shows.
    pub fn preview_scroll(&self) -> usize {
        self.preview_scroll
    }
}

/// What happened, as far as the state is concerned. It grows with every step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    Quit,
    Up,
    Down,
    /// A page is as many items as fit, or one item if the height is not known.
    PageUp,
    PageDown,
    First,
    Last,
    /// The number of items of the overview that fit on the screen.
    Viewport(usize),
    /// Switches the preview of the file under the cursor on or off.
    TogglePreview,
    /// Scrolls the preview by one line.
    PreviewUp,
    PreviewDown,
    /// Scrolls the preview by half of its height.
    PreviewHalfPageUp,
    PreviewHalfPageDown,
    /// Marks the item under the cursor, or takes the mark away.
    ToggleMark,
    /// Takes all marks away.
    ClearMarks,
    /// Closes the alert.
    DismissAlert,
    /// Edits the file under the cursor in the editor of the user.
    Edit,
    /// The editor of the user was closed, or could not be run.
    EditorClosed {
        file: FileId,
        outcome: EditorOutcome,
    },
    /// The file was read from the disk again, or could not be.
    Reloaded {
        file: FileId,
        result: Result<(), ReloadError>,
    },
}

/// The new state for a message, and the effect that is to be performed, if there is one. While
/// there is an alert only quitting, closing it, the size of the screen and the results of the
/// outside world get through; what the keys of the view do is blocked.
pub fn update(mut state: AppState, message: Message) -> (AppState, Option<Effect>) {
    let gets_through = matches!(
        message,
        Message::Quit
            | Message::DismissAlert
            | Message::Viewport(_)
            | Message::EditorClosed { .. }
            | Message::Reloaded { .. }
    );
    if state.alert.is_some() && !gets_through {
        return (state, None);
    }
    let mut effect = None;
    match message {
        Message::Quit => state.quit = true,
        Message::DismissAlert => state.alert = None,
        Message::Edit => match edit_effect(&state) {
            Some(edit) => effect = Some(edit),
            None => state.alert = Some("edit needs exactly one file".to_owned()),
        },
        Message::EditorClosed { file, outcome } => match outcome {
            EditorOutcome::Done => effect = Some(Effect::Reload(file)),
            EditorOutcome::NoEditor => {
                state.alert = Some("no editor: set VISUAL or EDITOR".to_owned());
            }
            EditorOutcome::CouldNotStart => {
                state.alert = Some("could not start the editor".to_owned());
            }
            EditorOutcome::Failed(Some(code)) => {
                state.alert = Some(format!("the editor ended with exit code {code}"));
            }
            EditorOutcome::Failed(None) => {
                state.alert = Some("the editor ended with an error".to_owned());
            }
        },
        Message::Reloaded { result, .. } => {
            if let Err(error) = result {
                let text = match error {
                    ReloadError::UnknownFile => "unknown file",
                    ReloadError::Deleted => "the file was deleted",
                    ReloadError::Unreadable => "the file could not be read",
                    ReloadError::Undecodable => "the file could not be decoded",
                };
                state.alert = Some(text.to_owned());
            }
            // the file may have got shorter
            settle_overview(&mut state);
        }
        movement_or_preview => apply_to_overview(&mut state, movement_or_preview),
    }
    // whatever the message was, the rows may have changed
    settle_overview(&mut state);
    (state, effect)
}

/// The effect for editing the selection: it has to be exactly one file.
fn edit_effect(state: &AppState) -> Option<Effect> {
    let chosen = selection(state);
    let [ItemId::File(file)] = chosen.as_slice() else {
        return None;
    };
    let path = state.session.store().file_path(*file)?.to_path_buf();
    Some(Effect::Edit { file: *file, path })
}

fn apply_to_overview(state: &mut AppState, message: Message) {
    let items = overview(&state.session);
    let View::Overview(view) = &mut state.view;
    view.apply(message, &items);
}

fn settle_overview(state: &mut AppState) {
    let items = overview(&state.session);
    let View::Overview(view) = &mut state.view;
    view.settle(&items);
}
