//! TUI rendering with ratatui.
//!
//! Draws the grid layout with borders and cell content. The focused cell
//! gets a highlighted border. Supports ANSI colors and text selection.

use crate::grid::{CellRect, GridLayout};
use crate::terminal_cell::{CellColor, StyledChar, TerminalCell};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

/// Selection info passed to the renderer.
pub struct Selection {
    pub cell_index: usize,
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl Selection {
    /// Check if a position is within the selection (normalized).
    fn contains(&self, row: usize, col: usize) -> bool {
        let (start, end) = if self.start.0 < self.end.0
            || (self.start.0 == self.end.0 && self.start.1 <= self.end.1)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };

        if row < start.0 || row > end.0 {
            return false;
        }
        if start.0 == end.0 {
            col >= start.1 && col <= end.1
        } else if row == start.0 {
            col >= start.1
        } else if row == end.0 {
            col <= end.1
        } else {
            true
        }
    }
}

/// Convert CellColor to ratatui Color.
fn to_ratatui_color(color: CellColor) -> Option<Color> {
    match color {
        CellColor::Default => None,
        CellColor::Indexed(i) => Some(Color::Indexed(i)),
        CellColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Convert a StyledChar's style to a ratatui Style.
fn styled_char_style(sc: &StyledChar) -> Style {
    let mut style = Style::default();

    let (fg, bg) = if sc.style.reverse {
        (sc.style.bg, sc.style.fg)
    } else {
        (sc.style.fg, sc.style.bg)
    };

    if let Some(c) = to_ratatui_color(fg) {
        style = style.fg(c);
    } else {
        style = style.fg(Color::White);
    }

    if let Some(c) = to_ratatui_color(bg) {
        style = style.bg(c);
    }

    let mut modifiers = Modifier::empty();
    if sc.style.bold {
        modifiers |= Modifier::BOLD;
    }
    if sc.style.dim {
        modifiers |= Modifier::DIM;
    }
    if sc.style.italic {
        modifiers |= Modifier::ITALIC;
    }
    if sc.style.underline {
        modifiers |= Modifier::UNDERLINED;
    }
    if !modifiers.is_empty() {
        style = style.add_modifier(modifiers);
    }

    style
}

/// The full grid widget that renders all cells.
pub struct GridWidget<'a> {
    pub layout: GridLayout,
    pub cell_rects: &'a [CellRect],
    pub cells: &'a [TerminalCell],
    pub focus_index: usize,
    pub selection: Option<&'a Selection>,
}

impl<'a> Widget for GridWidget<'a> {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let visible_count = self.layout.cell_count().min(self.cell_rects.len());

        for i in 0..visible_count {
            let rect = self.cell_rects[i];
            let is_focused = i == self.focus_index;

            draw_border(buf, rect, is_focused);

            if i < self.cells.len() {
                let sel = self
                    .selection
                    .filter(|s| s.cell_index == i);
                draw_cell_content(buf, rect, &self.cells[i], sel);
            }
        }
    }
}

/// Draw a border around the cell rectangle.
fn draw_border(buf: &mut Buffer, rect: CellRect, focused: bool) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }

    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let x1 = rect.x;
    let y1 = rect.y;
    let x2 = rect.x + rect.width - 1;
    let y2 = rect.y + rect.height - 1;

    set_cell(buf, x1, y1, '┌', style);
    set_cell(buf, x2, y1, '┐', style);
    set_cell(buf, x1, y2, '└', style);
    set_cell(buf, x2, y2, '┘', style);

    for x in (x1 + 1)..x2 {
        set_cell(buf, x, y1, '─', style);
        set_cell(buf, x, y2, '─', style);
    }

    for y in (y1 + 1)..y2 {
        set_cell(buf, x1, y, '│', style);
        set_cell(buf, x2, y, '│', style);
    }
}

/// Draw the terminal content inside the cell's inner area with colors and selection.
fn draw_cell_content(buf: &mut Buffer, rect: CellRect, cell: &TerminalCell, selection: Option<&Selection>) {
    let inner = match rect.inner() {
        Some(i) => i,
        None => return,
    };

    let visible = cell.visible_lines();

    for (row_idx, line) in visible.iter().enumerate() {
        if row_idx as u16 >= inner.height {
            break;
        }
        let y = inner.y + row_idx as u16;

        for (col_idx, sc) in line.iter().enumerate() {
            if col_idx as u16 >= inner.width {
                break;
            }
            let x = inner.x + col_idx as u16;

            let is_selected = selection
                .map(|s| s.contains(row_idx, col_idx))
                .unwrap_or(false);

            let style = if is_selected {
                // Invert colors for selection
                let base = styled_char_style(sc);
                let fg = base.bg.unwrap_or(Color::Black);
                let bg = base.fg.unwrap_or(Color::White);
                Style::default().fg(fg).bg(bg)
            } else {
                styled_char_style(sc)
            };

            set_cell(buf, x, y, sc.ch, style);
        }
    }
}

/// Safely set a character in the buffer, checking bounds.
fn set_cell(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    let area = buf.area();
    if x >= area.x + area.width || y >= area.y + area.height {
        return;
    }
    if x < area.x || y < area.y {
        return;
    }
    let cell = &mut buf[(x, y)];
    cell.set_char(ch);
    cell.set_style(style);
}

/// Render a status bar at the bottom showing layout and focus info.
pub struct StatusBar {
    pub layout: GridLayout,
    pub focus_row: usize,
    pub focus_col: usize,
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout_name = match self.layout {
            GridLayout::Grid2x2 => "2x2",
            GridLayout::Grid3x3 => "3x3",
            GridLayout::Grid4x4 => "4x4",
        };
        let text = format!(
            " vpower-shell | Layout: {} | Focus: ({},{}) | Alt+G: cycle | Ctrl+Arrow: move | Ctrl+C/V: copy/paste | Ctrl+Q: quit ",
            layout_name, self.focus_row, self.focus_col
        );

        let style = Style::default().fg(Color::Black).bg(Color::Cyan);

        for (i, ch) in text.chars().enumerate() {
            let x = area.x + i as u16;
            if x >= area.x + area.width {
                break;
            }
            let cell = &mut buf[(x, area.y)];
            cell.set_char(ch);
            cell.set_style(style);
        }

        for x in (area.x + text.len() as u16)..(area.x + area.width) {
            let cell = &mut buf[(x, area.y)];
            cell.set_char(' ');
            cell.set_style(style);
        }
    }
}
