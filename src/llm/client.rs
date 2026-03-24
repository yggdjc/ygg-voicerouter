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

/// OpenAI-compatible chat completion client.
pub struct LlmClient {
    endpoint: String,
    model: String,
    api_key: String,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
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

impl LlmClient {
    /// Create a new client from config. Reads API key from the environment variable specified in config.
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env)
            .with_context(|| format!("missing env var: {}", config.api_key_env))?;
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
        let response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_string(&body)
            .context("LLM API request failed")?;
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
}
