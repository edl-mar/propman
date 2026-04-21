use crate::{
    state::{AppState, SelectionScope},
    view_model::{CellContent, ViewRow},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

// ── PropertiesWidget ──────────────────────────────────────────────────────────

/// Top-level widget: renders a viewport window of `state.view_rows`.
pub struct PropertiesWidget<'a> {
    state: &'a AppState,
}

impl<'a> PropertiesWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

impl Widget for PropertiesWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let vp_height = area.height as usize;
        let state = self.state;

        if vp_height == 0 || state.view_rows.is_empty() {
            return;
        }

        let start = state.scroll_offset.min(state.view_rows.len());
        let end = (start + vp_height).min(state.view_rows.len());
        // Use effective_locale_idx so that when cursor_locale names a locale
        // the current bundle doesn't have, we highlight the nearest available
        // column to the left instead of leaving nothing highlighted.
        // cursor_locale itself is unchanged (preserved as the sticky preference).
        let cursor_locale: Option<&str> = state.cursor_locale.as_ref().and_then(|_| {
            state
                .effective_locale_idx()
                .and_then(|idx| state.visible_locales.get(idx))
                .map(|s| s.as_str())
        });

        // Scope prefix: rows whose identity.prefix starts with this string are
        // "in scope" (children of the anchor) when Children/ChildrenAll is active.
        let scope_pfx: Option<String> = if matches!(
            state.selection_scope,
            SelectionScope::Children | SelectionScope::ChildrenAll
        ) {
            Some(format!("{}.", state.anchor_prefix()))
        } else {
            None
        };

        for (screen_idx, row_idx) in (start..end).enumerate() {
            let row = &state.view_rows[row_idx];
            let is_cursor_row = row_idx == state.cursor_row;
            let y = area.y + screen_idx as u16;

            let is_in_scope = !is_cursor_row
                && scope_pfx
                    .as_ref()
                    .map_or(false, |p| row.identity.prefix_str().starts_with(p.as_str()));

            let is_bundle_hdr = row.identity.is_bundle_header();

            if is_bundle_hdr {
                draw_bundle_header(row, is_cursor_row, cursor_locale, is_in_scope, buf, area, y);
            } else if !row.identity.is_leaf {
                draw_group_header(row, is_cursor_row, cursor_locale, is_in_scope, buf, area, y);
            } else {
                draw_leaf(
                    row,
                    is_cursor_row,
                    cursor_locale,
                    state.cursor_segment,
                    is_in_scope,
                    buf,
                    area,
                    y,
                );
            }
        }
    }
}

// ── Row drawing ───────────────────────────────────────────────────────────────

/// `bundlename:[locale1][locale2]…`
fn draw_bundle_header(
    row: &ViewRow,
    is_cursor_row: bool,
    cursor_locale: Option<&str>,
    is_in_scope: bool,
    buf: &mut Buffer,
    area: Rect,
    y: u16,
) {
    let is_key_sel = is_cursor_row && cursor_locale.is_none();
    let name_style = if is_key_sel {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let default_tag = if is_in_scope {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut spans: Vec<Span> = vec![
        Span::styled(row.identity.bundle_name().to_string(), name_style),
        Span::styled(":", default_tag),
    ];
    for cell in &row.locale_cells {
        if matches!(&cell.content, CellContent::Tag) {
            let is_locale_sel = is_cursor_row && cursor_locale == Some(cell.locale.as_str());
            let style = if is_locale_sel {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::REVERSED)
            } else {
                default_tag
            };
            spans.push(Span::styled(format!("[{}]", cell.locale), style));
        }
    }
    buf.set_line(area.x, y, &Line::from(spans), area.width);
}

/// `{indent}.{key_segments}:[locale_tags]`
fn draw_group_header(
    row: &ViewRow,
    is_cursor_row: bool,
    cursor_locale: Option<&str>,
    is_in_scope: bool,
    buf: &mut Buffer,
    area: Rect,
    y: u16,
) {
    let is_sel = is_cursor_row && cursor_locale.is_none();
    let seg_style = if is_sel {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else if is_in_scope {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let pad_style = if is_sel || is_in_scope {
        seg_style
    } else {
        Style::default()
    };
    let dot_style = Style::default().fg(Color::DarkGray);
    let tag_style = if is_in_scope {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let base = usize::from(!row.identity.bundle_name().is_empty());
    let gi = row.indent.saturating_sub(base);

    let mut spans = group_indent_spans(gi, pad_style, dot_style);
    spans.extend(key_segs_spans(&row.key_segments, seg_style, dot_style));
    spans.push(Span::styled(":", tag_style));
    for cell in &row.locale_cells {
        if matches!(&cell.content, CellContent::Tag) {
            let is_locale_sel = is_cursor_row && cursor_locale == Some(cell.locale.as_str());
            let style = if is_locale_sel {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::REVERSED)
            } else {
                tag_style
            };
            spans.push(Span::styled(format!("[{}]", cell.locale), style));
        }
    }
    buf.set_line(area.x, y, &Line::from(spans), area.width);
}

/// `{pin}{dirty}{indent}.{key_segments}:{locale_values}`
fn draw_leaf(
    row: &ViewRow,
    is_cursor_row: bool,
    cursor_locale: Option<&str>,
    cursor_segment: usize,
    is_in_scope: bool,
    buf: &mut Buffer,
    area: Rect,
    y: u16,
) {
    let is_key_sel = is_cursor_row && cursor_locale.is_none();
    let is_dirty = row.identity.is_dirty;
    let is_dangling = row.identity.is_dangling;
    let is_pinned = row.identity.is_pinned;
    let is_temp = row.identity.is_temp_pinned;
    let dot_style = Style::default().fg(Color::DarkGray);
    let sep_style = Style::default().fg(Color::DarkGray);

    let key_style = if is_key_sel && !is_temp {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else if is_in_scope && !is_temp {
        Style::default().add_modifier(Modifier::REVERSED)
    } else if is_dirty {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if is_temp {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    let base = usize::from(!row.identity.bundle_name().is_empty());
    let gi = row.indent.saturating_sub(base);

    let mut spans = leaf_indent_spans(
        gi,
        is_key_sel,
        is_dirty,
        is_dangling,
        is_pinned,
        is_temp,
        key_style,
        dot_style,
    );

    // Key segments.  When cursor_segment > 0 on a multi-segment chain-collapsed
    // row, dim the segments to the right of the anchor.
    let segs = &row.key_segments;
    let n = segs.len();
    if is_key_sel && cursor_segment > 0 && n > 1 {
        let anchor_idx = n.saturating_sub(1 + cursor_segment);
        let dim_style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);
        for (i, seg) in segs.iter().enumerate() {
            if i > 0 {
                let sep = if i <= anchor_idx {
                    dot_style
                } else {
                    dim_style
                };
                spans.push(Span::styled(".", sep));
            }
            let s = if i <= anchor_idx {
                key_style
            } else {
                dim_style
            };
            spans.push(Span::styled(seg.clone(), s));
        }
    } else {
        spans.extend(key_segs_spans(segs, key_style, dot_style));
    }

    // Locale values.
    spans.push(Span::styled(":", sep_style));
    let mut first = true;
    for cell in &row.locale_cells {
        if !cell.visible {
            continue;
        }
        if !first {
            spans.push(Span::raw("  "));
        }
        first = false;
        let is_locale_sel = is_cursor_row && cursor_locale == Some(cell.locale.as_str());
        spans.extend(locale_cell_spans(
            &cell.locale,
            &cell.content,
            is_locale_sel,
            cell.dirty,
            is_in_scope,
        ));
    }

    buf.set_line(area.x, y, &Line::from(spans), area.width);
}

// ── Indent span helpers ───────────────────────────────────────────────────────

/// Leading spans for a group-header row.
///
/// gi=0 → `"  "` (two spaces, no dot)
/// gi=1 → `"   ."` (three spaces + dot)
/// gi=N → `" ".repeat(2*(N+1)-1) + "."`
fn group_indent_spans(gi: usize, pad_style: Style, dot_style: Style) -> Vec<Span<'static>> {
    let indent_len = 2 * (gi + 1);
    if gi == 0 {
        vec![Span::styled(" ".repeat(indent_len), pad_style)]
    } else {
        vec![
            Span::raw(" ".repeat(indent_len - 1)),
            Span::styled(".", dot_style),
        ]
    }
}

/// Leading spans for a leaf row (pin marker, dirty marker, indentation, dot).
fn leaf_indent_spans(
    gi: usize,
    is_anchor: bool,
    is_dirty: bool,
    is_dangling: bool,
    is_pinned: bool,
    is_temp_pinned: bool,
    pad_style: Style,
    dot_style: Style,
) -> Vec<Span<'static>> {
    let indent_len = 2 * (gi + 1);

    let p0: Span<'static> = if is_pinned {
        Span::styled("@", Style::default().fg(Color::Cyan))
    } else if is_temp_pinned {
        Span::styled("~", Style::default().fg(Color::Blue))
    } else {
        Span::styled(" ", pad_style)
    };

    let p1: Span<'static> = if is_dirty {
        let s = if is_anchor {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Yellow)
        };
        Span::styled("#", s)
    } else if is_dangling {
        Span::styled("*", Style::default().fg(Color::DarkGray))
    } else {
        Span::styled(" ", pad_style)
    };

    if gi == 0 {
        vec![p0, p1]
    } else {
        vec![
            p0,
            p1,
            Span::raw(" ".repeat(indent_len - 3)),
            Span::styled(".", dot_style),
        ]
    }
}

// ── Key segment spans ─────────────────────────────────────────────────────────

fn key_segs_spans(segs: &[String], style: Style, dot_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(".", dot_style));
        }
        spans.push(Span::styled(seg.clone(), style));
    }
    spans
}

// ── Locale cell spans ─────────────────────────────────────────────────────────

fn locale_cell_spans(
    locale: &str,
    content: &CellContent,
    is_selected: bool,
    is_dirty: bool,
    is_in_scope: bool,
) -> Vec<Span<'static>> {
    let (tag, locale_tag_style) = if is_dirty {
        (format!("#[{locale}]"), Style::default().fg(Color::Yellow))
    } else if is_in_scope {
        (
            format!("[{locale}]"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )
    } else {
        (format!("[{locale}]"), Style::default().fg(Color::DarkGray))
    };

    let (text, val_style) = match content {
        CellContent::Missing => (
            "<missing>".to_string(),
            if is_selected {
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Red)
            },
        ),
        CellContent::Value(v) => {
            let display = v.replace("\\\n", "");
            let s = match (is_selected, is_dirty) {
                (true, true) => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                (true, false) => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                (false, true) => Style::default().fg(Color::Yellow),
                (false, false) => Style::default(),
            };
            (display, s)
        }
        // Tag / Empty: only meaningful on header rows, not leaf locale cells.
        CellContent::Tag | CellContent::Empty => (String::new(), Style::default()),
    };

    vec![
        Span::styled(tag, locale_tag_style),
        Span::raw(" "),
        Span::styled(text, val_style),
    ]
}
