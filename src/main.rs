mod editor;
mod filter;
mod keybindings;
mod messages;
mod parser;
mod render_model;
mod search;
mod state;
mod tui;
mod update;
mod workspace;
mod writer;

use std::path::PathBuf;
use anyhow::Result;

fn main() -> Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"));

    let workspace = workspace::Workspace::load(&dir)?;
    let state = state::AppState::new(workspace);
    let keybindings = keybindings::default_keybindings();

    tui::run(state, keybindings)?;
    Ok(())
}
