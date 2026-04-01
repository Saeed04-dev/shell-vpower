//! Individual terminal cell state.
//!
//! Each cell in the grid maintains its own screen buffer and cursor position,
//! populated by a VTE parser processing PTY output.

use vte::{Params, Perform, Parser};

/// Maximum number of lines retained in the scrollback buffer per cell.
const MAX_SCROLLBACK: usize = 1000;

/// Represents the state of a single terminal cell in the grid.
pub struct TerminalCell {
    /// Character grid: rows of columns. Each entry is a character.
    pub lines: Vec<Vec<char>>,
    /// Current cursor row (0-indexed, relative to the visible area).
    pub cursor_row: usize,
    /// Current cursor column (0-indexed).
    pub cursor_col: usize,
    /// Width of this cell in characters.
    pub width: usize,
    /// Height of this cell in characters.
    pub height: usize,
    /// VTE parser for processing escape sequences.
    parser: Parser,
}

impl TerminalCell {
    /// Create a new terminal cell with the given dimensions.
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let lines = vec![vec![' '; w]; h];
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            width: w,
            height: h,
            parser: Parser::new(),
        }
    }

    /// Resize this cell's buffer to new dimensions.
    /// Existing content is preserved where possible.
    pub fn resize(&mut self, new_width: usize, new_height: usize) {
        let w = new_width.max(1);
        let h = new_height.max(1);

        // Resize existing rows
        for line in &mut self.lines {
            line.resize(w, ' ');
        }

        // Add or remove rows
        while self.lines.len() < h {
            self.lines.push(vec![' '; w]);
        }
        // Keep scrollback but trim to MAX_SCROLLBACK
        if self.lines.len() > MAX_SCROLLBACK {
            let drain = self.lines.len() - MAX_SCROLLBACK;
            self.lines.drain(0..drain);
        }

        self.width = w;
        self.height = h;
        // Clamp cursor
        self.cursor_row = self.cursor_row.min(h.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(w.saturating_sub(1));
    }

    /// Feed raw bytes from PTY output into the VTE parser.
    /// This processes escape sequences and updates the cell buffer.
    pub fn feed(&mut self, data: &[u8]) {
        // We need to collect actions first because the parser borrows self
        // through the Perform trait. We use an intermediate performer.
        let mut performer = CellPerformer {
            lines: &mut self.lines,
            cursor_row: &mut self.cursor_row,
            cursor_col: &mut self.cursor_col,
            width: self.width,
            height: self.height,
        };
        for byte in data {
            self.parser.advance(&mut performer, *byte);
        }
    }

    /// Get the visible lines for rendering. Returns up to `height` lines
    /// ending at the cursor position or the bottom of the buffer.
    pub fn visible_lines(&self) -> &[Vec<char>] {
        let total = self.lines.len();
        if total <= self.height {
            &self.lines
        } else {
            // Show the last `height` lines
            &self.lines[total - self.height..]
        }
    }
}

/// Intermediate struct that implements `vte::Perform` to update the cell buffer.
struct CellPerformer<'a> {
    lines: &'a mut Vec<Vec<char>>,
    cursor_row: &'a mut usize,
    cursor_col: &'a mut usize,
    width: usize,
    height: usize,
}

impl<'a> CellPerformer<'a> {
    /// Ensure the cursor row exists in the buffer.
    fn ensure_row(&mut self) {
        while self.lines.len() <= *self.cursor_row {
            self.lines.push(vec![' '; self.width]);
        }
    }

    /// Scroll the visible area up by one line.
    fn scroll_up(&mut self) {
        self.lines.push(vec![' '; self.width]);
        // Don't decrement cursor_row — it stays at the bottom visible line
        // but conceptually the view scrolls up.
        if self.lines.len() > MAX_SCROLLBACK {
            self.lines.remove(0);
            if *self.cursor_row > 0 {
                *self.cursor_row -= 1;
            }
        }
    }

    /// Move cursor to a new line, scrolling if needed.
    fn newline(&mut self) {
        if *self.cursor_row + 1 >= self.height {
            self.scroll_up();
        } else {
            *self.cursor_row += 1;
        }
        self.ensure_row();
    }

    /// Carriage return — move cursor to column 0.
    fn carriage_return(&mut self) {
        *self.cursor_col = 0;
    }
}

impl<'a> Perform for CellPerformer<'a> {
    fn print(&mut self, c: char) {
        self.ensure_row();
        if *self.cursor_col >= self.width {
            // Auto-wrap
            *self.cursor_col = 0;
            self.newline();
        }
        if let Some(row) = self.lines.get_mut(*self.cursor_row) {
            if *self.cursor_col < row.len() {
                row[*self.cursor_col] = c;
            }
        }
        *self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // Newline (LF)
            b'\n' => {
                self.newline();
            }
            // Carriage return
            b'\r' => {
                self.carriage_return();
            }
            // Backspace
            0x08 => {
                if *self.cursor_col > 0 {
                    *self.cursor_col -= 1;
                }
            }
            // Tab
            b'\t' => {
                let next_tab = (*self.cursor_col + 8) & !7;
                *self.cursor_col = next_tab.min(self.width.saturating_sub(1));
            }
            // Bell — ignore
            0x07 => {}
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        // OSC sequences (like setting terminal title) — ignore for now
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        // Extract parameter values for convenience.
        let params_vec: Vec<u16> = params.iter().flat_map(|sub| sub.iter().copied()).collect();
        let p0 = params_vec.first().copied().unwrap_or(1) as usize;

        match action {
            // Cursor Up (CUU)
            'A' => {
                let n = if p0 == 0 { 1 } else { p0 };
                *self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            // Cursor Down (CUD)
            'B' => {
                let n = if p0 == 0 { 1 } else { p0 };
                *self.cursor_row = (*self.cursor_row + n).min(self.height.saturating_sub(1));
            }
            // Cursor Forward (CUF)
            'C' => {
                let n = if p0 == 0 { 1 } else { p0 };
                *self.cursor_col = (*self.cursor_col + n).min(self.width.saturating_sub(1));
            }
            // Cursor Backward (CUB)
            'D' => {
                let n = if p0 == 0 { 1 } else { p0 };
                *self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            // Cursor Position (CUP) / Horizontal and Vertical Position (HVP)
            'H' | 'f' => {
                let row = params_vec.first().copied().unwrap_or(1) as usize;
                let col = params_vec.get(1).copied().unwrap_or(1) as usize;
                // CSI row;col H — 1-indexed
                *self.cursor_row = row.saturating_sub(1).min(self.height.saturating_sub(1));
                *self.cursor_col = col.saturating_sub(1).min(self.width.saturating_sub(1));
            }
            // Erase in Display (ED)
            'J' => {
                let mode = params_vec.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        // Clear from cursor to end of screen
                        self.ensure_row();
                        if let Some(row) = self.lines.get_mut(*self.cursor_row) {
                            for c in row.iter_mut().skip(*self.cursor_col) {
                                *c = ' ';
                            }
                        }
                        for r in (*self.cursor_row + 1)..self.lines.len() {
                            for c in self.lines[r].iter_mut() {
                                *c = ' ';
                            }
                        }
                    }
                    1 => {
                        // Clear from start to cursor
                        for r in 0..*self.cursor_row {
                            if r < self.lines.len() {
                                for c in self.lines[r].iter_mut() {
                                    *c = ' ';
                                }
                            }
                        }
                        self.ensure_row();
                        if let Some(row) = self.lines.get_mut(*self.cursor_row) {
                            for c in row.iter_mut().take(*self.cursor_col + 1) {
                                *c = ' ';
                            }
                        }
                    }
                    2 | 3 => {
                        // Clear entire screen
                        for row in self.lines.iter_mut() {
                            for c in row.iter_mut() {
                                *c = ' ';
                            }
                        }
                        *self.cursor_row = 0;
                        *self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            // Erase in Line (EL)
            'K' => {
                let mode = params_vec.first().copied().unwrap_or(0);
                self.ensure_row();
                if let Some(row) = self.lines.get_mut(*self.cursor_row) {
                    match mode {
                        0 => {
                            // Clear from cursor to end of line
                            for c in row.iter_mut().skip(*self.cursor_col) {
                                *c = ' ';
                            }
                        }
                        1 => {
                            // Clear from start to cursor
                            for c in row.iter_mut().take(*self.cursor_col + 1) {
                                *c = ' ';
                            }
                        }
                        2 => {
                            // Clear entire line
                            for c in row.iter_mut() {
                                *c = ' ';
                            }
                        }
                        _ => {}
                    }
                }
            }
            // SGR (Select Graphic Rendition) — colors/attributes, ignore for MVP
            'm' => {}
            // Save/restore cursor, scrolling regions, etc. — ignore for MVP
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}
