//! Entry point only: CLI arguments and startup. No logic.

use std::process::ExitCode;

fn main() -> ExitCode {
    let folder = match propman::cli::parse(std::env::args_os().skip(1)) {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("propman: {error}\n{}", propman::cli::usage());
            return ExitCode::from(2);
        }
    };

    let (session, problems) = app::Session::open(&folder);
    match tui::run(app::AppState::new(session, problems)) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("propman: {error}");
            ExitCode::FAILURE
        }
    }
}
