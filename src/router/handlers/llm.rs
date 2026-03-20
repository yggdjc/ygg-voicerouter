//! LLM handler — sends text to an OpenAI-compatible API and logs the response.
//!
//! Configuration is read from environment variables at construction time:
//!
//! | Variable       | Default                          |
//! |----------------|----------------------------------|
//! | `LLM_BASE_URL` | `https://api.openai.com/v1`      |
//! | `LLM_MODEL`    | `gpt-4o-mini`                    |
//! | `LLM_API_KEY`  | *(required — empty string used)* |
//!
//! When the inject module (Task 5) is available, replace the `log::info!` call
//! in `handle` with `inject::linux::inject_text(&response, method)`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::router::handler::Handler;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Sends the transcribed text as a user message to an OpenAI-compatible chat
/// completion endpoint and logs the model response.
///
/// # Examples
///
/// ```no_run
/// use voicerouter::router::handler::Handler;
/// use voicerouter::router::handlers::llm::LlmHandler;
///
/// // Reads LLM_BASE_URL, LLM_MODEL, LLM_API_KEY from environment.
/// let handler = LlmHandler::from_env();
/// assert_eq!(handler.name(), "llm");
/// ```
pub struct LlmHandler {
    base_url: String,
    model: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl LlmHandler {
    /// Construct from environment variables, falling back to defaults.
    ///
    /// - `LLM_BASE_URL` — API base URL (default: `https://api.openai.com/v1`)
    /// - `LLM_MODEL`    — model identifier (default: `gpt-4o-mini`)
    /// - `LLM_API_KEY`  — bearer token (default: empty string)
    #[must_use]
    pub fn from_env() -> Self {
        let base_url = std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned());
        let model =
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
        Self {
            base_url,
            model,
            api_key,
            client: reqwest::blocking::Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

impl Handler for LlmHandler {
    fn name(&self) -> &str {
        "llm"
    }

    fn handle(&self, text: &str) -> Result<()> {
        log::debug!("[llm] sending to model '{}': {:?}", self.model, text);

        let body = ChatRequest {
            model: &self.model,
            messages: vec![Message { role: "user", content: text }],
        };

        let response = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("LLM HTTP request failed")?
            .error_for_status()
            .context("LLM API returned error status")?
            .json::<ChatResponse>()
            .context("LLM response JSON parse failed")?;

        let content = response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        // STUB: inject module not yet implemented (Task 5).
        // Replace with inject::linux::inject_text(&content, method).
        log::info!("[llm stub] would inject LLM response: {:?}", content);
        Ok(())
    }
}
