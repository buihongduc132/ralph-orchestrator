//! Hatless Ralph - the constant coordinator.
//!
//! Ralph is always present, cannot be configured away, and acts as a universal fallback.

use crate::config::CoreConfig;
use crate::hat_registry::HatRegistry;
use ralph_proto::Topic;

/// Hatless Ralph - the constant coordinator.
pub struct HatlessRalph {
    completion_promise: String,
    core: CoreConfig,
    hat_topology: Option<HatTopology>,
}

/// Hat topology for multi-hat mode prompt generation.
pub struct HatTopology {
    hats: Vec<HatInfo>,
}

/// Information about a hat for prompt generation.
pub struct HatInfo {
    pub name: String,
    pub subscribes_to: Vec<String>,
    pub publishes: Vec<String>,
}

impl HatTopology {
    /// Creates topology from registry.
    pub fn from_registry(registry: &HatRegistry) -> Self {
        let hats = registry
            .all()
            .map(|hat| HatInfo {
                name: hat.name.clone(),
                subscribes_to: hat.subscriptions.iter().map(|t| t.as_str().to_string()).collect(),
                publishes: hat.publishes.iter().map(|t| t.as_str().to_string()).collect(),
            })
            .collect();

        Self { hats }
    }
}

impl HatlessRalph {
    /// Creates a new HatlessRalph.
    pub fn new(completion_promise: impl Into<String>, core: CoreConfig, registry: &HatRegistry) -> Self {
        let hat_topology = if registry.is_empty() {
            None
        } else {
            Some(HatTopology::from_registry(registry))
        };

        Self {
            completion_promise: completion_promise.into(),
            core,
            hat_topology,
        }
    }

    /// Builds Ralph's prompt based on context.
    pub fn build_prompt(&self, _context: &str) -> String {
        let mut prompt = self.core_prompt();
        prompt.push_str(&self.workflow_section());

        if let Some(topology) = &self.hat_topology {
            prompt.push_str(&self.hats_section(topology));
        }

        prompt.push_str(&self.event_writing_section());
        prompt.push_str(&self.done_section());

        prompt
    }

    /// Always returns true - Ralph handles all events as fallback.
    pub fn should_handle(&self, _topic: &Topic) -> bool {
        true
    }

    fn core_prompt(&self) -> String {
        let guardrails = self
            .core
            .guardrails
            .iter()
            .enumerate()
            .map(|(i, g)| format!("{}. {g}", 999 + i))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r"I'm Ralph. Fresh context each iteration.

### 0a. ORIENTATION
Study `{specs_dir}` to understand requirements.
Don't assume features aren't implemented—search first.

### 0b. SCRATCHPAD
Study `{scratchpad}`. It's shared state. It's memory.

Task markers:
- `[ ]` pending
- `[x]` done
- `[~]` cancelled (with reason)

### GUARDRAILS
{guardrails}

",
            scratchpad = self.core.scratchpad,
            specs_dir = self.core.specs_dir,
            guardrails = guardrails,
        )
    }

    fn workflow_section(&self) -> String {
        format!(
            r"## WORKFLOW

### 1. GAP ANALYSIS
Compare specs against codebase. Use parallel subagents (up to 10) for searches.

### 2. PLAN
Update `{scratchpad}` with prioritized tasks.

### 3. IMPLEMENT
Pick ONE task. Only 1 subagent for build/tests.

### 4. COMMIT
Capture the why, not just the what. Mark `[x]` in scratchpad.

### 5. REPEAT
Until all tasks `[x]` or `[~]`.

",
            scratchpad = self.core.scratchpad
        )
    }

    fn hats_section(&self, topology: &HatTopology) -> String {
        let mut section = String::from("## HATS\n\nDelegate via events.\n\n");

        // Build hat table
        section.push_str("| Hat | Triggers On | Publishes |\n");
        section.push_str("|-----|-------------|----------|\n");

        for hat in &topology.hats {
            let subscribes = hat.subscribes_to.join(", ");
            let publishes = hat.publishes.join(", ");
            section.push_str(&format!("| {} | {} | {} |\n", hat.name, subscribes, publishes));
        }

        section.push('\n');
        section
    }

    fn event_writing_section(&self) -> String {
        format!(
            r#"## EVENT WRITING

Write events to `{events_file}` as:
{{"topic": "build.task", "payload": "...", "ts": "2026-01-14T12:00:00Z"}}

"#,
            events_file = ".agent/events.jsonl"
        )
    }

    fn done_section(&self) -> String {
        format!(
            r"## DONE

Output {} when all tasks complete.
",
            self.completion_promise
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RalphConfig;

    #[test]
    fn test_prompt_without_hats() {
        let config = RalphConfig::default();
        let registry = HatRegistry::new(); // Empty registry
        let ralph = HatlessRalph::new("LOOP_COMPLETE", config.core.clone(), &registry);

        let prompt = ralph.build_prompt("");

        // Identity with ghuntley style
        assert!(prompt.contains("I'm Ralph. Fresh context each iteration."));

        // Numbered orientation phases
        assert!(prompt.contains("### 0a. ORIENTATION"));
        assert!(prompt.contains("Study"));
        assert!(prompt.contains("Don't assume features aren't implemented"));

        // Scratchpad section with task markers
        assert!(prompt.contains("### 0b. SCRATCHPAD"));
        assert!(prompt.contains("Task markers:"));
        assert!(prompt.contains("- `[ ]` pending"));
        assert!(prompt.contains("- `[x]` done"));
        assert!(prompt.contains("- `[~]` cancelled"));

        // Workflow with numbered steps
        assert!(prompt.contains("## WORKFLOW"));
        assert!(prompt.contains("### 1. GAP ANALYSIS"));
        assert!(prompt.contains("Use parallel subagents (up to 10)"));
        assert!(prompt.contains("### 2. PLAN"));
        assert!(prompt.contains("### 3. IMPLEMENT"));
        assert!(prompt.contains("Only 1 subagent for build/tests"));
        assert!(prompt.contains("### 4. COMMIT"));
        assert!(prompt.contains("Capture the why"));
        assert!(prompt.contains("### 5. REPEAT"));

        // Should NOT have hats section when no hats
        assert!(!prompt.contains("## HATS"));

        // Event writing and completion
        assert!(prompt.contains("## EVENT WRITING"));
        assert!(prompt.contains(".agent/events.jsonl"));
        assert!(prompt.contains("LOOP_COMPLETE"));
    }

    #[test]
    fn test_prompt_with_hats() {
        let yaml = r#"
hats:
  planner:
    name: "Planner"
    triggers: ["task.start", "build.done", "build.blocked"]
    publishes: ["build.task"]
  builder:
    name: "Builder"
    triggers: ["build.task"]
    publishes: ["build.done", "build.blocked"]
"#;
        let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
        let registry = HatRegistry::from_config(&config);
        let ralph = HatlessRalph::new("LOOP_COMPLETE", config.core.clone(), &registry);

        let prompt = ralph.build_prompt("");

        // Identity with ghuntley style
        assert!(prompt.contains("I'm Ralph. Fresh context each iteration."));

        // Orientation phases
        assert!(prompt.contains("### 0a. ORIENTATION"));
        assert!(prompt.contains("### 0b. SCRATCHPAD"));

        // Workflow is always present
        assert!(prompt.contains("## WORKFLOW"));
        assert!(prompt.contains("### 1. GAP ANALYSIS"));

        // Hats section when hats are defined
        assert!(prompt.contains("## HATS"));
        assert!(prompt.contains("Delegate via events"));
        assert!(prompt.contains("| Hat | Triggers On | Publishes |"));

        // Event writing and completion
        assert!(prompt.contains("## EVENT WRITING"));
        assert!(prompt.contains("LOOP_COMPLETE"));
    }

    #[test]
    fn test_should_handle_always_true() {
        let config = RalphConfig::default();
        let registry = HatRegistry::new();
        let ralph = HatlessRalph::new("LOOP_COMPLETE", config.core.clone(), &registry);

        assert!(ralph.should_handle(&Topic::new("any.topic")));
        assert!(ralph.should_handle(&Topic::new("build.task")));
        assert!(ralph.should_handle(&Topic::new("unknown.event")));
    }

    #[test]
    fn test_ghuntley_patterns_present() {
        let config = RalphConfig::default();
        let registry = HatRegistry::new();
        let ralph = HatlessRalph::new("LOOP_COMPLETE", config.core.clone(), &registry);

        let prompt = ralph.build_prompt("");

        // Key ghuntley language patterns
        assert!(prompt.contains("Study"), "Should use 'study' verb");
        assert!(
            prompt.contains("Don't assume features aren't implemented"),
            "Should have 'don't assume' guardrail"
        );
        assert!(
            prompt.contains("parallel subagents"),
            "Should mention parallel subagents for reads"
        );
        assert!(
            prompt.contains("Only 1 subagent"),
            "Should limit to 1 subagent for builds"
        );
        assert!(
            prompt.contains("Capture the why"),
            "Should emphasize 'why' in commits"
        );

        // Numbered guardrails (999+)
        assert!(prompt.contains("### GUARDRAILS"), "Should have guardrails section");
        assert!(prompt.contains("999."), "Guardrails should use high numbers");
    }

    #[test]
    fn test_scratchpad_format_documented() {
        let config = RalphConfig::default();
        let registry = HatRegistry::new();
        let ralph = HatlessRalph::new("LOOP_COMPLETE", config.core.clone(), &registry);

        let prompt = ralph.build_prompt("");

        // Task marker format is documented
        assert!(prompt.contains("- `[ ]` pending"));
        assert!(prompt.contains("- `[x]` done"));
        assert!(prompt.contains("- `[~]` cancelled (with reason)"));
    }
}
