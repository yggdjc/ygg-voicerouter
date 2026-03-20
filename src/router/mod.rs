//! Voice router — match transcript prefix and dispatch to the right handler.
//!
//! Rules are evaluated in declaration order; the first matching trigger wins.
//! Unmatched text is forwarded to the default handler (inject).
//!
//! # Examples
//!
//! ```no_run
//! use voicerouter::config::Config;
//! use voicerouter::router::Router;
//!
//! let config = Config::default();
//! let router = Router::new(&config);
//! // No rules → default inject handler forwards text to the focused window.
//! router.dispatch("hello world").unwrap();
//! ```

pub mod handler;
pub mod handlers;

use anyhow::Result;
use log::warn;

use crate::config::Config;
use handler::Handler;
use handlers::{inject::InjectHandler, llm::LlmHandler, shell::ShellHandler};

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct Rule {
    trigger: String,
    handler: Box<dyn Handler>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Prefix-based text router.
///
/// Dispatch rules are built from [`Config`] at construction time and are
/// immutable thereafter, making `Router` cheaply shareable across threads with
/// `Arc<Router>`.
pub struct Router {
    rules: Vec<Rule>,
    default_handler: Box<dyn Handler>,
}

impl Router {
    /// Build a `Router` from the application [`Config`].
    ///
    /// Each `RouterConfig` rule is mapped to a handler by its `handler` field:
    ///
    /// | `handler` value | Concrete type    |
    /// |-----------------|------------------|
    /// | `"inject"`      | `InjectHandler`  |
    /// | `"llm"`         | `LlmHandler`     |
    /// | `"shell"`       | `ShellHandler`   |
    /// | *(unknown)*     | `InjectHandler`  | (with a warning)
    ///
    /// The default handler (used when no rule matches) is always `InjectHandler`
    /// using the `inject.method` from config.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        let default_handler: Box<dyn Handler> =
            Box::new(InjectHandler::new(config.inject.method));

        let rules = config
            .router
            .rules
            .iter()
            .filter_map(|r| {
                if r.handler == "llm" && !config.llm.enabled {
                    warn!(
                        "[router] rule {:?} specifies handler=\"llm\" but llm.enabled=false — skipping",
                        r.trigger
                    );
                    return None;
                }
                Some(Rule {
                    trigger: r.trigger.clone(),
                    handler: build_handler(&r.handler, config),
                })
            })
            .collect();

        Self { rules, default_handler }
    }

    /// Dispatch `text` to the first matching rule handler, or the default.
    ///
    /// Matching is done by `str::starts_with` on the trigger string. The
    /// trigger prefix (plus any leading whitespace on the remainder) is
    /// stripped before the payload is passed to the handler.
    ///
    /// # Errors
    ///
    /// Propagates any error returned by the matched handler.
    pub fn dispatch(&self, text: &str) -> Result<()> {
        for rule in &self.rules {
            if text.starts_with(&rule.trigger) {
                let payload = text[rule.trigger.len()..].trim_start();
                log::debug!(
                    "[router] trigger {:?} matched, dispatching to '{}'",
                    rule.trigger,
                    rule.handler.name()
                );
                return rule.handler.handle(payload);
            }
        }
        log::debug!(
            "[router] no rule matched, dispatching to default '{}'",
            self.default_handler.name()
        );
        self.default_handler.handle(text)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_handler(name: &str, config: &Config) -> Box<dyn Handler> {
    let inject_method = config.inject.method;
    match name {
        "inject" => Box::new(InjectHandler::new(inject_method)),
        "llm" => match LlmHandler::from_env() {
            Ok(h) => Box::new(h),
            Err(e) => {
                warn!("[router] failed to build LLM handler ({e:#}), falling back to inject");
                Box::new(InjectHandler::new(inject_method))
            }
        },
        "shell" => Box::new(ShellHandler::new()),
        other => {
            warn!(
                "[router] unknown handler {:?}, falling back to inject",
                other
            );
            Box::new(InjectHandler::new(inject_method))
        }
    }
}
