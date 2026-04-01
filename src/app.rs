//! Application state and main event loop.
//!
//! Ties together the grid engine, PTY manager, input handler, and renderer.
//! Uses `tokio::select!` to multiplex between user input events and PTY output.

use crate::grid::{self, CellRect, GridLayout};
use crate::input::{self, Direction, InputAction};
use crate::pty::{PtyManager, PtyOutput};
use crate::terminal_cell::TerminalCell;
use crate::ui::{GridWidget, Selection, StatusBar};

use anyhow::Result;
use arboard::Clipboard;
use crossterm::event::{Event, EventStream, MouseButton, MouseEventKind};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use std::io;
use tokio::sync::mpsc;

/// Height of the status bar in rows.
const STATUS_BAR_HEIGHT: u16 = 1;

/// Get the inner (content area) size for a cell by index from the cell_rects slice.
fn cell_inner_size(cell_rects: &[CellRect], index: usize) -> (u16, u16) {
    if let Some(rect) = cell_rects.get(index) {
        if let Some(inner) = rect.inner() {
            return (inner.width, inner.height);
        }
    }
    (1, 1)
}

/// Mouse selection state.
struct SelectionState {
    /// Which cell the selection is in.
    cell_index: usize,
    /// Start position (row, col) within visible lines.
    start: (usize, usize),
    /// End position (row, col) within visible lines.
    end: (usize, usize),
    /// Whether the user is actively dragging.
    dragging: bool,
}

/// Application state.
pub struct App {
    /// Current grid layout.
    layout: GridLayout,
    /// Currently focused cell as (row, col).
    focus: (usize, usize),
    /// Terminal cell buffers — one per PTY.
    cells: Vec<TerminalCell>,
    /// Computed cell rectangles for the current layout.
    cell_rects: Vec<CellRect>,
    /// PTY manager.
    pty_manager: PtyManager,
    /// Terminal size (width, height) excluding the status bar.
    term_size: (u16, u16),
    /// Current mouse selection.
    selection: Option<SelectionState>,
    /// System clipboard.
    clipboard: Option<Clipboard>,
}

impl App {
    /// Create a new app with the initial 2x2 layout.
    fn new(
        term_width: u16,
        term_height: u16,
        pty_output_tx: mpsc::UnboundedSender<PtyOutput>,
    ) -> Self {
        let layout = GridLayout::Grid2x2;
        let grid_height = term_height.saturating_sub(STATUS_BAR_HEIGHT);
        let cell_rects = grid::compute_cells(layout, term_width, grid_height);
        let clipboard = Clipboard::new().ok();

        Self {
            layout,
            focus: (0, 0),
            cells: Vec::new(),
            cell_rects,
            pty_manager: PtyManager::new(pty_output_tx),
            term_size: (term_width, grid_height),
            selection: None,
            clipboard,
        }
    }

    /// Run the application event loop.
    pub async fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let size = terminal.size()?;
        let (pty_tx, mut pty_rx) = mpsc::unbounded_channel::<PtyOutput>();

        let mut app = App::new(size.width, size.height, pty_tx);
        app.init_cells()?;
        app.draw(terminal)?;

        let mut event_stream = EventStream::new();

        loop {
            tokio::select! {
                event = event_stream.next() => {
                    match event {
                        Some(Ok(Event::Key(key_event))) => {
                            match input::handle_key_event(key_event) {
                                InputAction::Quit => break,
                                InputAction::CycleLayout => {
                                    app.selection = None;
                                    app.cycle_layout()?;
                                    app.draw(terminal)?;
                                }
                                InputAction::MoveFocus(dir) => {
                                    app.selection = None;
                                    app.move_focus(dir);
                                    app.draw(terminal)?;
                                }
                                InputAction::Copy => {
                                    app.handle_copy()?;
                                    app.draw(terminal)?;
                                }
                                InputAction::Paste => {
                                    app.handle_paste()?;
                                }
                                InputAction::PtyInput(data) => {
                                    // Any keyboard input clears selection
                                    if app.selection.is_some() {
                                        app.selection = None;
                                        app.draw(terminal)?;
                                    }
                                    let idx = grid::rc_to_index(app.layout, app.focus.0, app.focus.1);
                                    app.pty_manager.write_to(idx, &data)?;
                                }
                                InputAction::None => {}
                            }
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            app.handle_mouse(mouse)?;
                            app.draw(terminal)?;
                        }
                        Some(Ok(Event::Resize(w, h))) => {
                            app.handle_resize(w, h)?;
                            app.draw(terminal)?;
                        }
                        Some(Err(_)) => break,
                        None => break,
                        _ => {}
                    }
                }
                Some(output) = pty_rx.recv() => {
                    if output.cell_index < app.cells.len() {
                        app.cells[output.cell_index].feed(&output.data);
                    }
                    app.draw(terminal)?;
                }
            }
        }

        Ok(())
    }

    /// Initialize terminal cells and spawn PTYs for the current layout.
    fn init_cells(&mut self) -> Result<()> {
        let count = self.layout.cell_count();
        let rects = &self.cell_rects;
        self.pty_manager
            .ensure_count(count, |idx| cell_inner_size(rects, idx))?;

        while self.cells.len() < count {
            let (cols, rows) = cell_inner_size(&self.cell_rects, self.cells.len());
            self.cells
                .push(TerminalCell::new(cols as usize, rows as usize));
        }

        Ok(())
    }

    /// Cycle to the next grid layout.
    fn cycle_layout(&mut self) -> Result<()> {
        self.layout = self.layout.next();
        self.recompute_grid()?;
        Ok(())
    }

    /// Move focus in the given direction, wrapping at grid boundaries.
    fn move_focus(&mut self, direction: Direction) {
        let n = self.layout.size();
        let (row, col) = self.focus;

        self.focus = match direction {
            Direction::Up => {
                if row == 0 { (n - 1, col) } else { (row - 1, col) }
            }
            Direction::Down => {
                if row + 1 >= n { (0, col) } else { (row + 1, col) }
            }
            Direction::Left => {
                if col == 0 { (row, n - 1) } else { (row, col - 1) }
            }
            Direction::Right => {
                if col + 1 >= n { (row, 0) } else { (row, col + 1) }
            }
        };
    }

    /// Handle terminal resize.
    fn handle_resize(&mut self, width: u16, height: u16) -> Result<()> {
        self.term_size = (width, height.saturating_sub(STATUS_BAR_HEIGHT));
        self.recompute_grid()?;
        Ok(())
    }

    /// Recompute grid cells and resize PTYs/buffers accordingly.
    fn recompute_grid(&mut self) -> Result<()> {
        let (w, h) = self.term_size;
        self.cell_rects = grid::compute_cells(self.layout, w, h);

        let count = self.layout.cell_count();
        let rects = &self.cell_rects;
        self.pty_manager
            .ensure_count(count, |idx| cell_inner_size(rects, idx))?;

        while self.cells.len() < count {
            let (cols, rows) = cell_inner_size(&self.cell_rects, self.cells.len());
            self.cells
                .push(TerminalCell::new(cols as usize, rows as usize));
        }

        for i in 0..count {
            let (cols, rows) = cell_inner_size(&self.cell_rects, i);
            if i < self.cells.len() {
                self.cells[i].resize(cols as usize, rows as usize);
            }
            let _ = self.pty_manager.resize(i, cols, rows);
        }

        let n = self.layout.size();
        self.focus.0 = self.focus.0.min(n - 1);
        self.focus.1 = self.focus.1.min(n - 1);

        Ok(())
    }

    /// Handle mouse events (click to focus, drag to select, right-click to paste).
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Result<()> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Find which cell was clicked
                if let Some((cell_idx, inner_row, inner_col)) = self.screen_to_cell(x, y) {
                    // Change focus to clicked cell
                    let n = self.layout.size();
                    self.focus = (cell_idx / n, cell_idx % n);

                    // Start selection
                    self.selection = Some(SelectionState {
                        cell_index: cell_idx,
                        start: (inner_row, inner_col),
                        end: (inner_row, inner_col),
                        dragging: true,
                    });
                } else {
                    self.selection = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let hit = self.screen_to_cell(x, y);
                if let Some(sel) = &mut self.selection {
                    if sel.dragging {
                        if let Some((cell_idx, inner_row, inner_col)) = hit {
                            if cell_idx == sel.cell_index {
                                sel.end = (inner_row, inner_col);
                            }
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(sel) = &mut self.selection {
                    sel.dragging = false;
                    // If start == end, it's a click not a selection
                    if sel.start == sel.end {
                        self.selection = None;
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click to paste
                self.handle_paste()?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Convert screen coordinates to (cell_index, inner_row, inner_col).
    fn screen_to_cell(&self, x: u16, y: u16) -> Option<(usize, usize, usize)> {
        let count = self.layout.cell_count().min(self.cell_rects.len());
        for i in 0..count {
            let rect = self.cell_rects[i];
            if let Some(inner) = rect.inner() {
                if x >= inner.x
                    && x < inner.x + inner.width
                    && y >= inner.y
                    && y < inner.y + inner.height
                {
                    let row = (y - inner.y) as usize;
                    let col = (x - inner.x) as usize;
                    return Some((i, row, col));
                }
            }
        }
        None
    }

    /// Handle Ctrl+C: copy selection or send interrupt.
    fn handle_copy(&mut self) -> Result<()> {
        if let Some(sel) = &self.selection {
            let text = self.get_selected_text(sel.cell_index, sel.start, sel.end);
            if let Some(clipboard) = &mut self.clipboard {
                let _ = clipboard.set_text(&text);
            }
            self.selection = None;
        } else {
            // No selection → send Ctrl+C (interrupt) to PTY
            let idx = grid::rc_to_index(self.layout, self.focus.0, self.focus.1);
            self.pty_manager.write_to(idx, &[3])?;
        }
        Ok(())
    }

    /// Handle Ctrl+V: paste clipboard text to PTY.
    fn handle_paste(&mut self) -> Result<()> {
        if let Some(clipboard) = &mut self.clipboard {
            if let Ok(text) = clipboard.get_text() {
                if !text.is_empty() {
                    let idx = grid::rc_to_index(self.layout, self.focus.0, self.focus.1);
                    self.pty_manager.write_to(idx, text.as_bytes())?;
                }
            }
        }
        Ok(())
    }

    /// Extract selected text from a cell.
    fn get_selected_text(&self, cell_idx: usize, start: (usize, usize), end: (usize, usize)) -> String {
        if cell_idx >= self.cells.len() {
            return String::new();
        }

        let visible = self.cells[cell_idx].visible_lines();

        // Normalize start/end so start is before end
        let (start, end) = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
            (start, end)
        } else {
            (end, start)
        };

        let mut result = String::new();

        for row in start.0..=end.0 {
            if row >= visible.len() {
                break;
            }
            let line = &visible[row];
            let col_start = if row == start.0 { start.1 } else { 0 };
            let col_end = if row == end.0 { end.1 + 1 } else { line.len() };

            for col in col_start..col_end.min(line.len()) {
                result.push(line[col].ch);
            }

            // Trim trailing spaces from each line
            if row < end.0 {
                let trimmed = result.trim_end().len();
                result.truncate(trimmed);
                result.push('\n');
            }
        }

        // Trim trailing spaces from the last line
        let trimmed = result.trim_end().len();
        result.truncate(trimmed);
        result
    }

    /// Draw the current state to the terminal.
    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let layout = self.layout;
        let focus_index = grid::rc_to_index(layout, self.focus.0, self.focus.1);
        let cell_rects = &self.cell_rects;
        let cells = &self.cells;
        let focus = self.focus;

        // Build selection info for rendering
        let selection = self.selection.as_ref().map(|sel| Selection {
            cell_index: sel.cell_index,
            start: sel.start,
            end: sel.end,
        });

        terminal.draw(|frame| {
            let area = frame.area();

            let grid_widget = GridWidget {
                layout,
                cell_rects,
                cells,
                focus_index,
                selection: selection.as_ref(),
            };

            frame.render_widget(grid_widget, area);

            if area.height > 0 {
                let status_area = Rect {
                    x: 0,
                    y: area.height - 1,
                    width: area.width,
                    height: 1,
                };
                let status = StatusBar {
                    layout,
                    focus_row: focus.0,
                    focus_col: focus.1,
                };
                frame.render_widget(status, status_area);
            }
        })?;

        Ok(())
    }
}
