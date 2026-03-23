//! Concrete handler implementations for the pipeline.

pub mod inject;
pub mod shell;

use crate::config::Config;
use super::handler::Handler;
use inject::InjectHandler;
use shell::ShellHandler;

/// Build a handler by name from config.
pub fn build_handler(name: &str, config: &Config) -> Box<dyn Handler> {
    match name {
        "inject" => Box::new(InjectHandler::new(config.inject.method)),
        "shell" => Box::new(ShellHandler),
        other => {
            log::warn!("[pipeline] unknown handler {other:?}, falling back to inject");
            Box::new(InjectHandler::new(config.inject.method))
        }
    }
}
