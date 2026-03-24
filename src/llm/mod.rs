//! OpenAI-compatible LLM client for intent classification.

mod client;

pub use client::{
    LlmClient, LlmResponse, ChatMessage, ConversationResponse,
    build_system_prompt, parse_llm_response, parse_chat_json,
};
