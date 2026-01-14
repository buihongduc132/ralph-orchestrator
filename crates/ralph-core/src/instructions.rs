//! Instruction builder for Ralph agent prompts.
//!
//! Philosophy: One agent, multiple hats. Ralph switches hats, not personalities:
//! - Planner hat: Plans work, owns scratchpad, validates completion
//! - Builder hat: Implements tasks, runs backpressure, commits
//!
//! This maps directly to ghuntley's PROMPT_plan.md / PROMPT_build.md split.

use crate::config::{CoreConfig, EventMetadata};
use ralph_proto::Hat;
use std::collections::HashMap;

/// Builds the prepended instructions for agent prompts.
///
/// One agent, two hats: Planner and Builder. Both are Ralph wearing different hats.
/// The orchestrator routes events to trigger hat changes.
///
/// Per spec: "Core behaviors are always present—hats add to them, never replace."
/// The builder injects core behaviors (scratchpad, specs, guardrails) into every prompt.
#[derive(Debug)]
pub struct InstructionBuilder {
    completion_promise: String,
    core: CoreConfig,
    /// Event metadata for deriving instructions from pub/sub contracts.
    events: HashMap<String, EventMetadata>,
}

impl InstructionBuilder {
    /// Creates a new instruction builder with core configuration.
    ///
    /// The core config provides paths and guardrails that are injected
    /// into every prompt, per the spec's "Core Behaviors" requirement.
    pub fn new(completion_promise: impl Into<String>, core: CoreConfig) -> Self {
        Self {
            completion_promise: completion_promise.into(),
            core,
            events: HashMap::new(),
        }
    }

    /// Creates a new instruction builder with event metadata for custom hats.
    pub fn with_events(
        completion_promise: impl Into<String>,
        core: CoreConfig,
        events: HashMap<String, EventMetadata>,
    ) -> Self {
        Self {
            completion_promise: completion_promise.into(),
            core,
            events,
        }
    }

    /// Builds the core behaviors section injected into all prompts.
    ///
    /// Per spec: "Every Ralph invocation includes these behaviors, regardless of which hat is active."
    fn build_core_behaviors(&self) -> String {
        let guardrails = self
            .core
            .guardrails
            .iter()
            .map(|g| format!("- {g}"))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r"## CORE BEHAVIORS
**Scratchpad:** `{scratchpad}` is shared state. Read it. Update it.
**Specs:** `{specs_dir}` is the source of truth. Implementations must match.

**IMPORTANT**: Less is more, do the smallest, atomic task possible. Leave work for future workers.

### Guardrails
{guardrails}
",
            scratchpad = self.core.scratchpad,
            specs_dir = self.core.specs_dir,
            guardrails = guardrails,
        )
    }

    /// Builds Planner instructions (Ralph with planner hat).
    ///
    /// Planner owns the scratchpad and decides what work needs doing.
    /// It does NOT implement—that's the builder hat's job.
    pub fn build_coordinator(&self, prompt_content: &str) -> String {
        let core_behaviors = self.build_core_behaviors();

        format!(
            r#"You are Ralph. You've got your planner hat on.

{core_behaviors}

## PLANNER MODE

You're planning, not building.

1. **Gap analysis.** Compare `{specs_dir}` against codebase. What's missing? Broken?

2. **Own the scratchpad.** Create or update `{scratchpad}` with prioritized tasks.
   - `[ ]` pending
   - `[x]` done
   - `[~]` cancelled (with reason)

3. **Dispatch work.** Publish `<event topic="build.task">` ONE AT A TIME for the highest priority task. Clear acceptance criteria.

4. **Validate.** When build reports done, verify it satisfies the spec.

## DON'T

- ❌ Write implementation code
- ❌ Run tests or make commits
- ❌ Pick tasks to implement yourself
- ❌ Output {promise} until you've created tasks AND they've all been completed

## DONE

**Prerequisites** (ALL must be true before outputting {promise}):
1. Scratchpad exists with at least one task
2. You dispatched work to the builder at least once
3. ALL tasks are marked `[x]` (done) or `[~]` (cancelled)
4. Specs are satisfied

Only when ALL prerequisites are met, output: {promise}

---
{prompt}"#,
            core_behaviors = core_behaviors,
            specs_dir = self.core.specs_dir,
            scratchpad = self.core.scratchpad,
            promise = self.completion_promise,
            prompt = prompt_content
        )
    }

    /// Builds Builder instructions (Ralph with builder hat).
    ///
    /// Builder implements tasks. It does NOT plan or manage the scratchpad.
    pub fn build_ralph(&self, prompt_content: &str) -> String {
        let core_behaviors = self.build_core_behaviors();

        format!(
            r#"You are Ralph. You've got your builder hat on.

{core_behaviors}

## BUILDER MODE

You're building, not planning. One task, then exit.

1. **Pick ONE task.** Highest priority `[ ]` from `{scratchpad}`.
   - **Known Issues are HIGH PRIORITY.** Check `ISSUES.md` in the repo root. Fixing known issues takes precedence. Once fixed, remove the issue from `ISSUES.md`.

2. **Implement.** Write the code. Follow existing patterns.

3. **Validate.** Run backpressure. Must pass.

4. **Commit.** One task, one commit. Mark `[x]` in scratchpad.

5. **Exit.** Publish `<event topic="build.done">` with evidence:
   ```
   <event topic="build.done">
   tests: pass
   lint: pass
   typecheck: pass
   </event>
   ```
   All three checks must show "pass" or the event will be rejected.

## DON'T

- ❌ Create the scratchpad (planner does that)
- ❌ Decide what tasks to add (planner does that)
- ❌ Output the completion promise (planner does that)

## STUCK?

Can't finish? Publish `<event topic="build.blocked">` with:
- What you tried
- Why it failed
- What would unblock you

---
{prompt}"#,
            core_behaviors = core_behaviors,
            scratchpad = self.core.scratchpad,
            prompt = prompt_content
        )
    }

    /// Derives instructions from a hat's pub/sub contract and event metadata.
    ///
    /// For each event the hat triggers on or publishes:
    /// 1. Check event metadata for on_trigger/on_publish instructions
    /// 2. Fall back to built-in defaults for well-known events
    ///
    /// This allows users to define custom events with custom behaviors,
    /// while still getting sensible defaults for standard events.
    fn derive_instructions_from_contract(&self, hat: &Hat) -> String {
        let mut behaviors: Vec<String> = Vec::new();

        // Derive behaviors from triggers (what this hat responds to)
        for trigger in &hat.subscriptions {
            let trigger_str = trigger.as_str();

            // First, check event metadata
            if let Some(meta) = self.events.get(trigger_str) {
                if !meta.on_trigger.is_empty() {
                    behaviors.push(format!("**On `{}`:** {}", trigger_str, meta.on_trigger));
                    continue;
                }
            }

            // Fall back to built-in defaults for well-known events
            let default_behavior = match trigger_str {
                "task.start" | "task.resume" => Some("Analyze the task and create a plan in the scratchpad."),
                "build.done" => Some("Review the completed work and decide next steps."),
                "build.blocked" => Some("Analyze the blocker and decide how to unblock (simplify task, gather info, or escalate)."),
                "build.task" => Some("Implement the assigned task. Follow existing patterns. Run backpressure (tests/checks). Commit when done."),
                "review.request" => Some("Review the recent changes for correctness, tests, patterns, errors, and security."),
                "review.approved" => Some("Mark the task complete `[x]` and proceed to next task."),
                "review.changes_requested" => Some("Add fix tasks to scratchpad and dispatch."),
                _ => None,
            };

            if let Some(behavior) = default_behavior {
                behaviors.push(format!("**On `{}`:** {}", trigger_str, behavior));
            }
        }

        // Derive behaviors from publishes (what this hat outputs)
        for publish in &hat.publishes {
            let publish_str = publish.as_str();

            // First, check event metadata
            if let Some(meta) = self.events.get(publish_str) {
                if !meta.on_publish.is_empty() {
                    behaviors.push(format!("**Publish `{}`:** {}", publish_str, meta.on_publish));
                    continue;
                }
            }

            // Fall back to built-in defaults for well-known events
            let default_behavior = match publish_str {
                "build.task" => Some("Dispatch ONE AT A TIME for pending `[ ]` tasks."),
                "build.done" => Some("When implementation is finished and tests pass."),
                "build.blocked" => Some("When stuck - include what you tried and why it failed."),
                "review.request" => Some("After build completion, before marking done."),
                "review.approved" => Some("If changes look good and meet requirements."),
                "review.changes_requested" => Some("If issues found - include specific feedback."),
                _ => None,
            };

            if let Some(behavior) = default_behavior {
                behaviors.push(format!("**Publish `{}`:** {}", publish_str, behavior));
            }
        }

        // Add must-publish rule if hat has publishable events
        if !hat.publishes.is_empty() {
            let topics: Vec<&str> = hat.publishes.iter().map(|t| t.as_str()).collect();
            behaviors.push(format!(
                "**IMPORTANT:** Every iteration MUST publish one of: `{}` or the loop will terminate.",
                topics.join("`, `")
            ));
        }

        if behaviors.is_empty() {
            "Follow the incoming event instructions.".to_string()
        } else {
            format!("### Derived Behaviors\n\n{}", behaviors.join("\n\n"))
        }
    }

    /// Builds custom hat instructions for extended multi-agent configurations.
    ///
    /// Use this for teams beyond the default planner + builder hats.
    /// When instructions are empty, derives them from the pub/sub contract.
    pub fn build_custom_hat(&self, hat: &Hat, events_context: &str) -> String {
        let core_behaviors = self.build_core_behaviors();

        let role_instructions = if hat.instructions.is_empty() {
            self.derive_instructions_from_contract(hat)
        } else {
            hat.instructions.clone()
        };

        let (publish_topics, must_publish) = if hat.publishes.is_empty() {
            (String::new(), String::new())
        } else {
            let topics: Vec<&str> = hat.publishes.iter().map(|t| t.as_str()).collect();
            let topics_list = topics.join(", ");
            let topics_backticked = format!("`{}`", topics.join("`, `"));

            (
                format!("You publish to: {}", topics_list),
                format!(
                    "\n\n**You MUST publish one of these events based on your task results:** {}\nFailure to publish will terminate the loop.",
                    topics_backticked
                ),
            )
        };

        format!(
            r#"You are {name}. Fresh context each iteration.

{core_behaviors}

## YOUR ROLE

{role_instructions}

## THE RULES

1. **One task, then exit.** The loop continues.

## EVENTS

Communicate via: `<event topic="...">message</event>`
{publish_topics}{must_publish}

## COMPLETION

Only Coordinator outputs: {promise}

---
INCOMING:
{events}"#,
            name = hat.name,
            core_behaviors = core_behaviors,
            role_instructions = role_instructions,
            publish_topics = publish_topics,
            must_publish = must_publish,
            promise = self.completion_promise,
            events = events_context,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_builder(promise: &str) -> InstructionBuilder {
        InstructionBuilder::new(promise, CoreConfig::default())
    }

    #[test]
    fn test_planner_hat_plans_not_implements() {
        let builder = default_builder("LOOP_COMPLETE");
        let instructions = builder.build_coordinator("Build a CLI tool");

        // Identity: Ralph with planner hat
        assert!(instructions.contains("You are Ralph"));
        assert!(instructions.contains("planner hat"));
        assert!(instructions.contains("Build a CLI tool"));

        // Planner mode header per spec
        assert!(instructions.contains("## PLANNER MODE"));
        assert!(instructions.contains("planning, not building"));

        // Planner's job (per spec lines 236-263)
        assert!(instructions.contains("Gap analysis"));
        assert!(instructions.contains("Own the scratchpad"));
        assert!(instructions.contains("Dispatch work")); // dispatches build.task events
        assert!(instructions.contains("build.task")); // publishes build.task
        assert!(instructions.contains("ONE AT A TIME")); // per spec
        assert!(instructions.contains("Validate")); // validates completion
        assert!(instructions.contains("./specs/"));

        // Task markers per spec
        assert!(instructions.contains("[ ]")); // pending
        assert!(instructions.contains("[x]")); // done
        assert!(instructions.contains("[~]")); // cancelled

        // Completion promise (Planner outputs it)
        assert!(instructions.contains("LOOP_COMPLETE"));

        // What Planner doesn't do
        assert!(instructions.contains("❌ Write implementation code"));
        assert!(instructions.contains("❌ Run tests or make commits"));
        assert!(instructions.contains("❌ Pick tasks to implement yourself"));

        // Guards against premature completion (vacuous truth prevention)
        assert!(instructions.contains("Prerequisites"));
        assert!(instructions.contains("Scratchpad exists with at least one task"));
        assert!(instructions.contains("dispatched work to the builder at least once"));
    }

    #[test]
    fn test_builder_hat_implements_not_plans() {
        let builder = default_builder("LOOP_COMPLETE");
        let instructions = builder.build_ralph("Build a CLI tool");

        // Identity: Ralph with builder hat
        assert!(instructions.contains("You are Ralph"));
        assert!(instructions.contains("builder hat"));
        assert!(instructions.contains("Build a CLI tool"));

        // Builder mode header per spec
        assert!(instructions.contains("## BUILDER MODE"));
        assert!(instructions.contains("building, not planning"));

        // Builder's job (per spec lines 266-294)
        assert!(instructions.contains("Pick ONE task"));
        assert!(instructions.contains("Known Issues are HIGH PRIORITY")); // fixes known issues first
        assert!(instructions.contains("ISSUES.md")); // centralized issue tracking
        assert!(instructions.contains("remove the issue from")); // cleans up after fix
        assert!(instructions.contains("Implement"));
        assert!(instructions.contains("Validate")); // step 3 per spec
        assert!(instructions.contains("backpressure")); // must run backpressure
        assert!(instructions.contains("Commit")); // step 4
        assert!(instructions.contains("Exit")); // step 5
        assert!(instructions.contains("build.done")); // publishes build.done

        // What Builder doesn't do - references planner, not Coordinator
        assert!(instructions.contains("❌ Create the scratchpad (planner does that)"));
        assert!(instructions.contains("❌ Decide what tasks to add (planner does that)"));
        assert!(instructions.contains("❌ Output the completion promise (planner does that)"));

        // STUCK section for build.blocked events
        assert!(instructions.contains("## STUCK?"));
        assert!(instructions.contains("build.blocked"));

        // Should NOT contain completion promise in output
        assert!(!instructions.contains("LOOP_COMPLETE"));
    }

    #[test]
    fn test_coordinator_and_ralph_share_guardrails() {
        let builder = default_builder("DONE");
        let coordinator = builder.build_coordinator("test");
        let ralph = builder.build_ralph("test");

        // Both reference the scratchpad (from CoreConfig)
        assert!(coordinator.contains(".agent/scratchpad.md"));
        assert!(ralph.contains(".agent/scratchpad.md"));

        // Both include default guardrails
        assert!(coordinator.contains("search first"));
        assert!(ralph.contains("search first"));
        assert!(coordinator.contains("Backpressure"));
        assert!(ralph.contains("Backpressure"));

        // Both use task markers
        assert!(coordinator.contains("[x]"));
        assert!(ralph.contains("[x]"));
        assert!(coordinator.contains("[~]"));
    }

    #[test]
    fn test_separation_of_concerns() {
        let builder = default_builder("DONE");
        let planner = builder.build_coordinator("test");
        let builder_hat = builder.build_ralph("test");

        // Planner does planning, not implementation
        assert!(planner.contains("Gap analysis"));
        assert!(planner.contains("PLANNER MODE"));
        assert!(!planner.contains("BUILDER MODE"));

        // Builder does implementation, not planning
        assert!(builder_hat.contains("BUILDER MODE"));
        assert!(!builder_hat.contains("Gap analysis"));

        // Only Planner outputs completion promise
        assert!(planner.contains("output: DONE"));
        assert!(!builder_hat.contains("output: DONE"));
    }

    #[test]
    fn test_custom_hat_for_extended_teams() {
        let builder = default_builder("DONE");
        let hat = Hat::new("reviewer", "Code Reviewer")
            .with_instructions("Review PRs for quality and correctness.");

        let instructions = builder.build_custom_hat(&hat, "PR #123 ready for review");

        // Custom role
        assert!(instructions.contains("Code Reviewer"));
        assert!(instructions.contains("Review PRs for quality"));

        // Events
        assert!(instructions.contains("PR #123 ready for review"));
        assert!(instructions.contains("<event topic="));

        // Core behaviors are injected
        assert!(instructions.contains("CORE BEHAVIORS"));
        assert!(instructions.contains(".agent/scratchpad.md"));
    }

    #[test]
    fn test_custom_guardrails_injected() {
        let custom_core = CoreConfig {
            scratchpad: ".workspace/plan.md".to_string(),
            specs_dir: "./specifications/".to_string(),
            guardrails: vec![
                "Custom rule one".to_string(),
                "Custom rule two".to_string(),
            ],
        };
        let builder = InstructionBuilder::new("DONE", custom_core);

        let coordinator = builder.build_coordinator("test");
        let ralph = builder.build_ralph("test");

        // Custom scratchpad path is used
        assert!(coordinator.contains(".workspace/plan.md"));
        assert!(ralph.contains(".workspace/plan.md"));

        // Custom specs dir is used
        assert!(coordinator.contains("./specifications/"));

        // Custom guardrails are injected
        assert!(coordinator.contains("Custom rule one"));
        assert!(coordinator.contains("Custom rule two"));
        assert!(ralph.contains("Custom rule one"));
        assert!(ralph.contains("Custom rule two"));
    }

    #[test]
    fn test_must_publish_injected_for_explicit_instructions() {
        use ralph_proto::Topic;

        let builder = default_builder("DONE");
        let hat = Hat::new("reviewer", "Code Reviewer")
            .with_instructions("Review PRs for quality and correctness.")
            .with_publishes(vec![
                Topic::new("review.approved"),
                Topic::new("review.changes_requested"),
            ]);

        let instructions = builder.build_custom_hat(&hat, "PR #123 ready");

        // Must-publish rule should be injected even with explicit instructions
        assert!(
            instructions.contains("You MUST publish one of these events"),
            "Must-publish rule should be injected for custom hats with publishes"
        );
        assert!(instructions.contains("`review.approved`"));
        assert!(instructions.contains("`review.changes_requested`"));
        assert!(instructions.contains("Failure to publish will terminate the loop"));
    }

    #[test]
    fn test_must_publish_not_injected_when_no_publishes() {
        let builder = default_builder("DONE");
        let hat = Hat::new("observer", "Silent Observer")
            .with_instructions("Observe and log, but do not emit events.");

        let instructions = builder.build_custom_hat(&hat, "Observe this");

        // No must-publish rule when hat has no publishes
        assert!(
            !instructions.contains("You MUST publish"),
            "Must-publish rule should NOT be injected when hat has no publishes"
        );
    }
}
