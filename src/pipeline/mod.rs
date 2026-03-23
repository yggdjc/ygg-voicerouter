//! Composable handler pipeline.

pub mod dag;
pub mod handler;
pub mod handlers;
pub mod stage;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use stage::Stage;

// ---- PipelineActor ----

pub struct PipelineActor {
    stages: Vec<Stage>,
}

impl PipelineActor {
    #[must_use]
    pub fn new(stages: Vec<Stage>) -> Self {
        Self { stages }
    }
}

impl Actor for PipelineActor {
    fn name(&self) -> &str {
        "pipeline"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let has_dag = self.stages.iter().any(|s| s.after.is_some());
        let mode = if has_dag { "dag" } else { "linear" };
        log::info!("[pipeline] ready with {} stages (mode: {mode})", self.stages.len());

        for msg in inbox {
            match msg {
                Message::Shutdown => break,
                Message::Transcript { ref text, .. }
                | Message::PipelineInput { ref text, .. } => {
                    if has_dag {
                        dag::execute_dag(&self.stages, text, &outbox);
                    } else {
                        execute_pipeline(&self.stages, text, &outbox);
                    }
                }
                _ => {}
            }
        }

        log::info!("[pipeline] stopped");
    }
}

// ---- Pipeline execution ----

pub fn execute_pipeline(
    stages: &[Stage],
    text: &str,
    outbox: &Sender<Message>,
) {
    let mut current_text = text.to_string();

    for stage in stages {
        if let Some(ref cond) = stage.condition {
            if !cond.matches_text(&current_text) {
                continue;
            }
        }

        let payload = stage.condition.as_ref()
            .and_then(|c| c.strip_prefix(&current_text))
            .unwrap_or(&current_text);

        let ctx = stage.to_context();
        match stage.handler.handle(payload, &ctx) {
            Ok(handler::HandlerResult::Forward(text)) => current_text = text,
            Ok(handler::HandlerResult::Emit(msg)) => {
                outbox.send(msg).ok();
            }
            Ok(handler::HandlerResult::ForwardAndEmit(text, msg)) => {
                current_text = text;
                outbox.send(msg).ok();
            }
            Ok(handler::HandlerResult::Done) => break,
            Err(e) => {
                log::error!("[pipeline] stage '{}' failed: {e:#}", stage.name);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::Message;
    use crate::pipeline::handler::{Handler, HandlerResult};
    use crate::pipeline::stage::{Condition, Stage, StageContext};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct RecordingHandler {
        received: Arc<Mutex<Vec<String>>>,
    }

    impl Handler for RecordingHandler {
        fn name(&self) -> &str { "recording" }
        fn handle(&self, text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
            self.received.lock().unwrap().push(text.to_string());
            Ok(HandlerResult::Forward(text.to_string()))
        }
    }

    struct UpperHandler;

    impl Handler for UpperHandler {
        fn name(&self) -> &str { "upper" }
        fn handle(&self, text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
            Ok(HandlerResult::Forward(text.to_uppercase()))
        }
    }

    fn make_stage(name: &str, handler: Box<dyn Handler>, cond: Option<Condition>) -> Stage {
        Stage {
            name: name.into(),
            handler,
            condition: cond,
            after: None,
            params: HashMap::new(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn pipeline_single_stage() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![make_stage("s1", Box::new(RecordingHandler {
            received: Arc::clone(&received),
        }), None)];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["hello"]);
    }

    #[test]
    fn pipeline_chain_transforms_text() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("upper", Box::new(UpperHandler), None),
            make_stage("record", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), None),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["HELLO"]);
    }

    #[test]
    fn pipeline_condition_skips_non_matching() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("conditional", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), Some(Condition::StartsWith("搜索".into()))),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "其他内容", &tx);
        assert!(received.lock().unwrap().is_empty());
    }

    #[test]
    fn pipeline_condition_strips_prefix() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("conditional", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), Some(Condition::StartsWith("搜索".into()))),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "搜索什么东西", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["什么东西"]);
    }

    #[test]
    fn pipeline_emit_sends_to_outbox() {
        struct EmitHandler;
        impl Handler for EmitHandler {
            fn name(&self) -> &str { "emit" }
            fn handle(&self, _text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
                Ok(HandlerResult::Emit(Message::SpeakRequest {
                    text: "spoken".into(),
                    source: crate::actor::SpeakSource::SystemFeedback,
                }))
            }
        }
        let stages = vec![make_stage("emit", Box::new(EmitHandler), None)];
        let (tx, rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::SpeakRequest { text, .. } if text == "spoken"));
    }

    #[test]
    fn dag_pipeline_fan_out() {
        use crate::pipeline::dag::execute_dag;

        struct ClassifyHandler;
        impl Handler for ClassifyHandler {
            fn name(&self) -> &str { "classify" }
            fn handle(
                &self,
                _text: &str,
                _ctx: &StageContext,
            ) -> anyhow::Result<HandlerResult> {
                Ok(HandlerResult::Forward("note".into()))
            }
        }

        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            Stage {
                name: "classify".into(),
                handler: Box::new(ClassifyHandler),
                condition: None,
                params: HashMap::new(),
                timeout: Duration::from_secs(10),
                after: None,
            },
            Stage {
                name: "note_handler".into(),
                handler: Box::new(RecordingHandler {
                    received: Arc::clone(&received),
                }),
                condition: Some(Condition::OutputEq("note".into())),
                params: HashMap::new(),
                timeout: Duration::from_secs(10),
                after: Some("classify".into()),
            },
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_dag(&stages, "test input", &tx);
        let texts = received.lock().unwrap();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0], "test input");
    }

    #[test]
    fn integration_bus_routes_transcript_to_pipeline() {
        use crate::actor::Bus;

        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![make_stage("record", Box::new(RecordingHandler {
            received: Arc::clone(&received),
        }), None)];

        let (pipeline_tx, pipeline_rx) = crossbeam::channel::bounded(8);
        let (bus_out_tx, _bus_out_rx) = crossbeam::channel::bounded(8);

        let mut bus = Bus::new();
        bus.subscribe("Transcript", pipeline_tx);

        bus.publish(Message::Transcript {
            text: "测试消息".into(),
            raw: "测试消息".into(),
        });

        let msg = pipeline_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        if let Message::Transcript { text, .. } = msg {
            execute_pipeline(&stages, &text, &bus_out_tx);
        }

        assert_eq!(*received.lock().unwrap(), vec!["测试消息"]);
    }
}
