//! HTTP client for OpenAI-compatible chat completion API.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;

#[derive(Debug, Deserialize)]
pub struct LlmResponse {
    pub intent: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub text: String,
}

/// Parse an LLM JSON response string into LlmResponse.
pub fn parse_llm_response(json: &str) -> Result<LlmResponse> {
    serde_json::from_str(json).context("failed to parse LLM response JSON")
}

/// Parse a conversation JSON response string into ConversationResponse.
pub fn parse_chat_json(json: &str) -> Result<ConversationResponse> {
    let mut resp: ConversationResponse =
        serde_json::from_str(json).context("failed to parse conversation JSON")?;
    resp.confidence = resp.confidence.clamp(0.0, 1.0);
    Ok(resp)
}

/// Build the system prompt for intent classification.
pub fn build_system_prompt(available_actions: &[String]) -> String {
    let actions_list = if available_actions.is_empty() {
        "none configured".to_string()
    } else {
        available_actions.join(", ")
    };
    format!(
        "You classify voice transcripts as commands or ambient speech.\n\
         Available actions: {actions_list}\n\n\
         Respond with JSON only: {{\"intent\": \"command\"|\"ambient\", \
         \"action\": \"<matching action or empty>\", \"text\": \"<processed text>\"}}\n\n\
         If the transcript is clearly a command directed at an assistant, respond with intent=command.\n\
         If it's casual conversation, self-talk, or ambient noise, respond with intent=ambient."
    )
}

/// A single message in a chat conversation.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Response from a conversation turn.
#[derive(Debug, Deserialize)]
pub struct ConversationResponse {
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize)]
struct ConversationChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    fmt_type: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: String,
}

/// OpenAI-compatible chat completion client.
pub struct LlmClient {
    endpoint: String,
    model: String,
    api_key: String,
}

impl LlmClient {
    /// Create a new client from config. If api_key_env is empty, no Authorization header is sent.
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let api_key = if config.api_key_env.is_empty() {
            String::new()
        } else {
            std::env::var(&config.api_key_env)
                .with_context(|| format!("missing env var: {}", config.api_key_env))?
        };
        Ok(Self {
            endpoint: config.endpoint.clone(),
            model: config.model.clone(),
            api_key,
        })
    }

    /// Classify a transcript using the LLM.
    pub fn classify(
        &self,
        transcript: &str,
        available_actions: &[String],
    ) -> Result<LlmResponse> {
        let system_prompt = build_system_prompt(available_actions);
        let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage { role: "system".into(), content: system_prompt },
                ChatMessage { role: "user".into(), content: transcript.to_string() },
            ],
            temperature: 0.0,
        };
        let body = serde_json::to_string(&request).context("failed to serialize chat request")?;

        let mut req = ureq::post(&url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5));
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }

        let response = req.send_string(&body).context("LLM API request failed")?;
        let response_text = response.into_string().context("failed to read LLM API response body")?;
        let chat_resp: ChatResponse =
            serde_json::from_str(&response_text).context("failed to parse LLM API response")?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("");
        parse_llm_response(content)
    }

    /// Send a multi-turn conversation to the LLM and receive a structured reply.
    pub fn chat(
        &self,
        messages: &[ChatMessage],
        timeout_secs: u64,
    ) -> Result<ConversationResponse> {
        let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
        let request = ConversationChatRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            stream: false,
            response_format: ResponseFormat { fmt_type: "json_object".into() },
        };
        let body = serde_json::to_string(&request)
            .context("failed to serialize conversation request")?;

        let mut req = ureq::post(&url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(timeout_secs));
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }

        let response = req.send_string(&body).context("conversation API request failed")?;
        let response_text = response
            .into_string()
            .context("failed to read conversation API response body")?;
        let chat_resp: ChatResponse =
            serde_json::from_str(&response_text).context("failed to parse API response")?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("{}");
        parse_chat_json(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmConfig;

    #[test]
    fn parse_chat_response_valid() {
        let json = r#"{"reply": "你好", "confidence": 0.9}"#;
        let resp = parse_chat_json(json).unwrap();
        assert_eq!(resp.reply, "你好");
        assert!((resp.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_chat_response_missing_confidence() {
        let json = r#"{"reply": "你好"}"#;
        let resp = parse_chat_json(json).unwrap();
        assert_eq!(resp.confidence, 0.0);
    }

    #[test]
    fn parse_chat_response_invalid_json() {
        let result = parse_chat_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn clamp_confidence_high() {
        let json = r#"{"reply": "ok", "confidence": 1.5}"#;
        let resp = parse_chat_json(json).unwrap();
        assert!((resp.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clamp_confidence_negative() {
        let json = r#"{"reply": "ok", "confidence": -0.5}"#;
        let resp = parse_chat_json(json).unwrap();
        assert!((resp.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn chat_message_is_pub() {
        let msg = ChatMessage { role: "user".into(), content: "hi".into() };
        assert_eq!(msg.role, "user");
    }

    #[test]
    fn llm_client_no_api_key_succeeds() {
        let config = LlmConfig {
            endpoint: "http://localhost:11434/v1".into(),
            model: "test".into(),
            api_key_env: String::new(),
        };
        let client = LlmClient::new(&config);
        assert!(client.is_ok());
    }
}
