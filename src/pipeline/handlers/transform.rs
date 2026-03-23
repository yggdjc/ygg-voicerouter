//! Transform handler — built-in text transformations.

use anyhow::Result;

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct TransformHandler;

impl Handler for TransformHandler {
    fn name(&self) -> &str { "transform" }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        // Template mode: replace {text} in template string.
        if let Some(template) = ctx.get("template") {
            return Ok(HandlerResult::Forward(template.replace("{text}", text)));
        }

        // Regex mode: replace pattern with replacement.
        if let Some(pattern) = ctx.get("regex") {
            let replacement = ctx.get("replacement").unwrap_or("");
            let re = regex_lite::Regex::new(pattern)?;
            let result = re.replace_all(text, replacement).to_string();
            return Ok(HandlerResult::Forward(result));
        }

        // No transform specified — pass through.
        Ok(HandlerResult::Forward(text.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx_with(key: &str, val: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert(key.into(), val.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn template_replaces_text() {
        let ctx = ctx_with("template", "prefix: {text} :suffix");
        let result = TransformHandler.handle("hello", &ctx).unwrap();
        match result {
            HandlerResult::Forward(t) => assert_eq!(t, "prefix: hello :suffix"),
            _ => panic!("expected Forward"),
        }
    }

    #[test]
    fn regex_replaces_pattern() {
        let mut params = HashMap::new();
        params.insert("regex".into(), r"\d+".into());
        params.insert("replacement".into(), "NUM".into());
        let ctx = StageContext { stage_name: "test".into(), params };
        let result = TransformHandler.handle("abc 123 def", &ctx).unwrap();
        match result {
            HandlerResult::Forward(t) => assert_eq!(t, "abc NUM def"),
            _ => panic!("expected Forward"),
        }
    }
}
