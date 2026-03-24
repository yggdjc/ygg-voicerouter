//! Continuous listening mode — VAD, intent classification.
//!
//! Speaker verification (`speaker` module) is retained as a library for future
//! use but is not wired into the runtime pipeline.

pub mod intent;
pub mod speaker;
pub mod vad;

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message, Metadata};
use crate::asr::AsrEngine;
use crate::audio_source::AudioChunk;
use crate::config::Config;
use crate::llm::LlmClient;
use crate::pipeline::handler::RiskLevel;
use crate::pipeline::handlers::build_handler;

use intent::{Intent, IntentFilter};
use vad::EnergyVad;

pub struct ContinuousActor {
    config: Config,
    audio_rx: Receiver<AudioChunk>,
}

impl ContinuousActor {
    #[must_use]
    pub fn new(config: Config, audio_rx: Receiver<AudioChunk>) -> Self {
        Self { config, audio_rx }
    }
}

/// A pipeline stage's trigger prefix, name, and risk level.
struct StageInfo {
    trigger: String,
    name: String,
    risk: RiskLevel,
}

/// Runtime state initialised once at actor start.
struct RuntimeState {
    vad: EnergyVad,
    asr_engine: Option<AsrEngine>,
    stage_infos: Vec<StageInfo>,
    intent_filter: IntentFilter,
    available_actions: Vec<String>,
    llm_client: Option<LlmClient>,
    sample_rate: u32,
}

/// Build all runtime state from configuration.
fn init_runtime(config: &Config) -> RuntimeState {
    let vad = EnergyVad::new(
        config.audio.sample_rate,
        config.audio.silence_threshold as f32,
    );

    let stage_configs = config.effective_pipeline_stages();
    let stage_infos: Vec<StageInfo> = stage_configs
        .iter()
        .map(|sc| {
            let trigger = extract_trigger(&sc.condition);
            let handler = build_handler(&sc.handler, config);
            let risk = handler.risk_level();
            StageInfo { trigger, name: sc.name.clone(), risk }
        })
        .collect();

    let trigger_refs: Vec<&str> = stage_infos
        .iter()
        .filter(|s| !s.trigger.is_empty())
        .map(|s| s.trigger.as_str())
        .collect();
    let intent_filter = IntentFilter::new(&trigger_refs);

    let available_actions: Vec<String> = stage_infos
        .iter()
        .filter(|s| !s.trigger.is_empty())
        .map(|s| s.trigger.clone())
        .collect();

    let llm_client = if !config.continuous.llm.endpoint.is_empty() {
        match LlmClient::new(&config.continuous.llm) {
            Ok(c) => {
                log::info!("[continuous] LLM client ready");
                Some(c)
            }
            Err(e) => {
                log::warn!(
                    "[continuous] LLM client init failed: {e:#}; \
                     uncertain intents will be discarded"
                );
                None
            }
        }
    } else {
        log::info!("[continuous] no LLM endpoint configured");
        None
    };

    RuntimeState {
        vad,
        asr_engine: None,
        stage_infos,
        intent_filter,
        available_actions,
        llm_client,
        sample_rate: config.audio.sample_rate,
    }
}

/// Process a single VAD speech segment: transcribe, classify, dispatch.
///
/// Returns `Some((text, stage))` when a high-risk confirmation is now pending.
fn process_speech_segment(
    segment: &[f32],
    state: &mut RuntimeState,
    config: &Config,
    outbox: &Sender<Message>,
) -> Option<(String, String)> {
    let dur = segment.len() as f32 / state.sample_rate as f32;
    log::info!("[continuous] VAD segment: {dur:.1}s, {} samples", segment.len());

    // Lazy-init ASR engine.
    if state.asr_engine.is_none() {
        log::info!("[continuous] initialising ASR engine (lazy)");
        match AsrEngine::new(&config.asr) {
            Ok(e) => state.asr_engine = Some(e),
            Err(e) => {
                log::error!("[continuous] ASR init failed: {e:#}");
                return None;
            }
        }
    }

    // Transcribe.
    let transcript = match state
        .asr_engine
        .as_mut()
        .unwrap()
        .transcribe(segment, state.sample_rate)
    {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            log::debug!("[continuous] empty transcript");
            return None;
        }
        Err(e) => {
            log::error!("[continuous] transcription failed: {e:#}");
            return None;
        }
    };

    log::info!("[continuous] transcript: {transcript:?}");

    let intent = state.intent_filter.classify(&transcript);
    log::debug!("[continuous] intent: {intent:?}");

    match intent {
        Intent::Command => dispatch_command(&transcript, &state.stage_infos, outbox),
        Intent::Uncertain => {
            if let Some(ref client) = state.llm_client {
                handle_uncertain(
                    &transcript,
                    client,
                    &state.available_actions,
                    &state.stage_infos,
                    outbox,
                )
            } else {
                log::debug!(
                    "[continuous] discarding uncertain (no LLM): {transcript:?}"
                );
                None
            }
        }
        Intent::Ambient => {
            log::debug!("[continuous] ambient, discarding: {transcript:?}");
            None
        }
    }
}

impl Actor for ContinuousActor {
    fn name(&self) -> &str {
        "continuous"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let mut state = init_runtime(&self.config);
        let mut muted = false;
        let mut pending_confirm: Option<(String, String)> = None;

        log::info!("[continuous] ready");

        loop {
            if drain_inbox(&inbox, &outbox, &mut muted, &mut pending_confirm) {
                return; // Shutdown received
            }

            match self.audio_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) if !muted => {
                    vad_feed(
                        &mut state,
                        &self.config,
                        &chunk,
                        &outbox,
                        &mut pending_confirm,
                    );
                }
                _ => {} // muted or timeout
            }
        }
    }
}

/// Drain all pending control messages from the inbox.
///
/// Returns `true` if `Shutdown` was received.
fn drain_inbox(
    inbox: &Receiver<Message>,
    outbox: &Sender<Message>,
    muted: &mut bool,
    pending_confirm: &mut Option<(String, String)>,
) -> bool {
    while let Ok(msg) = inbox.try_recv() {
        match msg {
            Message::Shutdown => {
                log::info!("[continuous] stopped");
                return true;
            }
            Message::MuteInput => {
                *muted = true;
                log::debug!("[continuous] muted");
            }
            Message::UnmuteInput => {
                *muted = false;
                log::debug!("[continuous] unmuted");
            }
            Message::ActionConfirmed => {
                if let Some((text, stage)) = pending_confirm.take() {
                    log::info!(
                        "[continuous] high-risk action confirmed, \
                         dispatching stage='{stage}'"
                    );
                    outbox
                        .send(Message::PipelineInput {
                            text,
                            metadata: Metadata {
                                source: "continuous".to_string(),
                                timestamp: Instant::now(),
                            },
                        })
                        .ok();
                }
            }
            Message::ActionRejected => {
                if let Some((_, stage)) = pending_confirm.take() {
                    log::info!(
                        "[continuous] high-risk action rejected/timeout for \
                         stage='{stage}', discarding"
                    );
                }
            }
            _ => {}
        }
    }
    false
}

/// Feed an audio chunk to VAD and process any resulting speech segments.
fn vad_feed(
    state: &mut RuntimeState,
    config: &Config,
    chunk: &AudioChunk,
    outbox: &Sender<Message>,
    pending_confirm: &mut Option<(String, String)>,
) {
    // Collect segments first to avoid borrowing state in the VAD callback.
    let mut segments: Vec<Vec<f32>> = Vec::new();
    state.vad.feed(chunk, &mut |segment| {
        segments.push(segment.to_vec());
    });

    for segment in &segments {
        if pending_confirm.is_some() {
            log::debug!("[continuous] ignoring segment, confirmation pending");
            break;
        }
        if let Some(pair) = process_speech_segment(segment, state, config, outbox) {
            *pending_confirm = Some(pair);
        }
    }
}

/// Extract trigger prefix from a stage condition string.
fn extract_trigger(condition: &Option<String>) -> String {
    condition
        .as_ref()
        .and_then(|c| c.strip_prefix("starts_with:"))
        .unwrap_or("")
        .to_string()
}

/// Dispatch a Command intent to the pipeline.
///
/// Returns `Some((text, stage))` if a high-risk action was sent and the caller
/// should record the pending confirmation.
fn dispatch_command(
    transcript: &str,
    stage_infos: &[StageInfo],
    outbox: &Sender<Message>,
) -> Option<(String, String)> {
    // Find the first matching stage by trigger prefix.
    let matched = stage_infos
        .iter()
        .find(|s| !s.trigger.is_empty() && transcript.starts_with(&s.trigger));

    match matched {
        Some(info) => {
            log::info!(
                "[continuous] matched stage '{}' (risk={:?})",
                info.name,
                info.risk
            );
            send_for_risk(transcript, &info.name, info.risk, outbox)
        }
        None => {
            // Imperative verb detected but no trigger matched — default inject (low risk).
            log::info!(
                "[continuous] command with no trigger match, default inject"
            );
            send_for_risk(transcript, "default", RiskLevel::Low, outbox)
        }
    }
}

/// Handle an Uncertain intent by calling the LLM.
///
/// Returns `Some((text, stage))` if a high-risk action was sent and the caller
/// should record the pending confirmation.
fn handle_uncertain(
    transcript: &str,
    llm: &LlmClient,
    available_actions: &[String],
    stage_infos: &[StageInfo],
    outbox: &Sender<Message>,
) -> Option<(String, String)> {
    match llm.classify(transcript, available_actions) {
        Ok(resp) => {
            log::info!(
                "[continuous] LLM classified: intent={}, action={:?}",
                resp.intent,
                resp.action
            );
            if resp.intent == "command" {
                // Look up matched action in stage_infos.
                let matched = if !resp.action.is_empty() {
                    stage_infos
                        .iter()
                        .find(|s| s.trigger == resp.action || s.name == resp.action)
                } else {
                    None
                };

                let text = if resp.text.is_empty() {
                    transcript
                } else {
                    &resp.text
                };

                return match matched {
                    Some(info) => send_for_risk(text, &info.name, info.risk, outbox),
                    None => send_for_risk(text, "default", RiskLevel::Low, outbox),
                };
            }
            // intent == "ambient" → discard
            None
        }
        Err(e) => {
            log::warn!(
                "[continuous] LLM classification failed: {e:#}; \
                 discarding uncertain"
            );
            None
        }
    }
}

/// Send PipelineInput (low risk) or ConfirmAction (high risk).
///
/// Returns `Some((text, stage))` when a high-risk action is sent so the
/// caller can record the pending confirmation and dispatch PipelineInput
/// upon ActionConfirmed.
fn send_for_risk(
    text: &str,
    stage_name: &str,
    risk: RiskLevel,
    outbox: &Sender<Message>,
) -> Option<(String, String)> {
    match risk {
        RiskLevel::Low => {
            outbox
                .send(Message::PipelineInput {
                    text: text.to_string(),
                    metadata: Metadata {
                        source: "continuous".to_string(),
                        timestamp: Instant::now(),
                    },
                })
                .ok();
            None
        }
        RiskLevel::High => {
            log::info!(
                "[continuous] high-risk action, requesting confirmation: \
                 stage={stage_name}"
            );
            outbox
                .send(Message::ConfirmAction {
                    text: text.to_string(),
                    stage: stage_name.to_string(),
                })
                .ok();
            Some((text.to_string(), stage_name.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_actor_name() {
        let (_tx, rx) = crossbeam::channel::bounded(1);
        let actor = ContinuousActor::new(Config::default(), rx);
        assert_eq!(Actor::name(&actor), "continuous");
    }

    #[test]
    fn extract_trigger_from_condition() {
        let cond = Some("starts_with:搜索".to_string());
        assert_eq!(extract_trigger(&cond), "搜索");
    }

    #[test]
    fn extract_trigger_none() {
        assert_eq!(extract_trigger(&None), "");
    }

    #[test]
    fn extract_trigger_non_starts_with() {
        let cond = Some("output_eq:note".to_string());
        assert_eq!(extract_trigger(&cond), "");
    }

    #[test]
    fn send_for_risk_low_sends_pipeline_input() {
        let (tx, rx) = crossbeam::channel::bounded(8);
        let pending = send_for_risk("test", "default", RiskLevel::Low, &tx);
        assert!(pending.is_none(), "low risk should not produce pending confirm");
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::PipelineInput { text, .. } if text == "test"));
    }

    #[test]
    fn send_for_risk_high_sends_confirm_action_and_returns_pending() {
        let (tx, rx) = crossbeam::channel::bounded(8);
        let pending = send_for_risk("rm -rf /", "shell_stage", RiskLevel::High, &tx);
        // Must return pending info for the caller to track.
        assert_eq!(pending, Some(("rm -rf /".to_string(), "shell_stage".to_string())));
        let msg = rx.try_recv().unwrap();
        assert!(
            matches!(msg, Message::ConfirmAction { text, stage } if text == "rm -rf /" && stage == "shell_stage")
        );
    }

    #[test]
    fn dispatch_command_no_match_sends_default() {
        let stages = vec![StageInfo {
            trigger: "搜索".to_string(),
            name: "search".to_string(),
            risk: RiskLevel::High,
        }];
        let (tx, rx) = crossbeam::channel::bounded(8);
        let pending = dispatch_command("打开浏览器", &stages, &tx);
        assert!(pending.is_none());
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::PipelineInput { .. }));
    }

    #[test]
    fn dispatch_command_matched_trigger_returns_pending() {
        let stages = vec![StageInfo {
            trigger: "搜索".to_string(),
            name: "search".to_string(),
            risk: RiskLevel::High,
        }];
        let (tx, rx) = crossbeam::channel::bounded(8);
        let pending = dispatch_command("搜索天气", &stages, &tx);
        assert_eq!(pending, Some(("搜索天气".to_string(), "search".to_string())));
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::ConfirmAction { stage, .. } if stage == "search"));
    }
}
