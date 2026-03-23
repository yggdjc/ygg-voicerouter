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
use handlers::{inject::InjectHandler, shell::ShellHandler};

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct Rule {
    trigger: String,
    handler: Box<dyn Handler>,
    /// Optional shell command template with `{text}` placeholder.
    command: Option<String>,
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
            .map(|r| Rule {
                trigger: r.trigger.clone(),
                handler: build_handler(&r.handler, config),
                command: r.command.clone(),
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
                let payload = text[rule.trigger.len()..]
                    .trim_start()
                    .trim_end_matches(|c: char| c.is_ascii_punctuation() || matches!(c,
                        '，' | '。' | '？' | '！' | '；' | '：' | '、' | '…'
                    ))
                    .trim();
                log::debug!(
                    "[router] trigger {:?} matched, dispatching to '{}'",
                    rule.trigger,
                    rule.handler.name()
                );
                // Apply command template if present, replacing {text}.
                // URL-encode the payload to avoid shell injection from
                // characters like single quotes in ASR output.
                let final_payload = match &rule.command {
                    Some(tpl) => {
                        let encoded = url_encode(payload);
                        tpl.replace("{text}", &encoded)
                    }
                    None => payload.to_string(),
                };
                return rule.handler.handle(&final_payload);
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

/// Percent-encode a string for safe use in URLs and shell commands.
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push('%');
                result.push_str(&format!("{b:02X}"));
            }
        }
    }
    result
}

fn build_handler(name: &str, config: &Config) -> Box<dyn Handler> {
    let inject_method = config.inject.method;
    match name {
        "inject" => Box::new(InjectHandler::new(inject_method)),
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
