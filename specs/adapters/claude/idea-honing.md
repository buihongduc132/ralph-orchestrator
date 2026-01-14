# Requirements Clarification

This document captures the Q&A process for refining the Claude adapter streaming output feature.

---

## Q1: What should users see when running `ralph run -P PROMPT.md`?

Currently, users see nothing until Claude completes. What output would be most valuable during execution?

**Answer:** Two-tier output verbosity:
- **Default mode:** Assistant text and tool invocations
- **Verbose mode:** Everything (assistant text, tool invocations, tool results, progress indicators, usage stats)

---

## Q2: How should the output be formatted?

For non-interactive terminal output, we have several formatting choices:

**Answer:** Plain text streaming format:
```
Claude: I'll start by reading the file...
[Tool] Read: src/main.rs
Claude: Now I'll make the changes...
[Tool] Edit: src/main.rs
```

---

## Q3: How should verbose mode be enabled?

**Answer:** Introduce verbosity flag if it doesn't exist. Use idiomatic precedence:
1. CLI flag (`--verbose` / `-v`) — highest priority
2. Environment variable (`RALPH_VERBOSE`)
3. Config file (`verbose: true`) — lowest priority

---

## Q4: How should errors and failures be displayed?

