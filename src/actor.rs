//! Actor infrastructure: Message types, Actor trait, and Bus.

use std::collections::HashMap;
use std::time::Instant;

use crossbeam::channel::Sender;

// ---- Message ----

#[derive(Clone, Debug)]
pub enum Message {
    Transcript { text: String, raw: String },
    PipelineInput { text: String, metadata: Metadata },
    PipelineOutput { text: String, stage: String },
    SpeakRequest { text: String, source: SpeakSource },
    SpeakDone,
    MuteInput,
    UnmuteInput,
    StartListening { wakeword: Option<String> },
    StopListening,
    /// Cancel active recording without transcribing (discard audio).
    CancelRecording,
    Shutdown,
    /// Request user confirmation for high-risk action (continuous mode).
    ConfirmAction { text: String, stage: String },
    /// User confirmed a pending high-risk action.
    ActionConfirmed,
    /// User rejected or timeout on a pending high-risk action.
    ActionRejected,
}

impl Message {
    #[must_use]
    pub fn topic(&self) -> &'static str {
        match self {
            Self::Transcript { .. } => "Transcript",
            Self::PipelineInput { .. } => "PipelineInput",
            Self::PipelineOutput { .. } => "PipelineOutput",
            Self::SpeakRequest { .. } => "SpeakRequest",
            Self::SpeakDone => "SpeakDone",
            Self::MuteInput => "MuteInput",
            Self::UnmuteInput => "UnmuteInput",
            Self::StartListening { .. } => "StartListening",
            Self::StopListening => "StopListening",
            Self::CancelRecording => "CancelRecording",
            Self::Shutdown => "Shutdown",
            Self::ConfirmAction { .. } => "ConfirmAction",
            Self::ActionConfirmed => "ActionConfirmed",
            Self::ActionRejected => "ActionRejected",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub source: String,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub enum SpeakSource {
    LlmReply,
    SystemFeedback,
}

// ---- Actor trait ----

pub trait Actor: Send + 'static {
    fn name(&self) -> &str;
    fn run(self, inbox: crossbeam::channel::Receiver<Message>, outbox: Sender<Message>);
}

// ---- Bus ----

pub struct Bus {
    subscriptions: HashMap<&'static str, Vec<Sender<Message>>>,
}

impl Bus {
    #[must_use]
    pub fn new() -> Self {
        Self { subscriptions: HashMap::new() }
    }

    pub fn subscribe(&mut self, topic: &'static str, sender: Sender<Message>) {
        self.subscriptions.entry(topic).or_default().push(sender);
    }

    pub fn publish(&self, msg: Message) {
        let topic = msg.topic();
        if let Some(subs) = self.subscriptions.get(topic) {
            for sender in subs {
                if let Err(e) = sender.send(msg.clone()) {
                    if matches!(msg, Message::Shutdown) {
                        log::warn!("failed to deliver Shutdown: {e}");
                    }
                }
            }
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_clone_preserves_data() {
        let msg = Message::Transcript {
            text: "hello".into(),
            raw: "hello".into(),
        };
        let cloned = msg.clone();
        assert!(matches!(cloned, Message::Transcript { text, .. } if text == "hello"));
    }

    #[test]
    fn message_topic_returns_variant_name() {
        assert_eq!(Message::StartListening { wakeword: None }.topic(), "StartListening");
        assert_eq!(Message::Shutdown.topic(), "Shutdown");
        let msg = Message::Transcript { text: "x".into(), raw: "x".into() };
        assert_eq!(msg.topic(), "Transcript");
    }

    #[test]
    fn bus_routes_to_subscribers() {
        let (tx, rx) = crossbeam::channel::bounded(8);
        let mut bus = Bus::new();
        bus.subscribe("StartListening", tx);
        bus.publish(Message::StartListening { wakeword: None });
        let received = rx.try_recv().unwrap();
        assert!(matches!(received, Message::StartListening { .. }));
    }

    #[test]
    fn bus_fan_out_to_multiple_subscribers() {
        let (tx1, rx1) = crossbeam::channel::bounded(8);
        let (tx2, rx2) = crossbeam::channel::bounded(8);
        let mut bus = Bus::new();
        bus.subscribe("Shutdown", tx1);
        bus.subscribe("Shutdown", tx2);
        bus.publish(Message::Shutdown);
        assert!(matches!(rx1.try_recv().unwrap(), Message::Shutdown));
        assert!(matches!(rx2.try_recv().unwrap(), Message::Shutdown));
    }

    #[test]
    fn bus_no_subscriber_is_silent() {
        let bus = Bus::new();
        bus.publish(Message::StartListening { wakeword: None });
    }

    #[test]
    fn continuous_message_topics() {
        assert_eq!(
            Message::ConfirmAction { text: "x".into(), stage: "y".into() }.topic(),
            "ConfirmAction"
        );
        assert_eq!(Message::ActionConfirmed.topic(), "ActionConfirmed");
        assert_eq!(Message::ActionRejected.topic(), "ActionRejected");
    }
}
