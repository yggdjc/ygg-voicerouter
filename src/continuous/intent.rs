//! Local rule-based intent classification for continuous listening.

/// Intent classification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// High confidence this is an actionable command.
    Command,
    /// High confidence this is ambient/non-actionable speech.
    Ambient,
    /// Not enough confidence to classify locally — send to LLM.
    Uncertain,
}

/// Rule-based intent filter operating on ASR transcript text.
pub struct IntentFilter {
    triggers: Vec<String>,
}

/// Filler characters that indicate non-actionable speech.
const FILLERS: &[char] = &['嗯', '啊', '哦', '呃', '唔', '额'];

/// Imperative verb prefixes that strongly indicate a command.
const IMPERATIVE_PREFIXES: &[&str] = &[
    "帮我", "打开", "搜索", "关闭", "播放", "切换",
    "停止", "启动", "运行", "执行", "删除", "创建",
    "发送", "查找", "显示",
];

impl IntentFilter {
    /// Create a new filter with pipeline trigger prefixes.
    pub fn new(triggers: &[&str]) -> Self {
        Self {
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Classify transcript text into Command, Ambient, or Uncertain.
    pub fn classify(&self, text: &str) -> Intent {
        let text = text.trim();

        // Rule 1: too short
        if text.chars().count() < 2 {
            return Intent::Ambient;
        }

        // Rule 2: pure filler words
        if text.chars().all(|c| FILLERS.contains(&c)) {
            return Intent::Ambient;
        }

        // Rule 3: matches a pipeline trigger prefix
        for trigger in &self.triggers {
            if text.starts_with(trigger.as_str()) {
                return Intent::Command;
            }
        }

        // Rule 4: imperative verb prefix
        for prefix in IMPERATIVE_PREFIXES {
            if text.starts_with(prefix) {
                return Intent::Command;
            }
        }

        // Rule 5: not enough signal to decide
        Intent::Uncertain
    }
}
