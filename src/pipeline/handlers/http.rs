//! HTTP handler — sync HTTP requests via ureq.

use anyhow::{bail, Result};

use crate::pipeline::handler::{Handler, HandlerResult, RiskLevel};
use crate::pipeline::stage::StageContext;

pub struct HttpHandler;

impl Handler for HttpHandler {
    fn name(&self) -> &str { "http" }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let url = ctx.get("url")
            .ok_or_else(|| anyhow::anyhow!("http handler requires 'url' param"))?;
        let method = ctx.get("method").unwrap_or("POST");

        let url = url.replace("{text}", text);

        let response = match method.to_uppercase().as_str() {
            "GET" => ureq::get(&url).call(),
            "POST" => {
                let body = ctx.get("body")
                    .map(|b| b.replace("{text}", text))
                    .unwrap_or_else(|| text.to_string());
                ureq::post(&url)
                    .set("Content-Type", "application/json")
                    .send_string(&body)
            }
            other => bail!("unsupported HTTP method: {other}"),
        };

        match response {
            Ok(resp) => {
                let body = resp.into_string()?;
                Ok(HandlerResult::Forward(body))
            }
            Err(e) => bail!("HTTP request failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    #[test]
    fn http_handler_name() {
        assert_eq!(HttpHandler.name(), "http");
    }

    #[test]
    fn http_requires_url_param() {
        let ctx = StageContext {
            stage_name: "test".into(),
            params: HashMap::new(),
        };
        assert!(HttpHandler.handle("hello", &ctx).is_err());
    }
}
