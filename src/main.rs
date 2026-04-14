mod app_model;
mod view_model;
mod editor;
mod filter;
mod keybindings;
mod messages;
mod ops;
mod parser;
mod radix_tree_arena;
mod search;
mod state;
mod tui;
mod update;
mod widgets;
mod workspace;
mod writer;

use anyhow::Result;
use std::path::PathBuf;

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
