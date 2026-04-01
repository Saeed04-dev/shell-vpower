//! TUI rendering with ratatui.
//!
//! Draws the grid layout with borders and cell content. The focused cell
//! gets a highlighted border to indicate which pane has input focus.

use crate::grid::{CellRect, GridLayout};
use crate::terminal_cell::TerminalCell;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

/// The full grid widget that renders all cells.
pub struct GridWidget<'a> {
    /// Current grid layout.
    pub layout: GridLayout,
    /// Computed cell rectangles.
    pub cell_rects: &'a [CellRect],
    /// Terminal cell states (may have more entries than visible cells).
    pub cells: &'a [TerminalCell],
    /// Index of the currently focused cell.
    pub focus_index: usize,
}

impl<'a> Widget for GridWidget<'a> {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let visible_count = self.layout.cell_count().min(self.cell_rects.len());

        for i in 0..visible_count {
            let rect = self.cell_rects[i];
            let is_focused = i == self.focus_index;

            // Draw border
            draw_border(buf, rect, is_focused);

            // Draw cell content
            if i < self.cells.len() {
                draw_cell_content(buf, rect, &self.cells[i]);
            }
        }
    }
}

/// Draw a border around the cell rectangle.
/// Focused cells get a bright cyan border; others get dark gray.
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

    // Corners
    set_cell(buf, x1, y1, '┌', style);
    set_cell(buf, x2, y1, '┐', style);
    set_cell(buf, x1, y2, '└', style);
    set_cell(buf, x2, y2, '┘', style);

    // Top and bottom edges
    for x in (x1 + 1)..x2 {
        set_cell(buf, x, y1, '─', style);
        set_cell(buf, x, y2, '─', style);
    }

    // Left and right edges
    for y in (y1 + 1)..y2 {
        set_cell(buf, x1, y, '│', style);
        set_cell(buf, x2, y, '│', style);
    }
}

/// Draw the terminal content inside the cell's inner area.
fn draw_cell_content(buf: &mut Buffer, rect: CellRect, cell: &TerminalCell) {
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

        for (col_idx, &ch) in line.iter().enumerate() {
            if col_idx as u16 >= inner.width {
                break;
            }
            let x = inner.x + col_idx as u16;
            set_cell(buf, x, y, ch, Style::default().fg(Color::White));
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
            " vpower-shell | Layout: {} | Focus: ({},{}) | Alt+G: cycle | Alt+Arrow: move | Ctrl+Q: quit ",
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

        // Fill remaining width with background
        for x in (area.x + text.len() as u16)..(area.x + area.width) {
            let cell = &mut buf[(x, area.y)];
            cell.set_char(' ');
            cell.set_style(style);
        }
    }
}
