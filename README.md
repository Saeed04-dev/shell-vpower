<p align="center">
  <img src="https://img.shields.io/badge/built%20with-Rust-e43717?style=for-the-badge&logo=rust&logoColor=white" alt="Built with Rust"/>
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="MIT License"/>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-brightgreen?style=for-the-badge" alt="Cross Platform"/>
  <img src="https://img.shields.io/badge/version-0.1.0-orange?style=for-the-badge" alt="Version"/>
</p>

<h1 align="center">Shell V-Power</h1>

<p align="center">
  <strong>A blazing-fast terminal multiplexer built in Rust.</strong><br/>
  Split your terminal into a grid. Run multiple shells at once. Stay in the flow.
</p>

<p align="center">
  <code>2x2</code> &nbsp;&bull;&nbsp; <code>3x3</code> &nbsp;&bull;&nbsp; <code>4x4</code> &nbsp;&mdash;&nbsp; up to <strong>16 terminals</strong> in a single window.
</p>

---

## Why Shell V-Power?

Most terminal multiplexers are powerful but complex. Shell V-Power takes a different approach:

- **Instant grid layouts** &mdash; no manual splitting, no memorizing pane commands
- **Full color & TUI support** &mdash; run Claude Code, vim, htop with proper rendering
- **Clipboard integration** &mdash; Ctrl+C/V copy & paste, mouse text selection
- **Lightweight** &mdash; optimized binary with LTO, minimal memory footprint
- **Cross-platform** &mdash; runs on Linux, macOS, and Windows out of the box
- **Zero config** &mdash; just launch and start working

```
┌──────────────┬──────────────┐
│ $ make build │ $ tail -f log│
│              │              │
├──────────────┼──────────────┤
│ $ git status │ $ htop      │
│              │              │
└──────────────┴──────────────┘
        Shell V-Power (2x2)
```

## Features

| Feature | Description |
|---------|-------------|
| **Dynamic Grid Engine** | Switch between 2x2, 3x3, and 4x4 layouts instantly |
| **Real PTY Integration** | Every cell runs an actual shell instance (PowerShell, bash, zsh) |
| **ANSI Color Support** | Full 256-color and RGB true color rendering |
| **Alternate Screen Buffer** | TUI apps (vim, htop, Claude Code) render correctly |
| **Clipboard Integration** | Ctrl+C to copy, Ctrl+V to paste, mouse text selection |
| **Mouse Selection** | Click to focus cells, drag to select text, right-click to paste |
| **Scroll Regions** | Proper DECSTBM support for TUI scroll areas |
| **Keyboard Multiplexing** | Navigate between cells with simple key combos |
| **Responsive Resize** | Grid adapts automatically when you resize the window |
| **Scrollback Buffer** | 1000 lines of history per cell |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.70+
- A C linker (gcc/clang on Linux/macOS, MSVC Build Tools on Windows)

### Install from source

```bash
git clone https://github.com/Saeed04-dev/shell-vpower.git
cd shell-vpower
cargo build --release
```

The binary will be at `target/release/vpower-shell` (or `vpower-shell.exe` on Windows).

### Run

```bash
./target/release/vpower-shell
```

That's it. You'll see a 2x2 grid with four live shell sessions.

## Keybindings

| Key | Action |
|-----|--------|
| <kbd>Alt</kbd> + <kbd>G</kbd> | Cycle layout: 2x2 &rarr; 3x3 &rarr; 4x4 &rarr; 2x2 |
| <kbd>Ctrl</kbd> + <kbd>&uarr;</kbd> | Move focus up |
| <kbd>Ctrl</kbd> + <kbd>&darr;</kbd> | Move focus down |
| <kbd>Ctrl</kbd> + <kbd>&larr;</kbd> | Move focus left |
| <kbd>Ctrl</kbd> + <kbd>&rarr;</kbd> | Move focus right |
| <kbd>Ctrl</kbd> + <kbd>C</kbd> | Copy selected text (or send interrupt if no selection) |
| <kbd>Ctrl</kbd> + <kbd>V</kbd> | Paste from clipboard |
| <kbd>Ctrl</kbd> + <kbd>Q</kbd> | Quit |

### Mouse

| Action | Effect |
|--------|--------|
| **Left click** | Focus cell + start selection |
| **Left drag** | Select text within cell |
| **Right click** | Paste from clipboard |

The **focused cell** is highlighted with a cyan border. All other keyboard input goes directly to the focused shell.

## Architecture

```
                    ┌─────────────────────────┐
                    │       main.rs            │
                    │   Terminal Setup + Loop  │
                    └────────────┬────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │        app.rs           │
                    │  tokio::select! loop    │
                    │  State + Event Router   │
                    └──┬─────┬─────┬─────┬───┘
                       │     │     │     │
              ┌────────▼┐ ┌──▼───┐ │  ┌──▼──────────┐
              │ input.rs│ │ui.rs │ │  │terminal_cell │
              │ Key Map │ │Render│ │  │  VTE Parse   │
              └─────────┘ └──────┘ │  └──────────────┘
                                   │
                    ┌──────────────▼──────────┐
                    │         pty.rs          │
                    │  PTY Spawn + I/O Threads│
                    └──────────┬─────────────┘
                               │
                    ┌──────────▼─────────────┐
                    │        grid.rs         │
                    │   Layout Math Engine   │
                    └────────────────────────┘
```

**Data Flow:**

1. **Input** &rarr; crossterm captures keystrokes &rarr; `input.rs` routes them
2. **To PTY** &rarr; keystrokes forwarded to the focused shell via `pty.rs`
3. **From PTY** &rarr; reader threads push output through mpsc channels
4. **Parse** &rarr; `terminal_cell.rs` processes ANSI sequences via VTE
5. **Render** &rarr; `ui.rs` draws the grid with ratatui

## Tech Stack

| Crate | Role |
|-------|------|
| [ratatui](https://crates.io/crates/ratatui) | TUI rendering framework |
| [crossterm](https://crates.io/crates/crossterm) | Cross-platform terminal control |
| [portable-pty](https://crates.io/crates/portable-pty) | PTY spawning (Linux, macOS, Windows) |
| [tokio](https://crates.io/crates/tokio) | Async runtime for concurrent I/O |
| [vte](https://crates.io/crates/vte) | VT100/ANSI escape sequence parser |
| [arboard](https://crates.io/crates/arboard) | Cross-platform clipboard support |

## Project Structure

```
src/
├── main.rs            # Entry point, terminal setup
├── app.rs             # App state, async event loop, clipboard, mouse
├── grid.rs            # Grid layout engine (+ unit tests)
├── input.rs           # Keyboard input mapping
├── pty.rs             # PTY process management
├── terminal_cell.rs   # Per-cell terminal buffer, VTE parser, alt screen
└── ui.rs              # ratatui widgets (grid + status bar + selection)
```

## Roadmap

- [x] ANSI color support (256-color + RGB)
- [x] Alternate screen buffer for TUI apps
- [x] Clipboard copy/paste (Ctrl+C/V)
- [x] Mouse text selection
- [x] Scroll regions (DECSTBM)
- [ ] Custom keybinding configuration
- [ ] Scrollback navigation (Shift+PageUp/Down)
- [ ] Session save & restore
- [ ] Plugin system
- [ ] Configurable color themes
- [ ] Named cells / cell labels
- [ ] SSH session support
- [ ] Custom grid dimensions (e.g. 2x3, 1x4)

## Performance

Shell V-Power is built for speed:

- **Link-Time Optimization (LTO)** enabled for release builds
- **Binary stripping** for minimal size
- **Non-blocking I/O** via dedicated reader threads per PTY
- **Zero-copy rendering** with ratatui's buffer system
- **Minimal dependencies** &mdash; only what's needed, nothing more

## Contributing

Contributions are welcome! Here's how to get started:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes and add tests
4. Run the test suite: `cargo test`
5. Submit a pull request

Please open an issue first for major changes so we can discuss the approach.

## License

This project is licensed under the **MIT License**. See [LICENSE](LICENSE) for details.

---

<p align="center">
  <strong>Shell V-Power</strong> &mdash; because your terminal should keep up with you.
</p>