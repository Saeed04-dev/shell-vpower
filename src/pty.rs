//! PTY spawning and management.
//!
//! Each grid cell gets a dedicated PTY running the user's shell. Output is read
//! on a dedicated tokio task and sent to the main loop via an mpsc channel.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use tokio::sync::mpsc;

/// Message sent from a PTY reader task to the main event loop.
pub struct PtyOutput {
    /// Index of the cell this output belongs to.
    pub cell_index: usize,
    /// Raw bytes of output from the PTY.
    pub data: Vec<u8>,
}

/// Manages a single PTY instance associated with a grid cell.
pub struct PtyInstance {
    /// The master side of the PTY — used for writing input and resizing.
    master: Box<dyn MasterPty + Send>,
    /// Writer handle for sending input to the PTY.
    writer: Box<dyn Write + Send>,
    /// The child process handle.
    _child: Box<dyn Child + Send + Sync>,
}

impl PtyInstance {
    /// Spawn a new PTY with the user's default shell.
    ///
    /// `cell_index` identifies which cell this PTY belongs to.
    /// `cols` and `rows` are the initial dimensions.
    /// `output_tx` is the channel sender for PTY output.
    pub fn spawn(
        cell_index: usize,
        cols: u16,
        rows: u16,
        output_tx: mpsc::UnboundedSender<PtyOutput>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(pty_size)
            .context("Failed to open PTY")?;

        // Determine the shell to run
        let shell = get_default_shell();

        let cmd = CommandBuilder::new(&shell);
        // On Windows, portable-pty handles this differently
        #[cfg(not(windows))]
        {
            cmd.env("TERM", "xterm-256color");
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell in PTY")?;

        // Drop the slave side — the master keeps the PTY alive
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        // Spawn a reader task
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to clone PTY reader")?;

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // PTY closed
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        if output_tx
                            .send(PtyOutput {
                                cell_index,
                                data,
                            })
                            .is_err()
                        {
                            break; // Receiver dropped
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master: pair.master,
            writer,
            _child: child,
        })
    }

    /// Write input bytes to the PTY (keypresses from the user).
    pub fn write_input(&mut self, data: &[u8]) -> Result<()> {
        self.writer
            .write_all(data)
            .context("Failed to write to PTY")?;
        self.writer.flush().context("Failed to flush PTY writer")?;
        Ok(())
    }

    /// Resize the PTY to new dimensions.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to resize PTY")?;
        Ok(())
    }
}

/// Check if an executable exists on PATH.
#[cfg(windows)]
fn which_exists(name: &str) -> bool {
    std::process::Command::new("where")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the default shell for the current platform.
fn get_default_shell() -> String {
    #[cfg(windows)]
    {
        // Prefer PowerShell (supports cd across drives, modern syntax)
        // Try pwsh (PowerShell 7+) first, then fall back to Windows PowerShell
        if which_exists("pwsh.exe") {
            "pwsh.exe".to_string()
        } else {
            "powershell.exe".to_string()
        }
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// Manages all PTY instances for the grid.
pub struct PtyManager {
    /// Active PTY instances, indexed by cell position.
    pub instances: Vec<Option<PtyInstance>>,
    /// Channel sender for PTY output — cloned for each new PTY.
    output_tx: mpsc::UnboundedSender<PtyOutput>,
}

impl PtyManager {
    /// Create a new PTY manager with the given output channel sender.
    pub fn new(output_tx: mpsc::UnboundedSender<PtyOutput>) -> Self {
        Self {
            instances: Vec::new(),
            output_tx,
        }
    }

    /// Ensure we have at least `count` PTY slots, spawning new ones as needed.
    /// `get_size` should return (cols, rows) for the given cell index.
    pub fn ensure_count<F>(&mut self, count: usize, get_size: F) -> Result<()>
    where
        F: Fn(usize) -> (u16, u16),
    {
        while self.instances.len() < count {
            let idx = self.instances.len();
            let (cols, rows) = get_size(idx);
            let instance = PtyInstance::spawn(idx, cols, rows, self.output_tx.clone())?;
            self.instances.push(Some(instance));
        }
        Ok(())
    }

    /// Write input to the PTY at the given cell index.
    pub fn write_to(&mut self, index: usize, data: &[u8]) -> Result<()> {
        if let Some(Some(pty)) = self.instances.get_mut(index) {
            pty.write_input(data)?;
        }
        Ok(())
    }

    /// Resize the PTY at the given cell index.
    pub fn resize(&self, index: usize, cols: u16, rows: u16) -> Result<()> {
        if let Some(Some(pty)) = self.instances.get(index) {
            pty.resize(cols, rows)?;
        }
        Ok(())
    }

    /// Resize all active PTY instances using the given size function.
    pub fn resize_all<F>(&self, get_size: F) -> Result<()>
    where
        F: Fn(usize) -> (u16, u16),
    {
        for (idx, slot) in self.instances.iter().enumerate() {
            if let Some(pty) = slot {
                let (cols, rows) = get_size(idx);
                pty.resize(cols, rows)?;
            }
        }
        Ok(())
    }
}
