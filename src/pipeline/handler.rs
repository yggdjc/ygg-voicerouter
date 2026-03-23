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

/// A pipeline stage handler.
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult>;
}
