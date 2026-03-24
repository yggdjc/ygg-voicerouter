//! Handler trait for pipeline stages.

use anyhow::Result;

use super::stage::StageContext;
use crate::actor::Message;

/// Result of a handler execution.
#[derive(Debug)]
pub enum HandlerResult {
    Forward(String),
    Emit(Message),
    ForwardAndEmit(String, Message),
    Done,
}

/// Risk level for continuous listening mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    /// Safe to execute silently (inject, speak, transform).
    Low,
    /// Requires user confirmation (shell, http, pipe).
    High,
}

/// A pipeline stage handler.
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult>;

    /// Risk level for continuous listening confirmation.
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low // default: low risk
    }
}
