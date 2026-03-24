//! Integration tests for conversation mode.

use crossbeam::channel;
use voicerouter::actor::{Bus, Message};
use voicerouter::conversation::sentence::split_sentences;
use voicerouter::conversation::session::Session;
use voicerouter::llm::parse_chat_json;

#[test]
fn session_full_lifecycle() {
    let mut s = Session::new(
        "system prompt".into(),
        vec!["结束".into(), "再见".into()],
    );

    s.add_user_message("你好");
    s.add_assistant_message("你好！有什么可以帮你的？");
    assert_eq!(s.turn_count(), 1);

    s.add_user_message("今天天气怎么样");
    s.add_assistant_message("今天晴天。");
    assert_eq!(s.turn_count(), 2);

    let msgs = s.messages();
    assert_eq!(msgs.len(), 5); // system + 4 messages
    assert_eq!(msgs[0].role, "system");

    assert!(s.is_end_phrase("结束"));
    assert!(!s.is_end_phrase("继续聊"));
}

#[test]
fn sentence_splitter_with_llm_output() {
    let reply = "今天天气很好。最高温度25度，适合出行！你还想知道什么？";
    let sentences = split_sentences(reply);
    assert_eq!(sentences.len(), 3);
}

#[test]
fn parse_ollama_json_response() {
    let json = r#"{"reply": "今天晴天，最高25度。", "confidence": 0.85}"#;
    let resp = parse_chat_json(json).unwrap();
    assert_eq!(resp.reply, "今天晴天，最高25度。");
    assert!((resp.confidence - 0.85).abs() < f64::EPSILON);
}

#[test]
fn parse_malformed_ollama_response() {
    assert!(parse_chat_json(r#"{"reply": "hi""#).is_err());
}

#[test]
fn conversation_messages_roundtrip() {
    let (tx, rx) = channel::bounded(8);
    let mut bus = Bus::new();
    bus.subscribe("StartConversation", tx);
    bus.publish(Message::StartConversation {
        wakeword: Some("小助手".into()),
    });
    let msg = rx.try_recv().unwrap();
    assert!(matches!(
        msg,
        Message::StartConversation { wakeword: Some(w) } if w == "小助手"
    ));
}

#[test]
fn end_conversation_message_routes() {
    let (tx, rx) = channel::bounded(8);
    let mut bus = Bus::new();
    bus.subscribe("EndConversation", tx);
    bus.publish(Message::EndConversation);
    let msg = rx.try_recv().unwrap();
    assert!(matches!(msg, Message::EndConversation));
}
