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

use crate::overview::overview;
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

/// Which item of the overview is selected and which one is shown first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverviewState {
    selected: usize,
    scroll: usize,
    height: usize,
}

impl OverviewState {
    /// Applies a message about the selection, for a list of `count` items.
    fn apply(&mut self, message: Message, count: usize) {
        let last = count.saturating_sub(1);
        let page = self.height.max(1);
        match message {
            Message::Up => self.selected = self.selected.saturating_sub(1),
            Message::Down => self.selected += 1,
            Message::PageUp => self.selected = self.selected.saturating_sub(page),
            Message::PageDown => self.selected += page,
            Message::First => self.selected = 0,
            Message::Last => self.selected = last,
            Message::Viewport(height) => self.height = height,
            Message::Quit => {}
        }
        self.selected = self.selected.min(last);
        self.follow(count);
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
}

pub fn update(mut state: AppState, message: Message) -> AppState {
    match message {
        Message::Quit => state.quit = true,
        movement => {
            let count = overview(&state.session).len();
            let View::Overview(view) = &mut state.view;
            view.apply(movement, count);
        }
    }
    state
}
