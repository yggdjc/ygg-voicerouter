//! Concrete handler implementations for the pipeline.

pub mod http;
pub mod inject;
pub mod pipe;
pub mod shell;
pub mod speak;
pub mod transform;

use crate::config::Config;
use super::handler::Handler;
use http::HttpHandler;
use inject::InjectHandler;
use pipe::PipeHandler;
use shell::ShellHandler;
use speak::SpeakHandler;
use transform::TransformHandler;

/// Build a handler by name from config.
pub fn build_handler(name: &str, config: &Config) -> Box<dyn Handler> {
    match name {
        "http" => Box::new(HttpHandler),
        "inject" => Box::new(InjectHandler::new(config.inject.method)),
        "pipe" => Box::new(PipeHandler),
        "shell" => Box::new(ShellHandler),
        "speak" => Box::new(SpeakHandler),
        "transform" => Box::new(TransformHandler),
        other => {
            log::warn!("[pipeline] unknown handler {other:?}, falling back to inject");
            Box::new(InjectHandler::new(config.inject.method))
        }
    }
}
