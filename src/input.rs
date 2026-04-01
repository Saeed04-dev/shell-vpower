//! Keyboard input handling.
//!
//! Parses crossterm key events and dispatches them as either multiplexer
//! commands (Alt+Arrow, Alt+G, Ctrl+Q) or raw input for the focused PTY.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Actions that the input handler can produce.
pub enum InputAction {
    /// Quit the application.
    Quit,
    /// Cycle to the next grid layout.
    CycleLayout,
    /// Move focus in the given direction.
    MoveFocus(Direction),
    /// Send raw bytes to the focused PTY.
    PtyInput(Vec<u8>),
    /// No action (unrecognized key combo).
    None,
}

/// Direction for focus movement.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Process a crossterm key event and return the corresponding action.
pub fn handle_key_event(event: KeyEvent) -> InputAction {
    let mods = event.modifiers;

    // Ctrl+Q → Quit
    if mods.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('q') {
        return InputAction::Quit;
    }

    // Alt combinations → multiplexer commands
    if mods.contains(KeyModifiers::ALT) {
        return match event.code {
            KeyCode::Char('g') | KeyCode::Char('G') => InputAction::CycleLayout,
            KeyCode::Up => InputAction::MoveFocus(Direction::Up),
            KeyCode::Down => InputAction::MoveFocus(Direction::Down),
            KeyCode::Left => InputAction::MoveFocus(Direction::Left),
            KeyCode::Right => InputAction::MoveFocus(Direction::Right),
            _ => InputAction::None,
        };
    }

    // Everything else → convert to bytes for the PTY
    let bytes = key_to_bytes(event);
    if bytes.is_empty() {
        InputAction::None
    } else {
        InputAction::PtyInput(bytes)
    }
}

/// Convert a key event to the byte sequence that should be sent to the PTY.
fn key_to_bytes(event: KeyEvent) -> Vec<u8> {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+A = 0x01, Ctrl+B = 0x02, etc.
                let ctrl_byte = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                if ctrl_byte <= 26 {
                    return vec![ctrl_byte];
                }
                // Ctrl+letter not in a-z — just send the char
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                buf[..c.len_utf8()].to_vec()
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                buf[..c.len_utf8()].to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => f_key_bytes(n),
        _ => Vec::new(),
    }
}

/// Generate escape sequence for function keys F1-F12.
fn f_key_bytes(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}
