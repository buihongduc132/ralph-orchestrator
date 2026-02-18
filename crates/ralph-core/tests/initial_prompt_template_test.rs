//! Integration tests for the initial_prompt_template feature.
//! Tests T10-T15: Custom initial prompt template functionality.

use ralph_core::{EventLoop, RalphConfig};

#[test]
fn test_event_loop_uses_custom_template_when_configured() {
    // T10: EventLoop uses configured template
    let yaml = r#"
event_loop:
  initial_prompt_template: |
    ## CUSTOM ORIENTATION
    You are a specialized assistant using custom template.
    
    ## SCRATCHPAD
    Path: {scratchpad}
    
    ## CUSTOM INSTRUCTIONS
    Follow these custom instructions for this task.
"#;

    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Test context");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("CUSTOM ORIENTATION"),
        "Prompt should reflect custom template content when configured"
    );
    assert!(
        prompt.contains("specialized assistant"),
        "Prompt should include custom template text"
    );
    assert!(
        prompt.contains("CUSTOM INSTRUCTIONS"),
        "Prompt should include custom instructions section"
    );
}

#[test]
fn test_build_prompt_returns_custom_template_content() {
    // T11: build_prompt returns template output
    let yaml = r#"
event_loop:
  initial_prompt_template: |
    # My Custom Prompt Template
    You are a {custom_role}.
    Work on: {objective}
    Use scratchpad: {scratchpad}
    
    ## Guidelines
    - Be thorough
    - Follow best practices
    - Complete one task at a time
"#;

    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Implement login feature");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("My Custom Prompt Template"),
        "build_prompt should return template content"
    );
    assert!(
        prompt.contains("Be thorough"),
        "build_prompt should include template guidelines"
    );
    assert!(
        prompt.contains("Complete one task at a time"),
        "build_prompt should include all template content"
    );
}

#[test]
fn test_custom_template_with_memories_enabled() {
    // T12: Template works with memories on
    let yaml = r#"
core:
  scratchpad: ".ralph/agent/scratchpad.md"
event_loop:
  initial_prompt_template: |
    ## MEMORY-ENABLED TEMPLATE
    You are operating with memories enabled.
    Scratchpad: {scratchpad}
    Memories are available for context.
    
    ## TASK ORIENTATION
    Focus on the current objective.
"#;
    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Test with memories");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("MEMORY-ENABLED TEMPLATE"),
        "Template should work when memories are enabled"
    );
    assert!(
        prompt.contains("memories enabled"),
        "Prompt should reflect memories-enabled template"
    );
}

#[test]
fn test_custom_template_with_memories_disabled() {
    // T13: Template works with memories off
    let yaml = r#"
memories:
  enabled: false
event_loop:
  initial_prompt_template: |
    ## MEMORY-DISABLED TEMPLATE
    You are operating without memories.
    Focus only on the scratchpad: {scratchpad}
    No persistent memory available.
    
    ## INSTRUCTIONS
    Work with current context only.
"#;
    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Test without memories");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("MEMORY-DISABLED TEMPLATE"),
        "Template should work when memories are disabled"
    );
    assert!(
        prompt.contains("current context only"),
        "Prompt should reflect memory-disabled template"
    );
}

#[test]
fn test_custom_template_in_solo_mode() {
    // T14: Template works in solo mode
    let yaml = r#"
event_loop:
  initial_prompt_template: |
    ## SOLO MODE TEMPLATE
    You are operating in solo mode.
    No hats are configured.
    Handle all tasks yourself.
    Scratchpad: {scratchpad}
    
    ## WORKFLOW
    1. Analyze the task
    2. Execute directly
    3. Report completion
"#;
    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Solo mode task");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("SOLO MODE TEMPLATE"),
        "Template should work in solo mode"
    );
    assert!(
        prompt.contains("Handle all tasks yourself"),
        "Prompt should reflect solo mode template"
    );
}

#[test]
fn test_custom_template_in_multi_hat_mode() {
    // T15: Template works in multi-hat mode
    let yaml = r#"
event_loop:
  initial_prompt_template: |
    ## MULTI-HAT TEMPLATE
    You are coordinating multiple hats.
    Use the hat system effectively.
    Scratchpad: {scratchpad}
    
    ## COORDINATION RULES
    - Delegate appropriately
    - Track handoffs
    - Monitor progress
"#;
    let config: RalphConfig = serde_yaml::from_str(yaml).unwrap();
    let event_loop = EventLoop::new(config);

    let prompt = event_loop.build_ralph_prompt("Multi-hat coordination task");

    // This should fail initially because the feature is not implemented yet
    assert!(
        prompt.contains("MULTI-HAT TEMPLATE"),
        "Template should work in multi-hat mode"
    );
    assert!(
        prompt.contains("Delegate appropriately"),
        "Prompt should reflect multi-hat template"
    );
}
