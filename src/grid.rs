//! Grid layout engine.
//!
//! Computes cell dimensions for NxN grid layouts. Handles odd terminal
//! dimensions by assigning remainder pixels to the last row/column.

/// Supported grid layouts, cycling 2x2 -> 3x3 -> 4x4 -> 2x2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLayout {
    Grid2x2,
    Grid3x3,
    Grid4x4,
}

impl GridLayout {
    /// Number of columns (and rows) in this layout.
    pub fn size(self) -> usize {
        match self {
            GridLayout::Grid2x2 => 2,
            GridLayout::Grid3x3 => 3,
            GridLayout::Grid4x4 => 4,
        }
    }

    /// Total number of cells in this layout.
    pub fn cell_count(self) -> usize {
        let s = self.size();
        s * s
    }

    /// Cycle to the next layout: 2x2 -> 3x3 -> 4x4 -> 2x2.
    pub fn next(self) -> GridLayout {
        match self {
            GridLayout::Grid2x2 => GridLayout::Grid3x3,
            GridLayout::Grid3x3 => GridLayout::Grid4x4,
            GridLayout::Grid4x4 => GridLayout::Grid2x2,
        }
    }
}

/// A rectangle representing a single cell's position and size in terminal coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl CellRect {
    /// The inner area available for content (excluding 1-char border on each side).
    /// Returns None if the cell is too small to have any inner area.
    pub fn inner(self) -> Option<CellRect> {
        if self.width < 3 || self.height < 3 {
            return None;
        }
        Some(CellRect {
            x: self.x + 1,
            y: self.y + 1,
            width: self.width - 2,
            height: self.height - 2,
        })
    }
}

/// Compute the rectangles for all cells in the given layout.
///
/// `total_width` and `total_height` are the terminal dimensions in characters.
/// Remainder from integer division is added to the last row/column.
///
/// Returns a Vec of `CellRect` in row-major order (row 0 col 0, row 0 col 1, ...).
pub fn compute_cells(layout: GridLayout, total_width: u16, total_height: u16) -> Vec<CellRect> {
    let n = layout.size() as u16;
    if n == 0 || total_width == 0 || total_height == 0 {
        return Vec::new();
    }

    let base_w = total_width / n;
    let rem_w = total_width % n;
    let base_h = total_height / n;
    let rem_h = total_height % n;

    // Pre-compute column x-offsets and widths.
    let mut col_x = Vec::with_capacity(n as usize);
    let mut col_w = Vec::with_capacity(n as usize);
    let mut x = 0u16;
    for c in 0..n {
        let w = if c == n - 1 { base_w + rem_w } else { base_w };
        col_x.push(x);
        col_w.push(w);
        x += w;
    }

    // Pre-compute row y-offsets and heights.
    let mut row_y = Vec::with_capacity(n as usize);
    let mut row_h = Vec::with_capacity(n as usize);
    let mut y = 0u16;
    for r in 0..n {
        let h = if r == n - 1 { base_h + rem_h } else { base_h };
        row_y.push(y);
        row_h.push(h);
        y += h;
    }

    let mut cells = Vec::with_capacity((n * n) as usize);
    for r in 0..n as usize {
        for c in 0..n as usize {
            cells.push(CellRect {
                x: col_x[c],
                y: row_y[r],
                width: col_w[c],
                height: row_h[r],
            });
        }
    }

    cells
}

/// Convert a (row, col) pair to a linear index in the cell list.
pub fn rc_to_index(layout: GridLayout, row: usize, col: usize) -> usize {
    row * layout.size() + col
}

/// Convert a linear index to a (row, col) pair.
pub fn index_to_rc(layout: GridLayout, index: usize) -> (usize, usize) {
    let n = layout.size();
    (index / n, index % n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2x2_even() {
        let cells = compute_cells(GridLayout::Grid2x2, 80, 24);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], CellRect { x: 0, y: 0, width: 40, height: 12 });
        assert_eq!(cells[1], CellRect { x: 40, y: 0, width: 40, height: 12 });
        assert_eq!(cells[2], CellRect { x: 0, y: 12, width: 40, height: 12 });
        assert_eq!(cells[3], CellRect { x: 40, y: 12, width: 40, height: 12 });
    }

    #[test]
    fn test_2x2_odd() {
        let cells = compute_cells(GridLayout::Grid2x2, 81, 25);
        assert_eq!(cells.len(), 4);
        // Last column gets remainder width (40+1=41), last row gets remainder height (12+1=13)
        assert_eq!(cells[0], CellRect { x: 0, y: 0, width: 40, height: 12 });
        assert_eq!(cells[1], CellRect { x: 40, y: 0, width: 41, height: 12 });
        assert_eq!(cells[2], CellRect { x: 0, y: 12, width: 40, height: 13 });
        assert_eq!(cells[3], CellRect { x: 40, y: 12, width: 41, height: 13 });
    }

    #[test]
    fn test_3x3_layout() {
        let cells = compute_cells(GridLayout::Grid3x3, 90, 30);
        assert_eq!(cells.len(), 9);
        assert_eq!(cells[0].width, 30);
        assert_eq!(cells[0].height, 10);
    }

    #[test]
    fn test_4x4_layout() {
        let cells = compute_cells(GridLayout::Grid4x4, 80, 24);
        assert_eq!(cells.len(), 16);
        assert_eq!(cells[0].width, 20);
        assert_eq!(cells[0].height, 6);
    }

    #[test]
    fn test_layout_cycling() {
        assert_eq!(GridLayout::Grid2x2.next(), GridLayout::Grid3x3);
        assert_eq!(GridLayout::Grid3x3.next(), GridLayout::Grid4x4);
        assert_eq!(GridLayout::Grid4x4.next(), GridLayout::Grid2x2);
    }

    #[test]
    fn test_tiny_terminal() {
        let cells = compute_cells(GridLayout::Grid2x2, 3, 3);
        assert_eq!(cells.len(), 4);
        // 3/2 = 1 remainder 1; last col/row gets 2
        assert_eq!(cells[0], CellRect { x: 0, y: 0, width: 1, height: 1 });
        assert_eq!(cells[3], CellRect { x: 1, y: 1, width: 2, height: 2 });
    }

    #[test]
    fn test_rc_conversions() {
        assert_eq!(rc_to_index(GridLayout::Grid3x3, 1, 2), 5);
        assert_eq!(index_to_rc(GridLayout::Grid3x3, 5), (1, 2));
    }

    #[test]
    fn test_cell_inner() {
        let cell = CellRect { x: 10, y: 5, width: 20, height: 10 };
        let inner = cell.inner().unwrap();
        assert_eq!(inner, CellRect { x: 11, y: 6, width: 18, height: 8 });
    }

    #[test]
    fn test_cell_inner_too_small() {
        let cell = CellRect { x: 0, y: 0, width: 2, height: 2 };
        assert!(cell.inner().is_none());
    }
}
