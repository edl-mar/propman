//! The state of the user interface and how messages change it, without any terminal library.
//!
//! The structure is inspired by The Elm Architecture: a message says what happened, `update`
//! takes the state by value and returns the new one, and the view only reads the state. The
//! state holds the session and what went wrong while loading it; what is shown and chosen (the
//! current view, the selection) belongs here too and never in the store.

use crate::session::{LoadProblem, Session};

pub struct AppState {
    session: Session,
    problems: Vec<LoadProblem>,
    quit: bool,
}

impl AppState {
    pub fn new(session: Session, problems: Vec<LoadProblem>) -> Self {
        Self {
            session,
            problems,
            quit: false,
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
}

/// What happened, as far as the state is concerned. It grows with every step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    Quit,
}

pub fn update(mut state: AppState, message: Message) -> AppState {
    match message {
        Message::Quit => state.quit = true,
    }
    state
}
