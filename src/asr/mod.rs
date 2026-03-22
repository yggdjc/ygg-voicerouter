//! ASR (Automatic Speech Recognition) module.
//!
//! Provides an offline ASR engine backed by [sherpa-onnx] via the
//! [`sherpa-rs`](https://crates.io/crates/sherpa-rs) crate, plus model
//! discovery and path management utilities.
//!
//!
//! # Quick start
//!
//! ```no_run
//! use voicerouter::asr::engine::AsrEngine;
//! use voicerouter::config::AsrConfig;
//!
//! let config = AsrConfig::default();
//! // Fails if model files are not installed; see models::ensure_model docs.
//! // let mut engine = AsrEngine::new(&config).unwrap();
//! // let text = engine.transcribe(&samples, 16_000).unwrap();
//! ```

pub mod engine;
pub mod models;

pub use engine::AsrEngine;
