//! PTY executor for running prompts with full terminal emulation.
//!
//! Spawns CLI tools in a pseudo-terminal to preserve rich TUI features like
//! colors, spinners, and animations. Supports both interactive mode (user
//! input forwarded) and observe mode (output-only).
//!
//! Key features:
//! - PTY creation via `portable-pty` for cross-platform support
//! - Idle timeout with activity tracking (output AND input reset timer)
//! - Double Ctrl+C handling (first forwards, second terminates)
//! - Raw mode management with cleanup on exit/crash

// Exit codes and PIDs are always within i32 range in practice
#![allow(clippy::cast_possible_wrap)]

use crate::cli_backend::CliBackend;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Result of a PTY execution.
#[derive(Debug)]
pub struct PtyExecutionResult {
    /// The accumulated output (ANSI sequences preserved).
    pub output: String,
    /// The ANSI-stripped output for event parsing.
    pub stripped_output: String,
    /// Whether the process exited successfully.
    pub success: bool,
    /// The exit code if available.
    pub exit_code: Option<i32>,
    /// How the process was terminated.
    pub termination: TerminationType,
}

/// How the PTY process was terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationType {
    /// Process exited naturally.
    Natural,
    /// Terminated due to idle timeout.
    IdleTimeout,
    /// Terminated by user (double Ctrl+C).
    UserInterrupt,
    /// Force killed by user (Ctrl+\).
    ForceKill,
}

/// Configuration for PTY execution.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    /// Enable interactive mode (forward user input).
    pub interactive: bool,
    /// Idle timeout in seconds (0 = disabled).
    pub idle_timeout_secs: u32,
    /// Terminal width.
    pub cols: u16,
    /// Terminal height.
    pub rows: u16,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            interactive: true,
            idle_timeout_secs: 30,
            cols: 80,
            rows: 24,
        }
    }
}

impl PtyConfig {
    /// Creates config from environment, falling back to defaults.
    pub fn from_env() -> Self {
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80);
        let rows = std::env::var("LINES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);

        Self {
            cols,
            rows,
            ..Default::default()
        }
    }
}

/// State machine for double Ctrl+C detection.
#[derive(Debug)]
pub struct CtrlCState {
    /// When the first Ctrl+C was pressed (if any).
    first_press: Option<Instant>,
    /// Window duration for double-press detection.
    window: Duration,
}

/// Action to take after handling Ctrl+C.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtrlCAction {
    /// Forward the Ctrl+C to Claude and start/restart the window.
    ForwardAndStartWindow,
    /// Terminate Claude (second Ctrl+C within window).
    Terminate,
}

impl CtrlCState {
    /// Creates a new Ctrl+C state tracker.
    pub fn new() -> Self {
        Self {
            first_press: None,
            window: Duration::from_secs(1),
        }
    }

    /// Handles a Ctrl+C keypress and returns the action to take.
    pub fn handle_ctrl_c(&mut self, now: Instant) -> CtrlCAction {
        match self.first_press {
            Some(first) if now.duration_since(first) < self.window => {
                // Second Ctrl+C within window - terminate
                self.first_press = None;
                CtrlCAction::Terminate
            }
            _ => {
                // First Ctrl+C or window expired - forward and start window
                self.first_press = Some(now);
                CtrlCAction::ForwardAndStartWindow
            }
        }
    }
}

impl Default for CtrlCState {
    fn default() -> Self {
        Self::new()
    }
}

/// Executor for running prompts in a pseudo-terminal.
pub struct PtyExecutor {
    backend: CliBackend,
    config: PtyConfig,
}

impl PtyExecutor {
    /// Creates a new PTY executor with the given backend and configuration.
    pub fn new(backend: CliBackend, config: PtyConfig) -> Self {
        Self { backend, config }
    }

    /// Spawns Claude in a PTY and returns the PTY pair and child process.
    fn spawn_pty(&self, prompt: &str) -> io::Result<(PtyPair, Box<dyn portable_pty::Child + Send>)> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: self.config.rows,
                cols: self.config.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| io::Error::other(e.to_string()))?;

        let (cmd, args, stdin_input) = self.backend.build_command(prompt);

        let mut cmd_builder = CommandBuilder::new(&cmd);
        cmd_builder.args(&args);

        // Set up environment for PTY
        cmd_builder.env("TERM", "xterm-256color");

        debug!(
            command = %cmd,
            args = ?args,
            cols = self.config.cols,
            rows = self.config.rows,
            "Spawning process in PTY"
        );

        let child = pair
            .slave
            .spawn_command(cmd_builder)
            .map_err(|e| io::Error::other(e.to_string()))?;

        // If we need to write to stdin, do it now
        if let Some(input) = stdin_input {
            let mut writer = pair.master.take_writer()
                .map_err(|e| io::Error::other(e.to_string()))?;
            writer.write_all(input.as_bytes())?;
        }

        Ok((pair, child))
    }

    /// Runs in observe mode (output-only, no input forwarding).
    ///
    /// Returns when the process exits or idle timeout triggers.
    ///
    /// # Errors
    ///
    /// Returns an error if PTY allocation fails, the command cannot be spawned,
    /// or an I/O error occurs during output handling.
    pub fn run_observe(&self, prompt: &str) -> io::Result<PtyExecutionResult> {
        let (pair, mut child) = self.spawn_pty(prompt)?;

        let mut reader = pair.master.try_clone_reader()
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Drop the slave to signal EOF when master closes
        drop(pair.slave);

        let mut output = Vec::new();
        let mut buf = [0u8; 4096];
        let mut last_activity = Instant::now();
        let timeout = if self.config.idle_timeout_secs > 0 {
            Some(Duration::from_secs(u64::from(self.config.idle_timeout_secs)))
        } else {
            None
        };

        let mut termination = TerminationType::Natural;

        loop {
            // Check if child has exited
            if let Some(status) = child.try_wait()
                .map_err(|e| io::Error::other(e.to_string()))?
            {
                debug!(exit_status = ?status, "Child process exited");
                // Drain any remaining output
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            io::stdout().write_all(&buf[..n])?;
                            io::stdout().flush()?;
                            output.extend_from_slice(&buf[..n]);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                let exit_code = status.exit_code();
                let success = status.success();

                return Ok(PtyExecutionResult {
                    output: String::from_utf8_lossy(&output).to_string(),
                    stripped_output: strip_ansi(&output),
                    success,
                    exit_code: Some(exit_code as i32),
                    termination,
                });
            }

            // Check idle timeout
            if let Some(timeout_duration) = timeout {
                if last_activity.elapsed() > timeout_duration {
                    warn!(
                        timeout_secs = self.config.idle_timeout_secs,
                        "Idle timeout triggered"
                    );
                    termination = TerminationType::IdleTimeout;
                    self.terminate_child(&mut child, true)?;
                    break;
                }
            }

            // Read output (non-blocking would be ideal, but we use small timeout)
            // For simplicity, we do blocking reads with a timeout check
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF - process likely exited
                    break;
                }
                Ok(n) => {
                    // Write to stdout in real-time
                    io::stdout().write_all(&buf[..n])?;
                    io::stdout().flush()?;

                    // Accumulate for return
                    output.extend_from_slice(&buf[..n]);

                    // Reset activity timer
                    last_activity = Instant::now();
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No data available, sleep briefly
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    // Real error or EOF
                    debug!(error = %e, "PTY read error");
                    break;
                }
            }
        }

        // Wait for child to fully exit
        let status = child.wait()
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(PtyExecutionResult {
            output: String::from_utf8_lossy(&output).to_string(),
            stripped_output: strip_ansi(&output),
            success: status.success(),
            exit_code: Some(status.exit_code() as i32),
            termination,
        })
    }

    /// Runs in interactive mode (bidirectional I/O).
    ///
    /// User input is forwarded to Claude (except reserved keys).
    /// Returns when the process exits, idle timeout triggers, or user interrupts.
    ///
    /// # Errors
    ///
    /// Returns an error if PTY allocation fails, the command cannot be spawned,
    /// or an I/O error occurs during bidirectional communication.
    #[allow(clippy::too_many_lines)] // Complex state machine requires cohesive implementation
    pub fn run_interactive(&self, prompt: &str) -> io::Result<PtyExecutionResult> {
        let (pair, mut child) = self.spawn_pty(prompt)?;

        let mut reader = pair.master.try_clone_reader()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let mut writer = pair.master.take_writer()
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Drop the slave
        drop(pair.slave);

        let mut output = Vec::new();
        let mut last_activity = Instant::now();
        let timeout = if self.config.idle_timeout_secs > 0 {
            Some(Duration::from_secs(u64::from(self.config.idle_timeout_secs)))
        } else {
            None
        };

        let mut ctrl_c_state = CtrlCState::new();
        let mut termination = TerminationType::Natural;

        // Flag for termination request
        let should_terminate = Arc::new(AtomicBool::new(false));
        let force_kill = Arc::new(AtomicBool::new(false));

        // Spawn input handling thread
        let should_terminate_clone = Arc::clone(&should_terminate);
        let force_kill_clone = Arc::clone(&force_kill);
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<InputEvent>();

        std::thread::spawn(move || {
            let mut stdin = io::stdin();
            let mut buf = [0u8; 1];

            loop {
                if should_terminate_clone.load(Ordering::SeqCst) {
                    break;
                }

                match stdin.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(1) => {
                        let byte = buf[0];
                        let event = match byte {
                            3 => InputEvent::CtrlC,      // Ctrl+C
                            28 => InputEvent::CtrlBackslash, // Ctrl+\
                            _ => InputEvent::Data(vec![byte]),
                        };
                        if input_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {} // Shouldn't happen with 1-byte buffer
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });

        // Main loop
        let mut buf = [0u8; 4096];

        loop {
            // Check if child has exited
            if let Some(status) = child.try_wait()
                .map_err(|e| io::Error::other(e.to_string()))?
            {
                debug!(exit_status = ?status, "Child process exited");

                // Drain remaining output
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            io::stdout().write_all(&buf[..n])?;
                            io::stdout().flush()?;
                            output.extend_from_slice(&buf[..n]);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }

                should_terminate.store(true, Ordering::SeqCst);

                return Ok(PtyExecutionResult {
                    output: String::from_utf8_lossy(&output).to_string(),
                    stripped_output: strip_ansi(&output),
                    success: status.success(),
                    exit_code: Some(status.exit_code() as i32),
                    termination,
                });
            }

            // Check idle timeout
            if let Some(timeout_duration) = timeout {
                if last_activity.elapsed() > timeout_duration {
                    warn!(
                        timeout_secs = self.config.idle_timeout_secs,
                        "Idle timeout triggered"
                    );
                    termination = TerminationType::IdleTimeout;
                    should_terminate.store(true, Ordering::SeqCst);
                    self.terminate_child(&mut child, true)?;
                    break;
                }
            }

            // Check for force kill flag
            if force_kill.load(Ordering::SeqCst) {
                termination = TerminationType::ForceKill;
                should_terminate.store(true, Ordering::SeqCst);
                self.terminate_child(&mut child, false)?;
                break;
            }

            // Process input events (non-blocking)
            while let Ok(event) = input_rx.try_recv() {
                match event {
                    InputEvent::CtrlC => {
                        match ctrl_c_state.handle_ctrl_c(Instant::now()) {
                            CtrlCAction::ForwardAndStartWindow => {
                                // Forward Ctrl+C to Claude
                                let _ = writer.write_all(&[3]);
                                let _ = writer.flush();
                                last_activity = Instant::now();
                            }
                            CtrlCAction::Terminate => {
                                info!("Double Ctrl+C detected, terminating");
                                termination = TerminationType::UserInterrupt;
                                should_terminate.store(true, Ordering::SeqCst);
                                self.terminate_child(&mut child, true)?;
                            }
                        }
                    }
                    InputEvent::CtrlBackslash => {
                        info!("Ctrl+\\ detected, force killing");
                        force_kill_clone.store(true, Ordering::SeqCst);
                    }
                    InputEvent::Data(data) => {
                        // Forward to Claude
                        let _ = writer.write_all(&data);
                        let _ = writer.flush();
                        last_activity = Instant::now();
                    }
                }

                // Exit loop if termination requested
                if should_terminate.load(Ordering::SeqCst) {
                    break;
                }
            }

            if should_terminate.load(Ordering::SeqCst) {
                break;
            }

            // Read output
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    io::stdout().write_all(&buf[..n])?;
                    io::stdout().flush()?;
                    output.extend_from_slice(&buf[..n]);
                    last_activity = Instant::now();
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    debug!(error = %e, "PTY read error");
                    break;
                }
            }
        }

        // Ensure termination flag is set for input thread
        should_terminate.store(true, Ordering::SeqCst);

        // Wait for child to fully exit
        let status = child.wait()
            .map_err(|e| io::Error::other(e.to_string()))?;

        Ok(PtyExecutionResult {
            output: String::from_utf8_lossy(&output).to_string(),
            stripped_output: strip_ansi(&output),
            success: status.success(),
            exit_code: Some(status.exit_code() as i32),
            termination,
        })
    }

    /// Terminates the child process.
    ///
    /// If `graceful` is true, sends SIGTERM and waits up to 5 seconds before SIGKILL.
    /// If `graceful` is false, sends SIGKILL immediately.
    #[allow(clippy::unused_self)] // Self is conceptually the right receiver for this method
    fn terminate_child(&self, child: &mut Box<dyn portable_pty::Child + Send>, graceful: bool) -> io::Result<()> {
        let pid = match child.process_id() {
            Some(id) => Pid::from_raw(id as i32),
            None => return Ok(()), // Already exited
        };

        if graceful {
            debug!(pid = %pid, "Sending SIGTERM");
            let _ = kill(pid, Signal::SIGTERM);

            // Wait up to 5 seconds for graceful exit
            let grace_period = Duration::from_secs(5);
            let start = Instant::now();

            while start.elapsed() < grace_period {
                if child.try_wait()
                    .map_err(|e| io::Error::other(e.to_string()))?
                    .is_some()
                {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            // Still running after grace period - force kill
            debug!(pid = %pid, "Grace period expired, sending SIGKILL");
        }

        debug!(pid = %pid, "Sending SIGKILL");
        let _ = kill(pid, Signal::SIGKILL);
        Ok(())
    }
}

/// Input events from the user.
#[derive(Debug)]
enum InputEvent {
    /// Ctrl+C pressed.
    CtrlC,
    /// Ctrl+\ pressed.
    CtrlBackslash,
    /// Regular data to forward.
    Data(Vec<u8>),
}

/// Strips ANSI escape sequences from raw bytes.
fn strip_ansi(bytes: &[u8]) -> String {
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(bytes);
    parser.screen().contents()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_ctrl_c_within_window() {
        let mut state = CtrlCState::new();
        let now = Instant::now();

        // First Ctrl+C: should forward and start window
        let action = state.handle_ctrl_c(now);
        assert_eq!(action, CtrlCAction::ForwardAndStartWindow);

        // Second Ctrl+C within 1 second: should terminate
        let later = now + Duration::from_millis(500);
        let action = state.handle_ctrl_c(later);
        assert_eq!(action, CtrlCAction::Terminate);
    }

    #[test]
    fn test_ctrl_c_window_expires() {
        let mut state = CtrlCState::new();
        let now = Instant::now();

        // First Ctrl+C
        state.handle_ctrl_c(now);

        // Wait 2 seconds (window expires)
        let later = now + Duration::from_secs(2);

        // Second Ctrl+C: window expired, should forward and start new window
        let action = state.handle_ctrl_c(later);
        assert_eq!(action, CtrlCAction::ForwardAndStartWindow);
    }

    #[test]
    fn test_strip_ansi_basic() {
        let input = b"\x1b[1;36m  Thinking...\x1b[0m\r\n";
        let stripped = strip_ansi(input);
        assert!(stripped.contains("Thinking..."));
        assert!(!stripped.contains("\x1b["));
    }

    #[test]
    fn test_completion_promise_extraction() {
        let mut term = vt100::Parser::new(24, 80, 0);

        // Simulate Claude output with heavy ANSI formatting
        term.process(b"\x1b[1;36m  Thinking...\x1b[0m\r\n");
        term.process(b"\x1b[2K\x1b[1;32m  Done!\x1b[0m\r\n");
        term.process(b"\x1b[33mLOOP_COMPLETE\x1b[0m\r\n");

        let stripped = term.screen().contents();

        // Event parser sees clean text
        assert!(stripped.contains("LOOP_COMPLETE"));
        assert!(!stripped.contains("\x1b["));
    }

    #[test]
    fn test_event_tag_extraction() {
        let mut term = vt100::Parser::new(24, 80, 0);

        // Event tags may be wrapped in ANSI codes
        term.process(b"\x1b[90m<event topic=\"build.done\">\x1b[0m\r\n");
        term.process(b"Task completed successfully\r\n");
        term.process(b"\x1b[90m</event>\x1b[0m\r\n");

        let stripped = term.screen().contents();

        assert!(stripped.contains("<event topic=\"build.done\">"));
        assert!(stripped.contains("</event>"));
    }

    #[test]
    fn test_pty_config_defaults() {
        let config = PtyConfig::default();
        assert!(config.interactive);
        assert_eq!(config.idle_timeout_secs, 30);
        assert_eq!(config.cols, 80);
        assert_eq!(config.rows, 24);
    }
}
