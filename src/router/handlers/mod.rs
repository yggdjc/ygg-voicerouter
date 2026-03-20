//! Concrete handler implementations for the voice router.
//!
//! Each sub-module provides a struct that implements [`Handler`].
//!
//! | Module       | Handler struct   | What it does                            |
//! |-------------|------------------|-----------------------------------------|
//! | [`inject`]  | `InjectHandler`  | Inject text into the focused window     |
//! | [`llm`]     | `LlmHandler`     | Send text to an OpenAI-compatible API   |
//! | [`shell`]   | `ShellHandler`   | Execute the text as a shell command     |
//!
//! [`Handler`]: crate::router::handler::Handler

pub mod inject;
pub mod llm;
pub mod shell;
