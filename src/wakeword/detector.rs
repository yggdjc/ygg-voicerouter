//! Wake word detection via ASR output prefix matching.

/// Detects configured wake phrases in ASR transcript text.
pub struct WakewordDetector {
    phrases: Vec<String>,
}

impl WakewordDetector {
    #[must_use]
    pub fn new(phrases: Vec<String>) -> Self {
        Self { phrases }
    }

    /// Check if text starts with any wake phrase.
    /// Returns (matched_phrase, remainder) or None.
    pub fn check<'a>(&self, text: &'a str) -> Option<(&str, &'a str)> {
        for phrase in &self.phrases {
            if text.starts_with(phrase.as_str()) {
                let remainder = text[phrase.len()..].trim_start();
                return Some((phrase.as_str(), remainder));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_matches_phrase() {
        let detector = WakewordDetector::new(
            vec!["小助手".into(), "hey router".into()],
        );
        assert_eq!(
            detector.check("小助手帮我搜索"),
            Some(("小助手", "帮我搜索"))
        );
        assert_eq!(
            detector.check("hey router do something"),
            Some(("hey router", "do something"))
        );
        assert_eq!(detector.check("random text"), None);
    }

    #[test]
    fn detector_returns_empty_remainder() {
        let detector = WakewordDetector::new(vec!["小助手".into()]);
        assert_eq!(detector.check("小助手"), Some(("小助手", "")));
    }

    #[test]
    fn detector_empty_phrases_never_matches() {
        let detector = WakewordDetector::new(Vec::new());
        assert_eq!(detector.check("anything"), None);
    }
}
