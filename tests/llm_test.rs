//! Tests for LLM response parsing.

use voicerouter::llm::{LlmResponse, parse_llm_response};

#[test]
fn parse_command_response() {
    let json = r#"{"intent":"command","action":"搜索","text":"Rust VAD"}"#;
    let resp = parse_llm_response(json).unwrap();
    assert_eq!(resp.intent, "command");
    assert_eq!(resp.action, "搜索");
    assert_eq!(resp.text, "Rust VAD");
}

#[test]
fn parse_ambient_response() {
    let json = r#"{"intent":"ambient","action":"","text":""}"#;
    let resp = parse_llm_response(json).unwrap();
    assert_eq!(resp.intent, "ambient");
}

#[test]
fn parse_invalid_json_returns_error() {
    let result = parse_llm_response("not json");
    assert!(result.is_err());
}

#[test]
fn build_system_prompt_includes_actions() {
    use voicerouter::llm::build_system_prompt;
    let actions = vec!["搜索".to_string(), "echo".to_string()];
    let prompt = build_system_prompt(&actions);
    assert!(prompt.contains("搜索"));
    assert!(prompt.contains("echo"));
}
