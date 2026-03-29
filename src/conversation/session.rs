use std::time::Instant;

use crate::llm::ChatMessage;

/// Multi-turn conversation session with history and timeout tracking.
/// Uses `crate::llm::ChatMessage` directly to avoid type duplication.
pub struct Session {
    history: Vec<ChatMessage>,
    system_prompt: String,
    pub created_at: Instant,
    pub last_activity: Instant,
    end_phrases: Vec<String>,
}

impl Session {
    pub fn new(system_prompt: String, end_phrases: Vec<String>) -> Self {
        let now = Instant::now();
        Self {
            history: Vec::new(),
            system_prompt,
            created_at: now,
            last_activity: now,
            end_phrases,
        }
    }

    fn add_message(&mut self, role: &str, content: &str) {
        self.history.push(ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
        self.last_activity = Instant::now();
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.add_message("user", content);
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.add_message("assistant", content);
    }

    pub fn messages(&self) -> Vec<ChatMessage> {
        let mut msgs = vec![ChatMessage {
            role: "system".into(),
            content: self.system_prompt.clone(),
        }];
        msgs.extend(self.history.clone());
        msgs
    }

    pub fn is_end_phrase(&self, text: &str) -> bool {
        // Strip punctuation for fuzzy matching — ASR may output "结束。" or "再见！"
        let cleaned: String = text.trim().chars()
            .filter(|c| !matches!(c, '。' | '！' | '？' | '，' | '.' | '!' | '?' | ',' | ' '))
            .collect();
        self.end_phrases.iter().any(|p| cleaned == *p || cleaned.contains(p.as_str()))
    }

    pub fn is_timed_out(&self, timeout_secs: f64) -> bool {
        self.last_activity.elapsed().as_secs_f64() >= timeout_secs
    }

    pub fn turn_count(&self) -> usize {
        self.history.iter().filter(|m| m.role == "user").count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_session_has_empty_history() {
        let s = Session::new("system".into(), vec!["结束".into()]);
        assert_eq!(s.turn_count(), 0);
        let msgs = s.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "system");
    }

    #[test]
    fn add_messages_builds_history() {
        let mut s = Session::new("system".into(), vec![]);
        s.add_user_message("hello");
        s.add_assistant_message("hi there");
        assert_eq!(s.turn_count(), 1);
        let msgs = s.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn end_phrase_matching() {
        let s = Session::new("sys".into(), vec!["结束".into(), "再见".into()]);
        assert!(s.is_end_phrase("结束"));
        assert!(s.is_end_phrase("再见"));
        assert!(!s.is_end_phrase("继续"));
    }

    #[test]
    fn end_phrase_with_punctuation() {
        let s = Session::new("sys".into(), vec!["结束".into(), "再见".into()]);
        assert!(s.is_end_phrase("结束。"));
        assert!(s.is_end_phrase("再见！"));
        assert!(s.is_end_phrase("再见."));
    }

    #[test]
    fn end_phrase_contains() {
        let s = Session::new("sys".into(), vec!["再见".into()]);
        assert!(s.is_end_phrase("好的再见"));
    }

    #[test]
    fn end_phrase_trimmed() {
        let s = Session::new("sys".into(), vec!["结束".into()]);
        assert!(s.is_end_phrase(" 结束 "));
    }

    #[test]
    fn timeout_check() {
        let mut s = Session::new("sys".into(), vec![]);
        s.last_activity = Instant::now() - Duration::from_secs(60);
        assert!(s.is_timed_out(30.0));
        assert!(!s.is_timed_out(120.0));
    }

    #[test]
    fn activity_resets_on_message() {
        let mut s = Session::new("sys".into(), vec![]);
        s.last_activity = Instant::now() - Duration::from_secs(60);
        s.add_user_message("new input");
        assert!(!s.is_timed_out(30.0));
    }
}
