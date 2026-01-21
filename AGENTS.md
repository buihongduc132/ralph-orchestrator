# AGENTS.md

> The orchestrator is a thin coordination layer, not a platform. Agents are smart; let them do the work.

## The Ralph Tenets

1. **Fresh Context Is Reliability** — Each iteration clears context. Re-read specs, plan, code every cycle. Optimize for the "smart zone" (40-60% of ~176K usable tokens).

2. **Backpressure Over Prescription** — Don't prescribe how; create gates that reject bad work. Tests, typechecks, builds, lints. For subjective criteria, use LLM-as-judge with binary pass/fail.

3. **The Plan Is Disposable** — Regeneration costs one planning loop. Cheap. Never fight to save a plan.

4. **Disk Is State, Git Is Memory** — `.agent/scratchpad.md` is the handoff mechanism. No sophisticated coordination needed.

5. **Steer With Signals, Not Scripts** — The codebase is the instruction manual. When Ralph fails a specific way, add a sign for next time. The prompts you start with won't be the prompts you end with.

6. **Let Ralph Ralph** — Sit *on* the loop, not *in* it. Tune like a guitar, don't conduct like an orchestra.

## Anti-Patterns

- ❌ Building features into the orchestrator that agents can handle
- ❌ Complex retry logic (fresh context handles recovery)
- ❌ Detailed step-by-step instructions (use backpressure instead)
- ❌ Scoping work at task selection time (scope at plan creation instead)
- ❌ Assuming functionality is missing without code verification

## Specs

- Create specs in `specs/` — do NOT implement without an approved spec first
- Work step-by-step: spec → dogfood spec → implement → dogfood implementation → done
- The bar: A new team member should implement using only the spec and codebase

## Tasks

- Create code tasks in `tasks/` using `.code-task.md` extension
- Use `/code-task-generator` to create structured task files from descriptions
- Run tasks with `/code-assist tasks/<task-name>.code-task.md`
- Tasks are self-contained implementation units with acceptance criteria

## Memories

Persistent learning system for accumulated wisdom across sessions. Storage: `.agent/memories.md`.

### Quick Start

```bash
# Add a memory
ralph memory add "uses barrel exports" --type pattern --tags imports,structure

# Search memories
ralph memory search "authentication"
ralph memory search --type fix --tags docker

# List and manage
ralph memory list                    # Show all
ralph memory show mem-1737372000-a1b2
ralph memory delete mem-1737372000-a1b2

# Context injection (used by orchestrator)
ralph memory prime --budget 2000
```

### Memory Types

| Type | Use Case | Emoji |
|------|----------|-------|
| `pattern` | How this codebase does things | 🔄 |
| `decision` | Why something was chosen | ⚖️ |
| `fix` | Solution to recurring problem | 🔧 |
| `context` | Project-specific knowledge | 📍 |

### Configuration

```yaml
# ralph.yml
memories:
  enabled: true
  inject: auto       # auto | manual | none
  budget: 2000       # max tokens (0 = unlimited)
  skill_injection: true
```

### When to Create Memories

- Discovering codebase patterns others should follow
- Making architectural decisions with rationale
- Solving problems that might recur
- Learning project-specific context

## Build & Test

```bash
cargo build
cargo test
```

### Git Hooks Setup

Run this once after cloning to install pre-commit hooks:

```bash
./scripts/setup-hooks.sh
```

The pre-commit hook runs `cargo fmt --check` and `cargo clippy` before each commit, catching CI failures early.

### Smoke Tests (Replay-Based)

Smoke tests use recorded JSONL fixtures instead of live API calls — fast, free, deterministic:

```bash
# Run all smoke tests (Claude + Kiro fixtures)
cargo test -p ralph-core smoke_runner

# Run Kiro-specific smoke tests
cargo test -p ralph-core kiro
```

**Fixtures location:** `crates/ralph-core/tests/fixtures/`
- `basic_session.jsonl` — Claude CLI session
- `kiro/` — Kiro CLI sessions (basic, tool use, autonomous mode)

**IMPORTANT**: You must smoke test after you make code changes.

### Recording New Fixtures

To create a new fixture from a live session:

```bash
# Record a session (outputs JSONL to session.jsonl)
cargo run --bin ralph -- run -c ralph.claude.yml --record-session session.jsonl -p "your prompt"

# Or capture raw CLI output
claude -p "your prompt" 2>&1 | tee output.txt
```

See `crates/ralph-core/tests/fixtures/kiro/README.md` for format details.

## TUI Validation

Use the `/tui-validate` skill to validate Terminal UI rendering. This applies **Tenet #2** (Backpressure Over Prescription) — using LLM-as-judge for subjective visual criteria instead of brittle string matching.

### Quick Start

```bash
# Validate header component from captured output
/tui-validate file:output.txt criteria:ralph-header

# Validate live TUI via tmux
/tui-validate tmux:ralph-session criteria:ralph-full save_screenshot:true

# Custom criteria validation
/tui-validate command:"cargo run --example tui_demo" criteria:"Shows bordered header with iteration count"
```

### Built-in Criteria

| Criteria | Validates |
|----------|-----------|
| `ralph-header` | `[iter N]`, elapsed time `MM:SS`, hat emoji+name, mode indicator |
| `ralph-footer` | Activity indicator (`◉`/`◯`/`■`), event topic, search display |
| `ralph-full` | Complete layout: header + content + footer + visual hierarchy |
| `tui-basic` | Generic: has content, no artifacts, proper dimensions |

### Live TUI Capture Workflow

```bash
# 1. Start TUI in tmux
tmux new-session -d -s ralph-test -x 100 -y 30
tmux send-keys -t ralph-test "cargo run --bin ralph -- run --tui -c ralph.yml -p 'your prompt'" Enter

# 2. Wait for TUI to render
sleep 3

# 3. Capture with freeze
tmux capture-pane -t ralph-test -p -e | freeze --language ansi -o tui-capture.svg

# 4. Validate
/tui-validate file:tui-capture.txt criteria:ralph-header
```

### Prerequisites

```bash
brew install charmbracelet/tap/freeze  # Screenshot tool
brew install tmux                       # For live TUI capture
```

### When to Use

- ✅ After modifying `ralph-tui` widgets
- ✅ Visual regression testing in CI
- ✅ Validating TUI state after specific interactions
- ✅ Creating documentation screenshots

See `.claude/skills/tui-validate/SKILL.md` for full documentation.

## E2E Testing

The `ralph-e2e` crate validates Ralph's behavior against real AI backends. Use this before releases or after significant changes to ensure the orchestration loop works correctly.

### Quick Start

```bash
# Run all tests for Claude backend
cargo run -p ralph-e2e -- claude

# Run all tests for all available backends
cargo run -p ralph-e2e -- all

# List available scenarios
cargo run -p ralph-e2e -- --list

# Fast mode (skip meta-Ralph analysis)
cargo run -p ralph-e2e -- claude --skip-analysis

# Debug mode (keep workspaces)
cargo run -p ralph-e2e -- claude --keep-workspace --verbose
```

### Test Tiers

| Tier | Focus | Scenarios |
|------|-------|-----------|
| 1 | Connectivity | Backend availability and auth |
| 2 | Orchestration Loop | Single/multi iteration, completion |
| 3 | Events | Event parsing, backpressure |
| 4 | Capabilities | Tool use, streaming output |
| 5 | Hat Collections | Hat workflows, event routing |
| 6 | Memory System | Add, search, injection, persistence |
| 7 | Error Handling | Timeout, max iterations, auth failures |

### Reports

Generated in `.e2e-tests/`:

```bash
.e2e-tests/
├── report.md      # Agent-readable Markdown
├── report.json    # Machine-readable JSON
└── claude-connect/  # Test workspace (with --keep-workspace)
```

### When to Use

- ✅ Before releases to validate all backends work
- ✅ After changing core orchestration logic
- ✅ After modifying event parsing or hat routing
- ✅ When adding support for new backends

### E2E Orchestration

When using Ralph to orchestrate E2E test development, use the isolated config to avoid scratchpad pollution:

```bash
# E2E test development (uses .agent/e2e-scratchpad.md)
ralph run -c ralph.e2e.yml -p "specs/e2e-test-fixes.spec.md"

# Running E2E tests themselves (uses isolated workspaces)
cargo run -p ralph-e2e -- claude
```

**Why separate scratchpads?**
- E2E orchestrator: `.agent/e2e-scratchpad.md` (project root)
- E2E test workspaces: `.e2e-tests/<scenario-id>/.agent/scratchpad.md` (isolated)
- Main Ralph: `.agent/scratchpad.md` (project root)

This prevents state pollution between the orchestrator and test workspaces.

See `crates/ralph-e2e/README.md` for detailed documentation.

## Diagnostics

The diagnostics system captures complete visibility into Ralph's operation for debugging and analysis. It's opt-in via environment variable with zero overhead when disabled.

### Enable Diagnostics

```bash
RALPH_DIAGNOSTICS=1 ralph run -p "your prompt"
```

### Output Location

Diagnostics are written to timestamped session directories:

```
.ralph/diagnostics/
└── 2024-01-21T08-45-30/           # ISO 8601 timestamp per session
    ├── agent-output.jsonl          # Agent text, tool calls, tool results
    ├── orchestration.jsonl         # Hat selection, events, backpressure
    ├── trace.jsonl                 # All tracing logs with metadata
    ├── performance.jsonl           # Timing, latency, token counts
    └── errors.jsonl                # Parse errors, validation failures
```

### Reviewing Diagnostics with jq

```bash
# All agent text output
jq 'select(.type == "text")' .ralph/diagnostics/*/agent-output.jsonl

# All tool calls
jq 'select(.type == "tool_call")' .ralph/diagnostics/*/agent-output.jsonl

# Hat selection decisions
jq 'select(.event.type == "hat_selected")' .ralph/diagnostics/*/orchestration.jsonl

# All errors
jq '.' .ralph/diagnostics/*/errors.jsonl

# ERROR level trace logs
jq 'select(.level == "ERROR")' .ralph/diagnostics/*/trace.jsonl
```

### Cleanup

```bash
ralph clean --diagnostics
```

### When to Use

- ✅ Debugging why Ralph selected a particular hat
- ✅ Understanding agent output flow and tool usage
- ✅ Investigating backpressure triggers
- ✅ Analyzing performance bottlenecks
- ✅ Post-mortem on failed orchestration runs

## PR Demos

Use the `/pr-demo` skill to create animated GIF demos for pull requests. This helps reviewers understand new features without reading code.

### Quick Start

```bash
# 1. Script your demo (20-30 seconds, show ONE thing)
# 2. Record with asciinema
asciinema rec demo.cast --cols 100 --rows 24

# 3. Convert to GIF
agg demo.cast demo.gif

# 4. Embed in PR
# ![feature demo](./docs/demos/feature-demo.gif)
```

### Prerequisites

```bash
brew install asciinema
cargo install --git https://github.com/asciinema/agg
```

### When to Use

- ✅ Adding user-facing CLI features
- ✅ Demonstrating new commands like `ralph plan`, `ralph task`
- ✅ Showing workflow improvements

See `.claude/skills/pr-demo/SKILL.md` for full documentation.

## IMPORTANT

- Run `cargo test` before declaring any task done (includes replay smoke tests)
- Backwards compatibility doesn't matter — it adds clutter for no reason
- Prefer replay-based smoke tests over live API calls for CI
- Run python tests, using a .venv
