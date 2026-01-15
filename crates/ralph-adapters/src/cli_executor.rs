//! CLI executor for running prompts through backends.
//!
//! Executes prompts via CLI tools with real-time streaming output.
//! Supports optional execution timeout with graceful SIGTERM termination.

use crate::cli_backend::CliBackend;
#[cfg(test)]
use crate::cli_backend::{OutputFormat, PromptMode};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

/// Result of a CLI execution.
#[derive(Debug)]
pub struct ExecutionResult {
    /// The full output from the CLI.
    pub output: String,
    /// Whether the execution succeeded (exit code 0).
    pub success: bool,
    /// The exit code.
    pub exit_code: Option<i32>,
    /// Whether the execution was terminated due to timeout.
    pub timed_out: bool,
}

/// Executor for running prompts through CLI backends.
#[derive(Debug)]
pub struct CliExecutor {
    backend: CliBackend,
}

impl CliExecutor {
    /// Creates a new executor with the given backend.
    pub fn new(backend: CliBackend) -> Self {
        Self { backend }
    }

    /// Executes a prompt and streams output to the provided writer.
    ///
    /// Output is streamed line-by-line to the writer while being accumulated
    /// for the return value. If `timeout` is provided and the execution exceeds
    /// it, the process receives SIGTERM and the result indicates timeout.
    pub async fn execute<W: Write + Send>(
        &self,
        prompt: &str,
        mut output_writer: W,
        timeout: Option<Duration>,
    ) -> std::io::Result<ExecutionResult> {
        // Note: _temp_file is kept alive for the duration of this function scope.
        // For large prompts (>7000 chars), Claude reads from the temp file.
        let (cmd, args, stdin_input, _temp_file) = self.backend.build_command(prompt, false);

        let mut command = Command::new(&cmd);
        command.args(&args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        // Set working directory to current directory (mirrors PTY executor behavior)
        let cwd = std::env::current_dir()?;
        command.current_dir(&cwd);

        debug!(
            command = %cmd,
            args = ?args,
            cwd = ?cwd,
            "Spawning CLI command"
        );

        if stdin_input.is_some() {
            command.stdin(Stdio::piped());
        }

        let mut child = command.spawn()?;

        // Write to stdin if needed
        if let Some(input) = stdin_input {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input.as_bytes()).await?;
                drop(stdin); // Close stdin to signal EOF
            }
        }

        let mut accumulated_output = String::new();
        let mut timed_out = false;

        // Wrap the streaming in a timeout if configured
        let stream_result = async {
            // Stream stdout
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Some(line) = lines.next_line().await? {
                    // Write to output writer (real-time streaming)
                    writeln!(output_writer, "{line}")?;
                    output_writer.flush()?;

                    // Accumulate for return value
                    accumulated_output.push_str(&line);
                    accumulated_output.push('\n');
                }
            }

            // Also capture stderr (append to output)
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();

                while let Some(line) = lines.next_line().await? {
                    writeln!(output_writer, "[stderr] {line}")?;
                    output_writer.flush()?;

                    accumulated_output.push_str("[stderr] ");
                    accumulated_output.push_str(&line);
                    accumulated_output.push('\n');
                }
            }

            Ok::<_, std::io::Error>(())
        };

        match timeout {
            Some(duration) => {
                debug!(timeout_secs = duration.as_secs(), "Executing with timeout");
                match tokio::time::timeout(duration, stream_result).await {
                    Ok(result) => result?,
                    Err(_) => {
                        // Timeout elapsed - send SIGTERM to the child process
                        warn!(
                            timeout_secs = duration.as_secs(),
                            "Execution timeout reached, sending SIGTERM"
                        );
                        timed_out = true;
                        Self::terminate_child(&mut child)?;
                    }
                }
            }
            None => {
                stream_result.await?;
            }
        }

        let status = child.wait().await?;

        Ok(ExecutionResult {
            output: accumulated_output,
            success: status.success() && !timed_out,
            exit_code: status.code(),
            timed_out,
        })
    }

    /// Terminates the child process with SIGTERM.
    fn terminate_child(child: &mut tokio::process::Child) -> std::io::Result<()> {
        if let Some(pid) = child.id() {
            #[allow(clippy::cast_possible_wrap)]
            let pid = Pid::from_raw(pid as i32);
            debug!(%pid, "Sending SIGTERM to child process");
            let _ = kill(pid, Signal::SIGTERM);
        }
        Ok(())
    }

    /// Executes a prompt without streaming (captures all output).
    ///
    /// Uses no timeout by default. For timed execution, use `execute_capture_with_timeout`.
    pub async fn execute_capture(&self, prompt: &str) -> std::io::Result<ExecutionResult> {
        self.execute_capture_with_timeout(prompt, None).await
    }

    /// Executes a prompt without streaming, with optional timeout.
    pub async fn execute_capture_with_timeout(
        &self,
        prompt: &str,
        timeout: Option<Duration>,
    ) -> std::io::Result<ExecutionResult> {
        // Use a sink that discards output for non-streaming execution
        let sink = std::io::sink();
        self.execute(prompt, sink, timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_echo() {
        // Use echo as a simple test backend
        let backend = CliBackend {
            command: "echo".to_string(),
            args: vec![],
            prompt_mode: PromptMode::Arg,
            prompt_flag: None,
            output_format: OutputFormat::Text,
        };

        let executor = CliExecutor::new(backend);
        let mut output = Vec::new();

        let result = executor.execute("hello world", &mut output, None).await.unwrap();

        assert!(result.success);
        assert!(!result.timed_out);
        assert!(result.output.contains("hello world"));
    }

    #[tokio::test]
    async fn test_execute_stdin() {
        // Use cat to test stdin mode
        let backend = CliBackend {
            command: "cat".to_string(),
            args: vec![],
            prompt_mode: PromptMode::Stdin,
            prompt_flag: None,
            output_format: OutputFormat::Text,
        };

        let executor = CliExecutor::new(backend);
        let result = executor.execute_capture("stdin test").await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("stdin test"));
    }

    #[tokio::test]
    async fn test_execute_failure() {
        let backend = CliBackend {
            command: "false".to_string(), // Always exits with code 1
            args: vec![],
            prompt_mode: PromptMode::Arg,
            prompt_flag: None,
            output_format: OutputFormat::Text,
        };

        let executor = CliExecutor::new(backend);
        let result = executor.execute_capture("").await.unwrap();

        assert!(!result.success);
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        // Use sleep to test timeout behavior
        // The sleep command ignores stdin, so we use PromptMode::Stdin
        // to avoid appending the prompt as an argument
        let backend = CliBackend {
            command: "sleep".to_string(),
            args: vec!["10".to_string()], // Sleep for 10 seconds
            prompt_mode: PromptMode::Stdin, // Use stdin mode so prompt doesn't interfere
            prompt_flag: None,
            output_format: OutputFormat::Text,
        };

        let executor = CliExecutor::new(backend);

        // Execute with a 100ms timeout - should trigger timeout
        let timeout = Some(Duration::from_millis(100));
        let result = executor.execute_capture_with_timeout("", timeout).await.unwrap();

        assert!(result.timed_out, "Expected execution to time out");
        assert!(!result.success, "Timed out execution should not be successful");
    }

    #[tokio::test]
    async fn test_execute_no_timeout_when_fast() {
        // Use echo which completes immediately
        let backend = CliBackend {
            command: "echo".to_string(),
            args: vec![],
            prompt_mode: PromptMode::Arg,
            prompt_flag: None,
            output_format: OutputFormat::Text,
        };

        let executor = CliExecutor::new(backend);

        // Execute with a generous timeout - should complete before timeout
        let timeout = Some(Duration::from_secs(10));
        let result = executor.execute_capture_with_timeout("fast", timeout).await.unwrap();

        assert!(!result.timed_out, "Fast command should not time out");
        assert!(result.success);
        assert!(result.output.contains("fast"));
    }
}
