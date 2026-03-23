//! Speak handler — emit SpeakRequest to trigger TTS playback.

use anyhow::Result;

use crate::actor::{Message, SpeakSource};
use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct SpeakHandler;

impl Handler for SpeakHandler {
    fn name(&self) -> &str {
        "speak"
    }

    fn handle(&self, text: &str, _ctx: &StageContext) -> Result<HandlerResult> {
        Ok(HandlerResult::Emit(Message::SpeakRequest {
            text: text.to_string(),
            source: SpeakSource::SystemFeedback,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn speak_emits_speak_request() {
        let ctx = StageContext {
            stage_name: "test".into(),
            params: HashMap::new(),
        };
        let result = SpeakHandler.handle("你好世界", &ctx).unwrap();
        match result {
            HandlerResult::Emit(Message::SpeakRequest { text, .. }) => {
                assert_eq!(text, "你好世界");
            }
            _ => panic!("expected Emit(SpeakRequest)"),
        }
    }
}
