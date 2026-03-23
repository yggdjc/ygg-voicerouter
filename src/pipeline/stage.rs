//! Pipeline stage types: Stage, Condition, StageContext.

use std::collections::HashMap;
use std::time::Duration;

use super::handler::Handler;

pub struct Stage {
    pub name: String,
    pub handler: Box<dyn Handler>,
    pub condition: Option<Condition>,
    pub after: Option<String>,
    pub params: HashMap<String, String>,
    pub timeout: Duration,
}

impl Stage {
    pub fn to_context(&self) -> StageContext {
        StageContext {
            stage_name: self.name.clone(),
            params: self.params.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Condition {
    Always,
    StartsWith(String),
    OutputEq(String),
    OutputContains(String),
}

impl Condition {
    #[must_use]
    pub fn matches_text(&self, text: &str) -> bool {
        match self {
            Self::Always => true,
            Self::StartsWith(prefix) => text.starts_with(prefix.as_str()),
            Self::OutputEq(_) | Self::OutputContains(_) => false,
        }
    }

    #[must_use]
    pub fn matches_with_results(&self, text: &str, results: &HashMap<String, String>) -> bool {
        match self {
            Self::Always => true,
            Self::StartsWith(prefix) => text.starts_with(prefix.as_str()),
            Self::OutputEq(expected) => results.values().any(|v| v.trim() == expected.as_str()),
            Self::OutputContains(substring) => results.values().any(|v| v.contains(substring.as_str())),
        }
    }

    #[must_use]
    pub fn strip_prefix<'a>(&self, text: &'a str) -> Option<&'a str> {
        match self {
            Self::StartsWith(prefix) => text.strip_prefix(prefix.as_str()).map(|s| s.trim()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StageContext {
    pub stage_name: String,
    pub params: HashMap<String, String>,
}

impl StageContext {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_starts_with_matches() {
        let cond = Condition::StartsWith("搜索".into());
        assert!(cond.matches_text("搜索什么东西"));
        assert!(!cond.matches_text("其他内容"));
    }

    #[test]
    fn condition_starts_with_strip_prefix() {
        let cond = Condition::StartsWith("搜索".into());
        assert_eq!(cond.strip_prefix("搜索什么东西"), Some("什么东西"));
        assert_eq!(cond.strip_prefix("其他内容"), None);
    }

    #[test]
    fn condition_always_matches_everything() {
        let cond = Condition::Always;
        assert!(cond.matches_text("anything"));
        assert_eq!(cond.strip_prefix("anything"), None);
    }

    #[test]
    fn stage_context_from_params() {
        let mut params = HashMap::new();
        params.insert("command".into(), "echo {text}".into());
        let ctx = StageContext { stage_name: "test".into(), params };
        assert_eq!(ctx.get("command"), Some("echo {text}"));
        assert_eq!(ctx.get("missing"), None);
    }
}
