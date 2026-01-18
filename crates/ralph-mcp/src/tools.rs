//! MCP tool parameter definitions for Ralph orchestration.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the ralph_run tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunParams {
    /// The prompt/task description to run
    #[schemars(description = "The prompt or task description to execute")]
    pub prompt: String,
    /// Optional path to config file (defaults to ralph.yml)
    #[schemars(description = "Path to Ralph config file (defaults to ralph.yml)")]
    #[serde(default)]
    pub config: Option<String>,
    /// Optional working directory
    #[schemars(description = "Working directory for the session")]
    #[serde(default)]
    pub working_dir: Option<String>,
}

/// Parameters for the ralph_status tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusParams {
    /// Session ID to check status for
    #[schemars(description = "Session ID to check status for")]
    pub session_id: String,
}

/// Parameters for the ralph_stop tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopParams {
    /// Session ID to stop
    #[schemars(description = "Session ID to stop")]
    pub session_id: String,
}

/// Parameters for the ralph_list_hats tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListHatsParams {
    /// Optional path to config file (defaults to ralph.yml)
    #[schemars(description = "Path to Ralph config file (defaults to ralph.yml)")]
    #[serde(default)]
    pub config: Option<String>,
}
