//! voicerouter — Voice router for Linux: offline ASR with pluggable handlers.

pub mod actor;
pub mod audio_source;
pub mod pipeline;
pub mod asr;
pub mod audio;
pub mod config;
pub mod core_actor;
pub mod hotkey;
pub mod inject;
pub mod ipc;
pub mod postprocess;
pub mod sound;
pub mod tts;
pub mod wakeword;
pub mod continuous;
pub mod llm;
pub mod conversation;
pub mod vad;
pub mod overlay;
