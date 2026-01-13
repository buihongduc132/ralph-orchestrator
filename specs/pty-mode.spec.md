# PTY Mode Specification

## Overview

Enable users to observe and optionally interact with Claude in real-time by running it in a pseudo-terminal (PTY) instead of headless mode. The PTY captures Claude's rich terminal UI while allowing Ralph to orchestrate iterations. An idle timeout mechanism terminates the process when output stalls, with timeout reset on user input to support interactive sessions.

## Problem Statement

Claude CLI **requires a TTY even with the `-p` flag**. This is a known issue ([GitHub #9026](https://github.com/anthropics/claude-code/issues/9026)) where Claude hangs indefinitely when spawned from non-TTY contexts. Additionally, running Claude in headless mode loses the rich terminal experience:

- No progress indicators or spinner animations
- No visual tool call feedback
- No streaming thought process display
- Users can't "follow along" as Claude works

For debugging, demos, and learning how Ralph orchestrates, users want to see exactly what Claude sees—and optionally interact when Claude needs input.

## Prior Art

| Tool | What It Does | Gap |
|------|--------------|-----|
| [**Zellij**](https://github.com/zellij-org/zellij) | Terminal multiplexer with keybinding-based input routing | Full multiplexer, overkill for our use case |
| [**tui-term**](https://github.com/a-kenji/tui-term) | PTY widget for Ratatui using vt100 + portable-pty | Requires full TUI, complex |
| [**faketty**](https://github.com/dtolnay/faketty) | Rust PTY wrapper preserving stdout/stderr separation | No idle timeout, no input handling |
| [**expect**](https://man7.org/linux/man-pages/man1/expect.1.html) | Classic PTY automation with pattern matching and timeout | Tcl scripted, not interactive |
| [**portable-pty**](https://docs.rs/portable-pty/latest/portable_pty/) | Cross-platform PTY abstraction (used by wezterm) | Low-level, no timeout or input routing |

**Key insight from Zellij:** Input routing via keybindings solves the "who gets the keystroke?" problem. Bound keys control the multiplexer; unbound keys pass to the focused pane. We adopt a simpler version: reserved keys control Ralph, everything else goes to Claude.

## Solution

PTY mode spawns Claude in a pseudo-terminal with bidirectional I/O:
- **Output:** Claude's TUI is forwarded to user's terminal in real-time
- **Input:** User keystrokes are forwarded to Claude (except reserved keys)
- **Timeout:** Idle timer resets on both Claude output AND user input

```
┌─────────────────────────────────────────────────────────────────────┐
│                     PTY Mode Architecture                            │
│                                                                      │
│  ┌──────────────┐                                                   │
│  │    Ralph     │                                                   │
│  │ (EventLoop)  │                                                   │
│  └──────┬───────┘                                                   │
│         │                                                           │
│         ▼                                                           │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                      PtyExecutor                             │   │
│  │                                                              │   │
│  │  User's stdin ──┬─▶ Ctrl+C ×2? ──▶ Ralph: SIGTERM Claude    │   │
│  │                 │   (within 1s)                              │   │
│  │                 ├─▶ Ctrl+\ ? ────▶ Ralph: SIGKILL Claude    │   │
│  │                 │                                            │   │
│  │                 └─▶ Other ───────▶ PTY Master ───▶ Claude   │   │
│  │                     (inc. 1st C-c)      │                    │   │
│  │                                         │ reset timeout      │   │
│  │                                         ▼                    │   │
│  │  PTY Master ◀─── Claude output ◀─── Claude (PTY Slave)      │   │
│  │       │                                                      │   │
│  │       ├──▶ User's stdout (real-time display)                │   │
│  │       ├──▶ Accumulate buffer (for event parsing)            │   │
│  │       └──▶ Reset timeout (activity detected)                │   │
│  │                                                              │   │
│  │  Timeout? ───yes───▶ SIGTERM ──▶ grace ──▶ SIGKILL          │   │
│  │                                                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

## Configuration

### ralph.yml

```yaml
event_loop:
  prompt_file: "PROMPT.md"
  completion_promise: "LOOP_COMPLETE"

cli:
  backend: "claude"
  pty_mode: true              # Enable PTY mode (default: false)
  pty_interactive: true       # Forward user input to Claude (default: true)
  idle_timeout_secs: 30       # Kill after N seconds of inactivity (default: 30)
```

### CLI Flags

```bash
# Enable PTY mode (interactive by default)
ralph loop --pty

# PTY mode, observation only (no input forwarding)
ralph loop --pty --observe

# Custom idle timeout
ralph loop --pty --idle-timeout 60

# Disable idle timeout (rely on natural exit only)
ralph loop --pty --idle-timeout 0

# Disable PTY mode even if config enables it
ralph loop --no-pty
```

## Behavior

### Mode Selection

| Config `pty_mode` | CLI Flag | Effective Mode |
|-------------------|----------|----------------|
| `false` | (none) | Headless |
| `false` | `--pty` | PTY Interactive |
| `false` | `--pty --observe` | PTY Observe-only |
| `false` | `--observe` | PTY Observe-only (`--observe` implies `--pty`) |
| `true` | (none) | PTY Interactive |
| `true` | `--observe` | PTY Observe-only |
| `true` | `--no-pty` | Headless |

**Note:** The `--observe` flag implicitly enables PTY mode. Using `--observe` without `--pty` is valid and equivalent to `--pty --observe`.

### PTY Setup

- **Dimensions:** Inherit from user's terminal (`$COLUMNS` x `$LINES`), fallback to 80x24
- **Prompt delivery:** Unchanged from headless mode (`-p` flag still used)
- **Terminal mode:** Raw mode enabled for proper keystroke capture

### Input Routing

User keystrokes are routed based on reserved keys:

| Key | Action |
|-----|--------|
| `Ctrl+C` (×2) | Two consecutive Ctrl+C within 1 second stops iteration (SIGTERM → Claude) |
| `Ctrl+C` (×1) | Single Ctrl+C is forwarded to Claude (allows Claude's own interrupt handling) |
| `Ctrl+\` | Force kill (SIGKILL → Claude, abort iteration) |
| All other keys | Forward to Claude's PTY stdin |

**Ctrl+C behavior (matching Claude Code):**
- First `Ctrl+C`: Forwarded to Claude, starts 1-second window
- Second `Ctrl+C` within window: Ralph intercepts, sends SIGTERM to Claude
- If window expires without second `Ctrl+C`: Reset, next `Ctrl+C` starts new window

In `--observe` mode, all input is ignored (stdin not connected to PTY).

### Idle Detection

The idle timeout triggers when **both** conditions are met:
1. No bytes received from Claude for `idle_timeout_secs`
2. No bytes sent by user for `idle_timeout_secs`

**What resets the timer:**
- Claude outputs any bytes (including ANSI sequences)
- User presses any key that is forwarded to Claude (in interactive mode)
- First Ctrl+C (since it's forwarded to Claude)

**What doesn't reset the timer:**
- Ralph's own reads/writes
- Second Ctrl+C (triggers SIGTERM, not forwarded)
- Ctrl+\ (triggers SIGKILL, not forwarded)

**Special case:** `idle_timeout_secs: 0` disables idle timeout entirely.

### Termination Sequence

When idle timeout triggers OR user presses Ctrl+C twice:

```
1. [Ralph] ─────SIGTERM─────▶ [Claude]
                                 │
                                 │ 5 second grace period (hardcoded)
                                 ▼
2. [Ralph] ◀────natural exit──── [Claude]  (if graceful)
           ─────SIGKILL─────▶    [Claude]  (if still running)
```

When user presses Ctrl+\ (emergency escape):
```
1. [Ralph] ─────SIGKILL─────▶ [Claude]  (immediate, no grace period)
```

### Output Handling

PTY output is:
1. **Written to Ralph's stdout** — User sees Claude's TUI in real-time
2. **Accumulated in buffer** — For event parsing after process terminates
3. **ANSI-stripped for parsing** — Escape sequences removed before scanning for events/completion promise

## Integration with Existing Components

### PtyExecutor

`PtyExecutor` is a new struct in `ralph-adapters`:

| Method | Description |
|--------|-------------|
| `new(backend, config)` | Create executor with backend and PTY config |
| `spawn(prompt)` | Create PTY, spawn Claude with prompt |
| `run_interactive()` | Main loop: forward I/O bidirectionally, track activity |
| `run_observe()` | Output-only loop: forward output, ignore input |
| `terminate(graceful)` | SIGTERM (graceful=true) or SIGKILL (graceful=false) |

### Input Loop

The input handling runs concurrently with output handling:

```
spawn two tasks:
  1. Output task: read PTY → write stdout, accumulate, reset timeout
  2. Input task: read stdin → check reserved → write PTY, reset timeout

select! on:
  - Output task completion (PTY read returns EOF or child process exits)
  - Idle timeout expired
  - Reserved key detected (double Ctrl+C or Ctrl+\)
```

**Output task completion:** The output task completes when the PTY master read returns 0 bytes (EOF), which occurs after the child process exits and all buffered output is consumed.

### EventLoop Integration

```
if config.pty_mode && stdout_is_tty() {
    let executor = PtyExecutor::new(backend, config);
    if config.pty_interactive {
        executor.run_interactive(prompt)
    } else {
        executor.run_observe(prompt)
    }
} else {
    CliExecutor::new(backend).execute(prompt, writer)
}
```

## Edge Cases

### Terminal Not Available

If Ralph's stdout is not a TTY (e.g., piped, CI pipeline), PTY mode:
1. Logs a warning: "PTY mode requested but stdout is not a TTY, falling back to headless"
2. Falls back to headless mode automatically

### PTY Allocation Fails

If `portable-pty` fails to allocate a PTY:
1. Logs an error with details
2. Falls back to headless mode

### Long-Running Tool Calls

Claude may spawn long-running commands that produce no output. Mitigations:

1. **Default timeout is generous** — 30 seconds accommodates most builds
2. **User activity extends timeout** — Pressing any key resets the timer
3. **User can increase timeout** — `--idle-timeout 120` for slow builds
4. **Disable if needed** — `--idle-timeout 0` for truly long operations

### User Types During Claude Output

User input is buffered and forwarded to Claude's PTY. Claude receives it when ready. This is standard terminal behavior.

### Claude Prompts for Confirmation

In interactive mode, user can respond naturally. In observe mode, Claude receives EOF and the idle timeout eventually triggers.

### Terminal Resize

SIGWINCH (terminal resize) is **not** propagated to the PTY. The PTY dimensions are set once at spawn time. This simplifies implementation and matches `faketty`'s behavior.

### Raw Mode Restoration

Ralph enters raw mode to capture individual keystrokes. On exit (normal or crash), raw mode must be restored:
- Use `scopeguard` or similar to ensure cleanup
- Handle SIGINT/SIGTERM to restore terminal state

## Crate Placement

| Component | Crate |
|-----------|-------|
| `PtyExecutor` | `ralph-adapters` |
| PTY config fields | `ralph-core` (in `CliConfig`) |
| CLI flags | `ralph-cli` |
| Raw mode handling | `ralph-cli` (uses `crossterm`) |

### CliConfig Extension

The existing `CliConfig` struct in `ralph-core/src/config.rs` must be extended with:

```rust
// Add to CliConfig struct
pub pty_mode: bool,           // default: false
pub pty_interactive: bool,    // default: true (only relevant when pty_mode=true)
pub idle_timeout_secs: u32,   // default: 30
```

These fields map directly to the YAML config under `cli:` section.

## Dependencies

Add to `ralph-adapters/Cargo.toml`:

```toml
[dependencies]
portable-pty = "0.8"
```

Add to `ralph-cli/Cargo.toml`:

```toml
[dependencies]
crossterm = "0.28"  # For raw mode and key event reading
```

## Acceptance Criteria

### Configuration

- **Given** `pty_mode: true` in config
- **When** Ralph starts an iteration and stdout is a TTY
- **Then** Claude is spawned in a PTY via `portable-pty`

- **Given** `--pty` flag without `--observe`
- **When** iteration runs
- **Then** interactive mode is enabled (user input forwarded)

- **Given** `--pty --observe` flags
- **When** iteration runs
- **Then** observe mode is enabled (user input ignored)

### Output Display

- **Given** PTY mode is active (either interactive or observe)
- **When** Claude produces TUI output
- **Then** output is written to Ralph's stdout in real-time

- **Given** PTY mode is active
- **When** Claude shows spinner/progress animations
- **Then** animations render correctly (ANSI sequences preserved)

### Input Routing (Interactive Mode)

- **Given** interactive PTY mode is active
- **When** user presses a regular key (e.g., 'y', Enter)
- **Then** key is forwarded to Claude's PTY stdin

- **Given** interactive PTY mode is active
- **When** user presses Ctrl+C once
- **Then** Ctrl+C is forwarded to Claude AND 1-second window starts

- **Given** interactive PTY mode is active and Ctrl+C window is open
- **When** user presses Ctrl+C again within 1 second
- **Then** SIGTERM is sent to Claude (second Ctrl+C NOT forwarded)

- **Given** interactive PTY mode is active and Ctrl+C window is open
- **When** 1 second passes without second Ctrl+C
- **Then** window closes, next Ctrl+C starts fresh window

- **Given** interactive PTY mode is active
- **When** user presses Ctrl+\
- **Then** SIGKILL is sent to Claude immediately

### Input Routing (Observe Mode)

- **Given** observe PTY mode is active (`--observe`)
- **When** user presses any key
- **Then** key is ignored, not forwarded to Claude

### Idle Timeout

- **Given** `idle_timeout_secs: 30`
- **When** neither Claude nor user produces activity for 30 seconds
- **Then** SIGTERM is sent to Claude process

- **Given** interactive mode with `idle_timeout_secs: 30`
- **When** Claude is idle but user presses a key at 29 seconds
- **Then** timeout resets, Claude continues

- **Given** interactive mode with `idle_timeout_secs: 30`
- **When** user is idle but Claude outputs at 29 seconds
- **Then** timeout resets, process continues

- **Given** `idle_timeout_secs: 0`
- **When** both Claude and user are idle
- **Then** no timeout triggers, wait indefinitely for natural exit or Ctrl+C

### Graceful Termination

- **Given** SIGTERM was sent (via timeout or Ctrl+C)
- **When** 5 seconds pass without process exit
- **Then** SIGKILL is sent to force termination

- **Given** Claude exits naturally
- **When** process terminates with any exit code
- **Then** no signals sent, iteration proceeds with accumulated output

### Terminal State

- **Given** Ralph entered raw mode for PTY session
- **When** iteration completes (success or failure)
- **Then** terminal is restored to original state

- **Given** Ralph entered raw mode for PTY session
- **When** Ralph crashes or receives SIGINT
- **Then** terminal is restored to original state (via cleanup handler)

### Fallback Behavior

- **Given** PTY mode is enabled
- **When** Ralph's stdout is not a TTY
- **Then** warning is logged and headless mode is used

- **Given** PTY allocation fails
- **When** iteration starts
- **Then** error is logged and headless mode is used as fallback

### Event Parsing

- **Given** PTY mode produced output with ANSI sequences
- **When** output is parsed for events
- **Then** ANSI sequences are stripped before parsing

- **Given** completion promise appears in PTY output
- **When** ANSI-stripped output is scanned
- **Then** loop terminates successfully

## Validation Plan

### Testing Stack

Based on prior art from Zellij, Alacritty, and ratatui-testlib, use this testing stack:

| Crate | Purpose |
|-------|---------|
| [**vt100**](https://docs.rs/vt100) | Virtual terminal for CI-safe testing (no real TTY needed) |
| [**rexpect**](https://docs.rs/rexpect) | Expect-style pattern matching and input injection |
| [**insta**](https://insta.rs/) | Snapshot testing for terminal output |
| **portable-pty** | Already in spec — PTY spawning |

Add to `ralph-adapters/Cargo.toml`:

```toml
[dev-dependencies]
vt100 = "0.16"
rexpect = "0.5"
insta = { version = "1", features = ["yaml"] }
```

### Automated Tests (CI-safe with vt100)

The `vt100` crate provides a virtual terminal that parses ANSI sequences without a real TTY:

```rust
// tests/pty_validation.rs

use vt100::Parser;

/// Virtual terminal for testing PTY output
struct TestTerminal {
    parser: Parser,
}

impl TestTerminal {
    fn new() -> Self {
        Self { parser: Parser::new(24, 80, 0) }
    }

    fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Get plain text (ANSI stripped) — use for event parsing
    fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    fn contains(&self, text: &str) -> bool {
        self.contents().contains(text)
    }
}
```

| Test | What It Validates | Implementation |
|------|-------------------|----------------|
| **ANSI stripping** | Event parsing finds completion promise in ANSI-laden output | Feed captured PTY output to `vt100::Parser`, assert `contents()` contains `LOOP_COMPLETE` |
| **Double Ctrl+C state machine** | Window timing and state transitions | Unit test `CtrlCState` struct in isolation with mock clock |
| **Config parsing** | YAML fields parse correctly | Unit test with sample config |
| **CLI flag parsing** | Flags override config | Unit test argument parser |
| **Fallback logic** | Non-TTY triggers headless mode | Mock `isatty()` to return false |

### Unit Test: Double Ctrl+C State Machine

```rust
// src/pty_executor.rs (or tests/)

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct MockClock {
        now: Instant,
    }

    impl MockClock {
        fn advance(&mut self, duration: Duration) {
            self.now = self.now + duration;
        }
    }

    #[test]
    fn test_double_ctrl_c_within_window() {
        let mut state = CtrlCState::new();
        let mut clock = MockClock { now: Instant::now() };

        // First Ctrl+C: should forward and start window
        let action = state.handle_ctrl_c(clock.now);
        assert_eq!(action, CtrlCAction::ForwardAndStartWindow);

        // Second Ctrl+C within 1 second: should terminate
        clock.advance(Duration::from_millis(500));
        let action = state.handle_ctrl_c(clock.now);
        assert_eq!(action, CtrlCAction::Terminate);
    }

    #[test]
    fn test_ctrl_c_window_expires() {
        let mut state = CtrlCState::new();
        let mut clock = MockClock { now: Instant::now() };

        // First Ctrl+C
        state.handle_ctrl_c(clock.now);

        // Wait 2 seconds (window expires)
        clock.advance(Duration::from_secs(2));

        // Second Ctrl+C: window expired, should forward and start new window
        let action = state.handle_ctrl_c(clock.now);
        assert_eq!(action, CtrlCAction::ForwardAndStartWindow);
    }
}
```

### Unit Test: ANSI Stripping with vt100

```rust
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
    assert!(!stripped.contains("\x1b["));  // No escape sequences
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
```

### Integration Tests with rexpect

For tests that need real PTY interaction, use `rexpect`:

```rust
// tests/pty_integration.rs

use rexpect::spawn;
use std::time::Duration;

#[test]
#[ignore]  // Run manually or in CI with TTY
fn test_pty_mode_basic() {
    let mut p = spawn("ralph loop --pty --idle-timeout 10", Some(30_000))
        .expect("Failed to spawn ralph");

    // Wait for Claude to start
    p.exp_regex(r"Claude|Thinking|Loading")
        .expect("Claude should show activity");

    // Let it run briefly, then double Ctrl+C
    std::thread::sleep(Duration::from_secs(2));
    p.send_control('c').unwrap();
    std::thread::sleep(Duration::from_millis(100));
    p.send_control('c').unwrap();

    // Should terminate gracefully
    p.exp_eof().expect("Process should exit after double Ctrl+C");
}

#[test]
#[ignore]
fn test_observe_mode_ignores_input() {
    let mut p = spawn("ralph loop --pty --observe --idle-timeout 5", Some(30_000))
        .expect("Failed to spawn ralph");

    // Send some keystrokes
    p.send_line("this should be ignored").unwrap();

    // Should timeout (input ignored), not crash
    p.exp_regex(r"timeout|idle|SIGTERM")
        .expect("Should timeout due to inactivity");
}
```

### Snapshot Testing with insta

```rust
#[test]
fn test_help_output_snapshot() {
    let output = std::process::Command::new("ralph")
        .args(["loop", "--help"])
        .output()
        .expect("Failed to run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("ralph_loop_help", stdout);
}

#[test]
fn test_config_show_snapshot() {
    let output = std::process::Command::new("ralph")
        .args(["config", "show"])
        .output()
        .expect("Failed to run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("ralph_config_show", stdout);
}
```

### Manual Dogfooding Checklist

Run these manually after implementation. Each test should be performed by the implementing agent:

#### Phase 1: Basic PTY (after Phase 4)

```bash
# Test: PTY spawns and output displays
ralph loop --pty --idle-timeout 0
# Expected: Claude's TUI renders with colors, spinners animate
# Validate: Can see tool calls, thinking indicators, progress bars
```

#### Phase 2: Observe Mode (after Phase 4)

```bash
# Test: Input is ignored in observe mode
ralph loop --pty --observe --idle-timeout 60
# Action: Type random keys while Claude works
# Expected: Keys have no effect, Claude doesn't receive them
# Validate: Claude completes without interference
```

#### Phase 3: Input Forwarding (after Phase 7)

```bash
# Test: Input reaches Claude
ralph loop --pty --idle-timeout 0
# Action: When Claude prompts for confirmation, type 'y' + Enter
# Expected: Claude receives input, continues
# Validate: Confirmation dialog is answered
```

#### Phase 4: Double Ctrl+C (after Phase 7)

```bash
# Test: Single Ctrl+C forwards to Claude
ralph loop --pty --idle-timeout 0
# Action: Press Ctrl+C once while Claude is working
# Expected: Claude receives interrupt (may show "Interrupted" or cancel current tool)
# Validate: Claude handles it, Ralph does NOT exit

# Test: Double Ctrl+C stops iteration
ralph loop --pty --idle-timeout 0
# Action: Press Ctrl+C twice quickly (within 1 second)
# Expected: Ralph sends SIGTERM, Claude exits, iteration ends
# Validate: Ralph prints termination message, returns to prompt

# Test: Window expiry
ralph loop --pty --idle-timeout 0
# Action: Press Ctrl+C, wait 2 seconds, press Ctrl+C again
# Expected: Both Ctrl+C are forwarded to Claude (window expired)
# Validate: Ralph does NOT exit after second Ctrl+C
```

#### Phase 5: Ctrl+\ Emergency Escape (after Phase 7)

```bash
# Test: Ctrl+\ immediately kills
ralph loop --pty --idle-timeout 0
# Action: Press Ctrl+\ while Claude is working
# Expected: Immediate SIGKILL, no grace period
# Validate: Claude dies instantly, terminal state restored
```

#### Phase 6: Idle Timeout (after Phase 9)

```bash
# Test: Timeout triggers after inactivity
ralph loop --pty --idle-timeout 5
# Setup: Use a prompt that makes Claude wait (e.g., "wait for my signal")
# Action: Don't type anything for 5 seconds
# Expected: SIGTERM sent after 5 seconds, then SIGKILL if no exit
# Validate: Iteration ends with timeout message

# Test: User input resets timeout
ralph loop --pty --idle-timeout 5
# Action: Every 3 seconds, press a key (e.g., space)
# Expected: Timeout never triggers (keeps resetting)
# Validate: Claude stays alive as long as user is active

# Test: Claude output resets timeout
ralph loop --pty --idle-timeout 5
# Setup: Use a prompt that makes Claude produce output every few seconds
# Expected: Timeout never triggers while Claude is outputting
# Validate: Iteration completes normally
```

#### Phase 7: Terminal State Restoration (after Phase 5)

```bash
# Test: Normal exit restores terminal
ralph loop --pty --idle-timeout 0
# Action: Let Claude complete normally
# Expected: Terminal returns to cooked mode, echo works
# Validate: Type "echo hello" after Ralph exits, see output

# Test: Crash restores terminal (manual crash)
ralph loop --pty --idle-timeout 0
# Action: In another terminal, `kill -9 <ralph_pid>`
# Expected: Terminal should still be usable (cleanup handler ran)
# Validate: Type commands, terminal responds normally
# Note: May need `reset` command if cleanup failed - document as known limitation
```

#### Phase 8: Fallback Behavior (after Phase 11)

```bash
# Test: Non-TTY falls back to headless
ralph loop --pty 2>&1 | cat
# Expected: Warning logged, runs in headless mode
# Validate: No PTY errors, Claude output appears (without TUI formatting)

# Test: PTY allocation failure (hard to trigger)
# Note: May need to mock portable-pty to simulate failure
# Expected: Error logged, falls back to headless
```

### Integration Test Script

Create `tests/pty_integration.sh` that automates some checks:

```bash
#!/bin/bash
set -e

echo "=== PTY Mode Integration Tests ==="

# Test 1: Fallback when piped
echo "Test 1: Fallback to headless when piped"
OUTPUT=$(ralph loop --pty -p "echo hello" 2>&1 | head -1)
if [[ "$OUTPUT" == *"falling back to headless"* ]]; then
    echo "✓ Fallback warning present"
else
    echo "✗ Expected fallback warning"
    exit 1
fi

# Test 2: Config parsing
echo "Test 2: Config parsing"
cat > /tmp/ralph-test.yml << EOF
cli:
  pty_mode: true
  pty_interactive: false
  idle_timeout_secs: 42
EOF
# Verify config loads without error
ralph --config /tmp/ralph-test.yml config show | grep -q "idle_timeout_secs: 42"
echo "✓ Config parsed correctly"

# Test 3: ANSI stripping (using captured sample)
echo "Test 3: ANSI stripping"
# This would use a fixture file with known ANSI sequences
# and verify event parsing extracts the completion promise

echo "=== All automated tests passed ==="
```

### What the Agent Should Report

After implementation, the agent should document:

1. **Which manual tests passed** — Checklist with ✓/✗
2. **Any deviations from spec** — Behavior that differs from what's written
3. **Edge cases discovered** — Scenarios not covered by the spec
4. **Terminal compatibility** — Tested on which terminals (iTerm2, Terminal.app, etc.)

### Known Limitations to Document

- Terminal resize (SIGWINCH) not propagated — document in README
- Raw mode restoration may fail on hard crash — document `reset` command as recovery
- Some terminals may not support all ANSI sequences — note which terminals tested

## Non-Goals

- No TUI replay/recording (use benchmark harness for that)
- No split-pane or multiplexed terminal views
- No SIGWINCH propagation (terminal resize)
- No configurable grace period (hardcoded 5 seconds)
- No configurable reserved keys (Ctrl+C and Ctrl+\ are fixed)

## Implementation Order

1. **Phase 1**: Add `pty_mode`, `pty_interactive`, `idle_timeout_secs` to `CliConfig`
2. **Phase 2**: Add `portable-pty` and `crossterm` dependencies
3. **Phase 3**: Implement `PtyExecutor::spawn()` with PTY creation
4. **Phase 4**: Implement `PtyExecutor::run_observe()` (output-only loop)
5. **Phase 5**: Implement raw mode enter/exit with cleanup handlers
6. **Phase 6**: Implement input routing with reserved key detection
7. **Phase 7**: Implement `PtyExecutor::run_interactive()` with bidirectional I/O
8. **Phase 8**: Implement activity tracking (reset timeout on input OR output)
9. **Phase 9**: Implement termination sequence (SIGTERM → grace → SIGKILL)
10. **Phase 10**: Add CLI flags (`--pty`, `--observe`, `--idle-timeout`)
11. **Phase 11**: Add fallback logic for non-TTY stdout and PTY allocation failure
12. **Phase 12**: Integrate with `EventLoop` execution path
