//! Inject handler — forwards text to the focused window.

use anyhow::Result;

use crate::config::InjectMethod;
use crate::inject::inject_text;
use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct InjectHandler {
    method: InjectMethod,
}

impl InjectHandler {
    #[must_use]
    pub fn new(method: InjectMethod) -> Self {
        Self { method }
    }
}

impl Handler for InjectHandler {
    fn name(&self) -> &str {
        "inject"
    }

    fn handle(&self, text: &str, _ctx: &StageContext) -> Result<HandlerResult> {
        inject_text(text, self.method)?;
        Ok(HandlerResult::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_handler_name() {
        let handler = InjectHandler::new(InjectMethod::Auto);
        assert_eq!(handler.name(), "inject");
    }
}
