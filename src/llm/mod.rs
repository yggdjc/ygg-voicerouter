//! OpenAI-compatible LLM client for intent classification.

mod client;

pub use client::{LlmClient, LlmResponse, build_system_prompt, parse_llm_response};
