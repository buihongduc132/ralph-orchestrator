//! Stream handler trait and implementations for processing Claude stream events.
//!
//! The `StreamHandler` trait abstracts over how stream events are displayed,
//! allowing for different output strategies (console, quiet, TUI, etc.).

use std::io::{self, Write};

/// Session completion result data.
#[derive(Debug, Clone)]
pub struct SessionResult {
    pub duration_ms: u64,
    pub total_cost_usd: f64,
    pub num_turns: u32,
    pub is_error: bool,
}

/// Handler for streaming output events from Claude.
///
/// Implementors receive events as Claude processes and can format/display
/// them in various ways (console output, TUI updates, logging, etc.).
pub trait StreamHandler: Send {
    /// Called when Claude emits text.
    fn on_text(&mut self, text: &str);

    /// Called when Claude invokes a tool.
    fn on_tool_call(&mut self, name: &str, id: &str);

    /// Called when a tool returns results (verbose only).
    fn on_tool_result(&mut self, id: &str, output: &str);

    /// Called when an error occurs.
    fn on_error(&mut self, error: &str);

    /// Called when session completes (verbose only).
    fn on_complete(&mut self, result: &SessionResult);
}

/// Writes streaming output to stdout/stderr.
///
/// In normal mode, displays assistant text and tool invocations.
/// In verbose mode, also displays tool results and session summary.
pub struct ConsoleStreamHandler {
    verbose: bool,
    stdout: io::Stdout,
    stderr: io::Stderr,
}

impl ConsoleStreamHandler {
    /// Creates a new console handler.
    ///
    /// # Arguments
    /// * `verbose` - If true, shows tool results and session summary.
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            stdout: io::stdout(),
            stderr: io::stderr(),
        }
    }
}

impl StreamHandler for ConsoleStreamHandler {
    fn on_text(&mut self, text: &str) {
        let _ = writeln!(self.stdout, "Claude: {}", text);
    }

    fn on_tool_call(&mut self, name: &str, _id: &str) {
        let _ = writeln!(self.stdout, "[Tool] {}", name);
    }

    fn on_tool_result(&mut self, _id: &str, output: &str) {
        if self.verbose {
            let _ = writeln!(self.stdout, "[Result] {}", truncate(output, 200));
        }
    }

    fn on_error(&mut self, error: &str) {
        // Write to both stdout (inline) and stderr (for separation)
        let _ = writeln!(self.stdout, "[Error] {}", error);
        let _ = writeln!(self.stderr, "[Error] {}", error);
    }

    fn on_complete(&mut self, result: &SessionResult) {
        if self.verbose {
            let _ = writeln!(
                self.stdout,
                "\n--- Session Complete ---\nDuration: {}ms | Cost: ${:.4} | Turns: {}",
                result.duration_ms,
                result.total_cost_usd,
                result.num_turns
            );
        }
    }
}

/// Suppresses all streaming output (for CI/silent mode).
pub struct QuietStreamHandler;

impl StreamHandler for QuietStreamHandler {
    fn on_text(&mut self, _: &str) {}
    fn on_tool_call(&mut self, _: &str, _: &str) {}
    fn on_tool_result(&mut self, _: &str, _: &str) {}
    fn on_error(&mut self, _: &str) {}
    fn on_complete(&mut self, _: &SessionResult) {}
}

/// Truncates a string to approximately `max_len` characters, adding "..." if truncated.
///
/// Uses `char_indices` to find a valid UTF-8 boundary, ensuring we never slice
/// in the middle of a multi-byte character.
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        // Find the byte index of the max_len-th character
        let byte_idx = s
            .char_indices()
            .nth(max_len)
            .map(|(idx, _)| idx)
            .unwrap_or(s.len());
        format!("{}...", &s[..byte_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_handler_verbose_shows_results() {
        let mut handler = ConsoleStreamHandler::new(true);

        // These calls should not panic
        handler.on_text("Hello");
        handler.on_tool_call("bash", "tool_1");
        handler.on_tool_result("tool_1", "output");
        handler.on_complete(&SessionResult {
            duration_ms: 1000,
            total_cost_usd: 0.01,
            num_turns: 1,
            is_error: false,
        });
    }

    #[test]
    fn test_console_handler_normal_skips_results() {
        let mut handler = ConsoleStreamHandler::new(false);

        // These should not show tool results
        handler.on_text("Hello");
        handler.on_tool_call("bash", "tool_1");
        handler.on_tool_result("tool_1", "output"); // Should be silent
        handler.on_complete(&SessionResult {
            duration_ms: 1000,
            total_cost_usd: 0.01,
            num_turns: 1,
            is_error: false,
        }); // Should be silent
    }

    #[test]
    fn test_quiet_handler_is_silent() {
        let mut handler = QuietStreamHandler;

        // All of these should be no-ops
        handler.on_text("Hello");
        handler.on_tool_call("bash", "tool_1");
        handler.on_tool_result("tool_1", "output");
        handler.on_error("Something went wrong");
        handler.on_complete(&SessionResult {
            duration_ms: 1000,
            total_cost_usd: 0.01,
            num_turns: 1,
            is_error: false,
        });
    }

    #[test]
    fn test_truncate_helper() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a long string", 10), "this is a ...");
    }

    #[test]
    fn test_truncate_utf8_boundaries() {
        // Arrow → is 3 bytes (U+2192: E2 86 92)
        let with_arrows = "→→→→→→→→→→";
        // Should truncate at character boundary, not byte boundary
        assert_eq!(truncate(with_arrows, 5), "→→→→→...");

        // Mixed ASCII and multi-byte
        let mixed = "a→b→c→d→e";
        assert_eq!(truncate(mixed, 5), "a→b→c...");

        // Emoji (4-byte characters)
        let emoji = "🎉🎊🎁🎈🎄";
        assert_eq!(truncate(emoji, 3), "🎉🎊🎁...");
    }
}
