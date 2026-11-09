//! The screen as text, to show to someone who cannot see it: for finding out how the program
//! looks without a picture. It is the rows of the screen, and below them the cells that are
//! not drawn in the plain style, one line for every run of equal style in a row.
//!
//! ```text
//! screen 60x8
//! text:
//! propman - fixtures/project
//! ...
//! styles:
//! row 2, columns 0-59: bold reversed
//! ```

use app::AppState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};

use crate::{Ui, render};

/// The style in words, or `None` for the plain style.
fn describe(style: Style) -> Option<String> {
    let mut words = Vec::new();
    if let Some(color) = style.fg.filter(|color| *color != Color::Reset) {
        words.push(format!("fg={color:?}"));
    }
    if let Some(color) = style.bg.filter(|color| *color != Color::Reset) {
        words.push(format!("bg={color:?}"));
    }
    let modifiers = [
        (Modifier::BOLD, "bold"),
        (Modifier::DIM, "dim"),
        (Modifier::ITALIC, "italic"),
        (Modifier::UNDERLINED, "underlined"),
        (Modifier::SLOW_BLINK, "slow-blink"),
        (Modifier::RAPID_BLINK, "rapid-blink"),
        (Modifier::REVERSED, "reversed"),
        (Modifier::HIDDEN, "hidden"),
        (Modifier::CROSSED_OUT, "crossed-out"),
    ];
    for (modifier, word) in modifiers {
        if style.add_modifier.contains(modifier) {
            words.push(word.to_owned());
        }
    }
    (!words.is_empty()).then(|| words.join(" "))
}

/// The text of the screen as `render` draws the state on a screen of this size.
pub fn screen_text(ui: &Ui, state: &AppState, width: u16, height: u16) -> String {
    // drawing into memory cannot fail
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height));
    let Ok(_) = terminal.draw(|frame| render(ui, state, frame));
    let buffer = terminal.backend().buffer();

    let mut text = format!("screen {width}x{height}\ntext:\n");
    let mut styles = String::new();
    for y in 0..height {
        let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        text.push_str(row.trim_end());
        text.push('\n');

        // runs of equal style in this row
        let mut run: Option<(u16, String)> = None;
        for x in 0..=width {
            let style = if x < width {
                describe(buffer[(x, y)].style())
            } else {
                None
            };
            let continues = matches!((&run, &style), (Some((_, open)), Some(now)) if open == now);
            if continues {
                continue;
            }
            if let Some((start, open)) = run.take() {
                styles.push_str(&format!("row {y}, columns {start}-{}: {open}\n", x - 1));
            }
            run = style.map(|style| (x, style));
        }
    }
    text.push_str("styles:\n");
    text.push_str(&styles);
    text
}
