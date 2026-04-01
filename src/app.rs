//! Application state and main event loop.
//!
//! Ties together the grid engine, PTY manager, input handler, and renderer.
//! Uses `tokio::select!` to multiplex between user input events and PTY output.

use crate::grid::{self, CellRect, GridLayout};
use crate::input::{self, Direction, InputAction};
use crate::pty::{PtyManager, PtyOutput};
use crate::terminal_cell::TerminalCell;
use crate::ui::{GridWidget, StatusBar};

use anyhow::Result;
use crossterm::event::{Event, EventStream};
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
    // Fallback for very small cells
    (1, 1)
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

        Self {
            layout,
            focus: (0, 0),
            cells: Vec::new(),
            cell_rects,
            pty_manager: PtyManager::new(pty_output_tx),
            term_size: (term_width, grid_height),
        }
    }

    /// Run the application event loop. This is the main entry point.
    pub async fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let size = terminal.size()?;
        let (pty_tx, mut pty_rx) = mpsc::unbounded_channel::<PtyOutput>();

        let mut app = App::new(size.width, size.height, pty_tx);
        app.init_cells()?;

        // Initial render
        app.draw(terminal)?;

        // Event stream for crossterm events
        let mut event_stream = EventStream::new();

        loop {
            tokio::select! {
                // Handle crossterm events (keyboard input, resize)
                event = event_stream.next() => {
                    match event {
                        Some(Ok(Event::Key(key_event))) => {
                            match input::handle_key_event(key_event) {
                                InputAction::Quit => break,
                                InputAction::CycleLayout => {
                                    app.cycle_layout()?;
                                    app.draw(terminal)?;
                                }
                                InputAction::MoveFocus(dir) => {
                                    app.move_focus(dir);
                                    app.draw(terminal)?;
                                }
                                InputAction::PtyInput(data) => {
                                    let idx = grid::rc_to_index(app.layout, app.focus.0, app.focus.1);
                                    app.pty_manager.write_to(idx, &data)?;
                                }
                                InputAction::None => {}
                            }
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
                // Handle PTY output
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

        // Create terminal cell buffers
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
                if row == 0 {
                    (n - 1, col)
                } else {
                    (row - 1, col)
                }
            }
            Direction::Down => {
                if row + 1 >= n {
                    (0, col)
                } else {
                    (row + 1, col)
                }
            }
            Direction::Left => {
                if col == 0 {
                    (row, n - 1)
                } else {
                    (row, col - 1)
                }
            }
            Direction::Right => {
                if col + 1 >= n {
                    (row, 0)
                } else {
                    (row, col + 1)
                }
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

        // Ensure enough PTYs exist
        let count = self.layout.cell_count();
        let rects = &self.cell_rects;
        self.pty_manager
            .ensure_count(count, |idx| cell_inner_size(rects, idx))?;

        // Ensure enough cell buffers exist
        while self.cells.len() < count {
            let (cols, rows) = cell_inner_size(&self.cell_rects, self.cells.len());
            self.cells
                .push(TerminalCell::new(cols as usize, rows as usize));
        }

        // Resize existing cell buffers and PTYs
        for i in 0..count {
            let (cols, rows) = cell_inner_size(&self.cell_rects, i);
            if i < self.cells.len() {
                self.cells[i].resize(cols as usize, rows as usize);
            }
            let _ = self.pty_manager.resize(i, cols, rows);
        }

        // Clamp focus to valid range
        let n = self.layout.size();
        self.focus.0 = self.focus.0.min(n - 1);
        self.focus.1 = self.focus.1.min(n - 1);

        Ok(())
    }

    /// Draw the current state to the terminal.
    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let layout = self.layout;
        let focus_index = grid::rc_to_index(layout, self.focus.0, self.focus.1);
        let cell_rects = &self.cell_rects;
        let cells = &self.cells;
        let focus = self.focus;

        terminal.draw(|frame| {
            let area = frame.area();

            // Main grid area (everything except status bar)
            let grid_widget = GridWidget {
                layout,
                cell_rects,
                cells,
                focus_index,
            };

            frame.render_widget(grid_widget, area);

            // Status bar at the bottom
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
