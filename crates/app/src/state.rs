//! The state of the user interface and how messages change it, without any terminal library.
//!
//! The structure is inspired by The Elm Architecture: a message says what happened, `update`
//! takes the state by value and returns the new one, and the view only reads the state. The
//! state holds the session and what went wrong while loading it; what is shown and chosen (the
//! current view, the selection) belongs here too and never in the store.
//!
//! The bundle overview is the first view. Its selection and scroll position are indices into the
//! list that `overview` gives. The height of the visible part of the list is told by a message,
//! because only the terminal knows it; until then it is 0, which means unknown. The scroll
//! position moves as little as possible to keep the selection visible.
//!
//! The preview of the selected file is part of the overview state: whether it is on and how far
//! it is scrolled. The scroll position starts at the top again when the selection changes and
//! never goes past the last window of the file. What the preview shows is `preview`.

use crate::overview::{OverviewItem, overview};
use crate::session::{LoadProblem, Session};

pub struct AppState {
    session: Session,
    problems: Vec<LoadProblem>,
    quit: bool,
    view: View,
}

impl AppState {
    pub fn new(session: Session, problems: Vec<LoadProblem>) -> Self {
        Self {
            session,
            problems,
            quit: false,
            view: View::Overview(OverviewState::default()),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// What went wrong while the session was loaded.
    pub fn problems(&self) -> &[LoadProblem] {
        &self.problems
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

/// Which item of the overview is selected and which one is shown first, and whether the preview
/// of the selected file is on and how far it is scrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverviewState {
    selected: usize,
    scroll: usize,
    height: usize,
    preview: bool,
    preview_scroll: usize,
}

impl Default for OverviewState {
    fn default() -> Self {
        Self {
            selected: 0,
            scroll: 0,
            height: 0,
            preview: true,
            preview_scroll: 0,
        }
    }
}

impl OverviewState {
    /// Applies a message about the selection or the preview, for a list of `count` items.
    /// `lines_of` gives the number of lines of the file at an index of the list, or `None` if the
    /// item is not a file.
    fn apply(&mut self, message: Message, count: usize, lines_of: impl Fn(usize) -> Option<usize>) {
        let last = count.saturating_sub(1);
        let page = self.height.max(1);
        let half = (self.preview_rows() / 2).max(1);
        let before = self.selected;
        match message {
            Message::Up => self.selected = self.selected.saturating_sub(1),
            Message::Down => self.selected += 1,
            Message::PageUp => self.selected = self.selected.saturating_sub(page),
            Message::PageDown => self.selected += page,
            Message::First => self.selected = 0,
            Message::Last => self.selected = last,
            Message::Viewport(height) => self.height = height,
            Message::TogglePreview => self.preview = !self.preview,
            Message::PreviewUp => self.preview_scroll = self.preview_scroll.saturating_sub(1),
            Message::PreviewDown => self.preview_scroll += 1,
            Message::PreviewHalfPageUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(half);
            }
            Message::PreviewHalfPageDown => self.preview_scroll += half,
            Message::Quit => {}
        }
        self.selected = self.selected.min(last);
        self.follow(count);
        if self.selected != before {
            self.preview_scroll = 0;
        }
        let furthest =
            lines_of(self.selected).map_or(0, |lines| lines.saturating_sub(self.preview_rows()));
        self.preview_scroll = self.preview_scroll.min(furthest);
    }

    /// The rows of the preview that are lines of the file: the height of the list without the
    /// row of the header.
    pub(crate) fn preview_rows(&self) -> usize {
        self.height.saturating_sub(1)
    }

    /// Moves the scroll position as little as possible to keep the selection visible, and not
    /// further than the last item at the bottom.
    fn follow(&mut self, count: usize) {
        if self.height == 0 {
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.height {
            self.scroll = self.selected + 1 - self.height;
        }
        self.scroll = self.scroll.min(count.saturating_sub(self.height));
    }

    /// The index of the selected item.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The index of the first item that is shown.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// The number of items that fit; 0 as long as it is not known.
    pub fn height(&self) -> usize {
        self.height
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
    /// Switches the preview of the selected file on or off.
    TogglePreview,
    /// Scrolls the preview by one line.
    PreviewUp,
    PreviewDown,
    /// Scrolls the preview by half of its height.
    PreviewHalfPageUp,
    PreviewHalfPageDown,
}

pub fn update(mut state: AppState, message: Message) -> AppState {
    match message {
        Message::Quit => state.quit = true,
        other => {
            let items = overview(&state.session);
            let lines_of = |index: usize| match items.get(index) {
                Some(OverviewItem::File { lines, .. }) => Some(*lines),
                _ => None,
            };
            let View::Overview(view) = &mut state.view;
            view.apply(other, items.len(), lines_of);
        }
    }
    state
}
