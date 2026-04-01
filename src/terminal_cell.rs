//! Individual terminal cell state.
//!
//! Each cell in the grid maintains its own screen buffer and cursor position,
//! populated by a VTE parser processing PTY output. Supports ANSI colors,
//! alternate screen buffer, scroll regions, and DEC private modes.

use vte::{Params, Perform, Parser};

const MAX_SCROLLBACK: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}
impl Default for CellColor {
    fn default() -> Self { CellColor::Default }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CellStyle {
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct StyledChar {
    pub ch: char,
    pub style: CellStyle,
}
impl Default for StyledChar {
    fn default() -> Self { Self { ch: ' ', style: CellStyle::default() } }
}

fn blank_line(w: usize) -> Vec<StyledChar> {
    vec![StyledChar::default(); w]
}

fn blank_screen(w: usize, h: usize) -> Vec<Vec<StyledChar>> {
    vec![blank_line(w); h]
}

pub struct TerminalCell {
    // Active buffer (always the one we read/write)
    lines: Vec<Vec<StyledChar>>,
    cursor_row: usize,
    cursor_col: usize,
    screen_top: usize,
    // Alternate buffer storage (swapped with active on enter/leave)
    alt_lines: Vec<Vec<StyledChar>>,
    alt_cursor_row: usize,
    alt_cursor_col: usize,
    alt_screen_top: usize,
    use_alternate: bool,
    pub width: usize,
    pub height: usize,
    pen: CellStyle,
    saved_cursor: Option<(usize, usize)>,
    scroll_top: usize,
    scroll_bottom: usize,
    auto_wrap: bool,
    pub cursor_visible: bool,
    parser: Parser,
}

impl TerminalCell {
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            lines: blank_screen(w, h),
            cursor_row: 0,
            cursor_col: 0,
            screen_top: 0,
            alt_lines: blank_screen(w, h),
            alt_cursor_row: 0,
            alt_cursor_col: 0,
            alt_screen_top: 0,
            use_alternate: false,
            width: w,
            height: h,
            pen: CellStyle::default(),
            saved_cursor: None,
            scroll_top: 0,
            scroll_bottom: h.saturating_sub(1),
            auto_wrap: true,
            cursor_visible: true,
            parser: Parser::new(),
        }
    }

    pub fn resize(&mut self, new_width: usize, new_height: usize) {
        let w = new_width.max(1);
        let h = new_height.max(1);
        for lines in [&mut self.lines, &mut self.alt_lines] {
            for line in lines.iter_mut() {
                line.resize(w, StyledChar::default());
            }
            while lines.len() < h {
                lines.push(blank_line(w));
            }
            if lines.len() > MAX_SCROLLBACK {
                lines.drain(0..lines.len() - MAX_SCROLLBACK);
            }
        }
        self.screen_top = self.lines.len().saturating_sub(h);
        self.alt_screen_top = self.alt_lines.len().saturating_sub(h);
        self.cursor_row = self.cursor_row.min(h.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(w.saturating_sub(1));
        self.alt_cursor_row = self.alt_cursor_row.min(h.saturating_sub(1));
        self.alt_cursor_col = self.alt_cursor_col.min(w.saturating_sub(1));
        self.width = w;
        self.height = h;
        self.scroll_top = 0;
        self.scroll_bottom = h.saturating_sub(1);
    }

    pub fn feed(&mut self, data: &[u8]) {
        let mut performer = CellPerformer {
            lines: &mut self.lines,
            cursor_row: &mut self.cursor_row,
            cursor_col: &mut self.cursor_col,
            screen_top: &mut self.screen_top,
            alt_lines: &mut self.alt_lines,
            alt_cursor_row: &mut self.alt_cursor_row,
            alt_cursor_col: &mut self.alt_cursor_col,
            alt_screen_top: &mut self.alt_screen_top,
            use_alternate: &mut self.use_alternate,
            pen: &mut self.pen,
            saved_cursor: &mut self.saved_cursor,
            scroll_top: &mut self.scroll_top,
            scroll_bottom: &mut self.scroll_bottom,
            auto_wrap: &mut self.auto_wrap,
            cursor_visible: &mut self.cursor_visible,
            width: self.width,
            height: self.height,
        };
        for byte in data {
            self.parser.advance(&mut performer, *byte);
        }
    }

    pub fn visible_lines(&self) -> &[Vec<StyledChar>] {
        let end = (self.screen_top + self.height).min(self.lines.len());
        if self.screen_top < self.lines.len() {
            &self.lines[self.screen_top..end]
        } else {
            &[]
        }
    }
}

struct CellPerformer<'a> {
    lines: &'a mut Vec<Vec<StyledChar>>,
    cursor_row: &'a mut usize,
    cursor_col: &'a mut usize,
    screen_top: &'a mut usize,
    alt_lines: &'a mut Vec<Vec<StyledChar>>,
    alt_cursor_row: &'a mut usize,
    alt_cursor_col: &'a mut usize,
    alt_screen_top: &'a mut usize,
    use_alternate: &'a mut bool,
    pen: &'a mut CellStyle,
    saved_cursor: &'a mut Option<(usize, usize)>,
    scroll_top: &'a mut usize,
    scroll_bottom: &'a mut usize,
    auto_wrap: &'a mut bool,
    cursor_visible: &'a mut bool,
    width: usize,
    height: usize,
}

impl<'a> CellPerformer<'a> {
    fn abs_row(&self) -> usize {
        *self.screen_top + *self.cursor_row
    }

    fn ensure_row(&mut self) {
        let abs = self.abs_row();
        while self.lines.len() <= abs {
            self.lines.push(blank_line(self.width));
        }
    }

    fn scroll_region_up(&mut self) {
        let st = *self.screen_top;
        let s_top = *self.scroll_top;
        let s_bot = *self.scroll_bottom;
        let abs_top = st + s_top;
        let abs_bot = st + s_bot;
        let w = self.width;

        if abs_top < self.lines.len() && abs_bot < self.lines.len() && abs_top <= abs_bot {
            self.lines.remove(abs_top);
            let insert_at = abs_bot.min(self.lines.len());
            self.lines.insert(insert_at, blank_line(w));
        } else if s_top == 0 && s_bot >= self.height.saturating_sub(1) {
            self.lines.push(blank_line(w));
            *self.screen_top += 1;
            if self.lines.len() > MAX_SCROLLBACK {
                self.lines.remove(0);
                *self.screen_top = self.screen_top.saturating_sub(1);
            }
        }
    }

    fn scroll_region_down(&mut self) {
        let st = *self.screen_top;
        let abs_top = st + *self.scroll_top;
        let abs_bot = st + *self.scroll_bottom;
        let w = self.width;

        if abs_top < self.lines.len() && abs_bot < self.lines.len() && abs_top <= abs_bot {
            if abs_bot < self.lines.len() {
                self.lines.remove(abs_bot);
            }
            self.lines.insert(abs_top, blank_line(w));
        }
    }

    fn newline(&mut self) {
        if *self.cursor_row >= *self.scroll_bottom {
            self.scroll_region_up();
        } else {
            *self.cursor_row += 1;
        }
        self.ensure_row();
    }

    fn enter_alternate(&mut self) {
        if !*self.use_alternate {
            *self.use_alternate = true;
            std::mem::swap(self.lines, self.alt_lines);
            std::mem::swap(self.cursor_row, self.alt_cursor_row);
            std::mem::swap(self.cursor_col, self.alt_cursor_col);
            std::mem::swap(self.screen_top, self.alt_screen_top);
            // Clear the now-active alternate screen
            *self.lines = blank_screen(self.width, self.height);
            *self.cursor_row = 0;
            *self.cursor_col = 0;
            *self.screen_top = 0;
        }
    }

    fn leave_alternate(&mut self) {
        if *self.use_alternate {
            *self.use_alternate = false;
            std::mem::swap(self.lines, self.alt_lines);
            std::mem::swap(self.cursor_row, self.alt_cursor_row);
            std::mem::swap(self.cursor_col, self.alt_cursor_col);
            std::mem::swap(self.screen_top, self.alt_screen_top);
        }
    }

    fn apply_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => *self.pen = CellStyle::default(),
                1 => self.pen.bold = true,
                2 => self.pen.dim = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = true,
                7 => self.pen.reverse = true,
                21 | 22 => { self.pen.bold = false; self.pen.dim = false; }
                23 => self.pen.italic = false,
                24 => self.pen.underline = false,
                27 => self.pen.reverse = false,
                30..=37 => self.pen.fg = CellColor::Indexed(params[i] as u8 - 30),
                38 => {
                    if i + 1 < params.len() {
                        match params[i + 1] {
                            5 if i + 2 < params.len() => {
                                self.pen.fg = CellColor::Indexed(params[i + 2] as u8);
                                i += 2;
                            }
                            2 if i + 4 < params.len() => {
                                self.pen.fg = CellColor::Rgb(params[i + 2] as u8, params[i + 3] as u8, params[i + 4] as u8);
                                i += 4;
                            }
                            _ => { i += 1; }
                        }
                    }
                }
                39 => self.pen.fg = CellColor::Default,
                40..=47 => self.pen.bg = CellColor::Indexed(params[i] as u8 - 40),
                48 => {
                    if i + 1 < params.len() {
                        match params[i + 1] {
                            5 if i + 2 < params.len() => {
                                self.pen.bg = CellColor::Indexed(params[i + 2] as u8);
                                i += 2;
                            }
                            2 if i + 4 < params.len() => {
                                self.pen.bg = CellColor::Rgb(params[i + 2] as u8, params[i + 3] as u8, params[i + 4] as u8);
                                i += 4;
                            }
                            _ => { i += 1; }
                        }
                    }
                }
                49 => self.pen.bg = CellColor::Default,
                90..=97 => self.pen.fg = CellColor::Indexed(params[i] as u8 - 90 + 8),
                100..=107 => self.pen.bg = CellColor::Indexed(params[i] as u8 - 100 + 8),
                _ => {}
            }
            i += 1;
        }
    }
}

impl<'a> Perform for CellPerformer<'a> {
    fn print(&mut self, c: char) {
        self.ensure_row();
        if *self.cursor_col >= self.width {
            if *self.auto_wrap {
                *self.cursor_col = 0;
                self.newline();
            } else {
                *self.cursor_col = self.width.saturating_sub(1);
            }
        }
        let abs = self.abs_row();
        let pen = *self.pen;
        if let Some(row) = self.lines.get_mut(abs) {
            if *self.cursor_col < row.len() {
                row[*self.cursor_col] = StyledChar { ch: c, style: pen };
            }
        }
        *self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.newline(),
            b'\r' => *self.cursor_col = 0,
            0x08 => {
                if *self.cursor_col > 0 { *self.cursor_col -= 1; }
            }
            b'\t' => {
                let next = (*self.cursor_col + 8) & !7;
                *self.cursor_col = next.min(self.width.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn hook(&mut self, _: &Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {}

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let pv: Vec<u16> = params.iter().flat_map(|s| s.iter().copied()).collect();
        let p0 = pv.first().copied().unwrap_or(0) as usize;
        let n = if p0 == 0 { 1 } else { p0 };
        let w = self.width;
        let h = self.height;

        // DEC private modes (CSI ? ... h/l)
        if intermediates.contains(&b'?') {
            match action {
                'h' => {
                    for &p in &pv {
                        match p {
                            7 => *self.auto_wrap = true,
                            25 => *self.cursor_visible = true,
                            1049 => {
                                *self.saved_cursor = Some((*self.cursor_row, *self.cursor_col));
                                self.enter_alternate();
                            }
                            1047 | 47 => self.enter_alternate(),
                            _ => {}
                        }
                    }
                }
                'l' => {
                    for &p in &pv {
                        match p {
                            7 => *self.auto_wrap = false,
                            25 => *self.cursor_visible = false,
                            1049 => {
                                self.leave_alternate();
                                if let Some((r, c)) = *self.saved_cursor {
                                    *self.cursor_row = r.min(h.saturating_sub(1));
                                    *self.cursor_col = c.min(w.saturating_sub(1));
                                }
                            }
                            1047 | 47 => self.leave_alternate(),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        match action {
            'A' => *self.cursor_row = self.cursor_row.saturating_sub(n),
            'B' => *self.cursor_row = (*self.cursor_row + n).min(h.saturating_sub(1)),
            'C' => *self.cursor_col = (*self.cursor_col + n).min(w.saturating_sub(1)),
            'D' => *self.cursor_col = self.cursor_col.saturating_sub(n),
            'E' => {
                *self.cursor_row = (*self.cursor_row + n).min(h.saturating_sub(1));
                *self.cursor_col = 0;
            }
            'F' => {
                *self.cursor_row = self.cursor_row.saturating_sub(n);
                *self.cursor_col = 0;
            }
            'G' => *self.cursor_col = n.saturating_sub(1).min(w.saturating_sub(1)),
            'H' | 'f' => {
                let row = pv.first().copied().unwrap_or(1).max(1) as usize;
                let col = pv.get(1).copied().unwrap_or(1).max(1) as usize;
                *self.cursor_row = row.saturating_sub(1).min(h.saturating_sub(1));
                *self.cursor_col = col.saturating_sub(1).min(w.saturating_sub(1));
                self.ensure_row();
            }
            'J' => {
                let mode = pv.first().copied().unwrap_or(0);
                let abs = self.abs_row();
                let st = *self.screen_top;
                let col = *self.cursor_col;
                self.ensure_row();
                match mode {
                    0 => {
                        if let Some(row) = self.lines.get_mut(abs) {
                            for c in row.iter_mut().skip(col) { *c = StyledChar::default(); }
                        }
                        let end = (st + h).min(self.lines.len());
                        for r in (abs + 1)..end {
                            for c in self.lines[r].iter_mut() { *c = StyledChar::default(); }
                        }
                    }
                    1 => {
                        for r in st..abs {
                            if r < self.lines.len() {
                                for c in self.lines[r].iter_mut() { *c = StyledChar::default(); }
                            }
                        }
                        if let Some(row) = self.lines.get_mut(abs) {
                            for c in row.iter_mut().take(col + 1) { *c = StyledChar::default(); }
                        }
                    }
                    2 | 3 => {
                        let end = (st + h).min(self.lines.len());
                        for r in st..end {
                            for c in self.lines[r].iter_mut() { *c = StyledChar::default(); }
                        }
                        *self.cursor_row = 0;
                        *self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                self.ensure_row();
                let abs = self.abs_row();
                let col = *self.cursor_col;
                let mode = pv.first().copied().unwrap_or(0);
                if let Some(row) = self.lines.get_mut(abs) {
                    match mode {
                        0 => { for c in row.iter_mut().skip(col) { *c = StyledChar::default(); } }
                        1 => { for c in row.iter_mut().take(col + 1) { *c = StyledChar::default(); } }
                        2 => { for c in row.iter_mut() { *c = StyledChar::default(); } }
                        _ => {}
                    }
                }
            }
            'L' => {
                let abs = self.abs_row();
                let abs_bot = *self.screen_top + *self.scroll_bottom;
                for _ in 0..n {
                    if abs <= abs_bot && abs < self.lines.len() {
                        self.lines.insert(abs, blank_line(w));
                        let rm = abs_bot + 1;
                        if rm < self.lines.len() { self.lines.remove(rm); }
                    }
                }
            }
            'M' => {
                let abs = self.abs_row();
                let abs_bot = *self.screen_top + *self.scroll_bottom;
                for _ in 0..n {
                    if abs <= abs_bot && abs < self.lines.len() {
                        self.lines.remove(abs);
                        let ins = abs_bot.min(self.lines.len());
                        self.lines.insert(ins, blank_line(w));
                    }
                }
            }
            'P' => {
                self.ensure_row();
                let abs = self.abs_row();
                let col = *self.cursor_col;
                if let Some(row) = self.lines.get_mut(abs) {
                    if col < row.len() {
                        let rm = n.min(row.len() - col);
                        row.drain(col..col + rm);
                        row.resize(w, StyledChar::default());
                    }
                }
            }
            'S' => { for _ in 0..n { self.scroll_region_up(); } }
            'T' => { for _ in 0..n { self.scroll_region_down(); } }
            'X' => {
                self.ensure_row();
                let abs = self.abs_row();
                let col = *self.cursor_col;
                if let Some(row) = self.lines.get_mut(abs) {
                    for i in 0..n {
                        if col + i < row.len() { row[col + i] = StyledChar::default(); }
                    }
                }
            }
            '@' => {
                self.ensure_row();
                let abs = self.abs_row();
                let col = *self.cursor_col;
                if let Some(row) = self.lines.get_mut(abs) {
                    if col < row.len() {
                        for _ in 0..n { row.insert(col, StyledChar::default()); }
                        row.truncate(w);
                    }
                }
            }
            'd' => {
                *self.cursor_row = n.saturating_sub(1).min(h.saturating_sub(1));
                self.ensure_row();
            }
            'm' => {
                if pv.is_empty() { *self.pen = CellStyle::default(); }
                else { self.apply_sgr(&pv); }
            }
            'r' => {
                let top = pv.first().copied().unwrap_or(1).max(1) as usize;
                let bot = pv.get(1).copied().unwrap_or(h as u16).max(1) as usize;
                *self.scroll_top = top.saturating_sub(1).min(h.saturating_sub(1));
                *self.scroll_bottom = bot.saturating_sub(1).min(h.saturating_sub(1));
                if *self.scroll_top > *self.scroll_bottom {
                    *self.scroll_top = 0;
                    *self.scroll_bottom = h.saturating_sub(1);
                }
                *self.cursor_row = 0;
                *self.cursor_col = 0;
            }
            's' => *self.saved_cursor = Some((*self.cursor_row, *self.cursor_col)),
            'u' => {
                if let Some((r, c)) = *self.saved_cursor {
                    *self.cursor_row = r.min(h.saturating_sub(1));
                    *self.cursor_col = c.min(w.saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        let h = self.height;
        let w = self.width;
        match byte {
            b'7' => *self.saved_cursor = Some((*self.cursor_row, *self.cursor_col)),
            b'8' => {
                if let Some((r, c)) = *self.saved_cursor {
                    *self.cursor_row = r.min(h.saturating_sub(1));
                    *self.cursor_col = c.min(w.saturating_sub(1));
                }
            }
            b'M' => {
                if *self.cursor_row == *self.scroll_top {
                    self.scroll_region_down();
                } else {
                    *self.cursor_row = self.cursor_row.saturating_sub(1);
                }
            }
            b'E' => { self.newline(); *self.cursor_col = 0; }
            b'D' => self.newline(),
            b'c' => {
                *self.pen = CellStyle::default();
                *self.scroll_top = 0;
                *self.scroll_bottom = h.saturating_sub(1);
                *self.auto_wrap = true;
                *self.cursor_visible = true;
                *self.saved_cursor = None;
            }
            _ => {}
        }
    }
}
