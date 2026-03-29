//! Overlay communication protocol — shared message types.

use serde::Deserialize;

/// Overlay state sent from daemon.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "state")]
pub enum OverlayMsg {
    #[serde(rename = "recording")]
    Recording {
        #[serde(default)]
        level: u8,
    },
    #[serde(rename = "transcribing")]
    Transcribing,
    #[serde(rename = "result")]
    Result { text: String },
    #[serde(rename = "thinking")]
    Thinking,
    #[serde(rename = "idle")]
    Idle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recording() {
        let msg: OverlayMsg = serde_json::from_str(r#"{"state":"recording","level":2}"#).unwrap();
        assert!(matches!(msg, OverlayMsg::Recording { level: 2 }));
    }

    #[test]
    fn parse_result() {
        let msg: OverlayMsg = serde_json::from_str(r#"{"state":"result","text":"hello"}"#).unwrap();
        if let OverlayMsg::Result { text } = msg {
            assert_eq!(text, "hello");
        } else {
            panic!("expected Result");
        }
    }

    #[test]
    fn parse_idle() {
        let msg: OverlayMsg = serde_json::from_str(r#"{"state":"idle"}"#).unwrap();
        assert!(matches!(msg, OverlayMsg::Idle));
    }

    #[test]
    fn parse_thinking() {
        let msg: OverlayMsg = serde_json::from_str(r#"{"state":"thinking"}"#).unwrap();
        assert!(matches!(msg, OverlayMsg::Thinking));
    }

    #[test]
    fn parse_transcribing() {
        let msg: OverlayMsg = serde_json::from_str(r#"{"state":"transcribing"}"#).unwrap();
        assert!(matches!(msg, OverlayMsg::Transcribing));
    }
}
