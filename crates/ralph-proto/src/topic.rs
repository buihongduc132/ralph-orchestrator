//! Topic types for event routing.
//!
//! Topics are routing keys used to match events to subscribers.
//! Supports glob-style patterns like `impl.*` to match `impl.done`.

use serde::{Deserialize, Serialize};

/// A topic for event routing.
///
/// Topics can be either concrete (e.g., `impl.done`) or patterns (e.g., `impl.*`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Topic(String);

impl Topic {
    /// Creates a new topic from a string.
    pub fn new(topic: impl Into<String>) -> Self {
        Self(topic.into())
    }

    /// Returns the topic as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns true if this is a global wildcard (`*`) that matches everything.
    ///
    /// Used for fallback routing - global wildcards have lower priority than
    /// specific subscriptions.
    pub fn is_global_wildcard(&self) -> bool {
        self.0 == "*"
    }

    /// Checks if this topic pattern matches a given topic.
    ///
    /// Pattern rules:
    /// - `*` matches any single segment (e.g., `impl.*` matches `impl.done`)
    /// - Exact match for non-pattern topics
    /// - A single `*` matches everything
    pub fn matches(&self, topic: &Topic) -> bool {
        let pattern = &self.0;
        let target = &topic.0;

        // Single wildcard matches everything
        if pattern == "*" {
            return true;
        }

        // Exact match
        if pattern == target {
            return true;
        }

        // Glob pattern matching
        let pattern_parts: Vec<&str> = pattern.split('.').collect();
        let target_parts: Vec<&str> = target.split('.').collect();

        if pattern_parts.len() != target_parts.len() {
            return false;
        }

        pattern_parts
            .iter()
            .zip(target_parts.iter())
            .all(|(p, t)| *p == "*" || p == t)
    }
}

impl From<&str> for Topic {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Topic {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let pattern = Topic::new("impl.done");
        let target = Topic::new("impl.done");
        assert!(pattern.matches(&target));
    }

    #[test]
    fn test_no_match() {
        let pattern = Topic::new("impl.done");
        let target = Topic::new("review.done");
        assert!(!pattern.matches(&target));
    }

    #[test]
    fn test_wildcard_suffix() {
        let pattern = Topic::new("impl.*");
        assert!(pattern.matches(&Topic::new("impl.done")));
        assert!(pattern.matches(&Topic::new("impl.started")));
        assert!(!pattern.matches(&Topic::new("review.done")));
    }

    #[test]
    fn test_wildcard_prefix() {
        let pattern = Topic::new("*.done");
        assert!(pattern.matches(&Topic::new("impl.done")));
        assert!(pattern.matches(&Topic::new("review.done")));
        assert!(!pattern.matches(&Topic::new("impl.started")));
    }

    #[test]
    fn test_global_wildcard() {
        let pattern = Topic::new("*");
        assert!(pattern.matches(&Topic::new("impl.done")));
        assert!(pattern.matches(&Topic::new("anything")));
    }

    #[test]
    fn test_length_mismatch() {
        let pattern = Topic::new("impl.*");
        assert!(!pattern.matches(&Topic::new("impl.sub.done")));
    }
}
