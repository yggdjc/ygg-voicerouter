//! Continuous listening mode — VAD, speaker verification, intent classification.

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
use speaker::SpeakerVerifier;
use vad::EnergyVad;

/// Path to the speaker enrollment embedding file.
const SPEAKER_ENROLLMENT_PATH: &str = ".config/voicerouter/speaker.bin";

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

impl Actor for ContinuousActor {
    fn name(&self) -> &str {
        "continuous"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        // Init VAD.
        let mut vad = EnergyVad::new(
            self.config.audio.sample_rate,
            self.config.audio.silence_threshold as f32,
        );

        // Load speaker verifier if enabled.
        let speaker_verifier = if self.config.continuous.speaker_verify {
            load_speaker_verifier(self.config.continuous.speaker_threshold as f32)
        } else {
            None
        };

        // Lazy ASR engine (initialized on first segment).
        let mut asr_engine: Option<AsrEngine> = None;

        // Build stage info from pipeline config for intent dispatch.
        let stage_configs = self.config.effective_pipeline_stages();
        let stage_infos: Vec<StageInfo> = stage_configs
            .iter()
            .map(|sc| {
                let trigger = extract_trigger(&sc.condition);
                let handler = build_handler(&sc.handler, &self.config);
                let risk = handler.risk_level();
                StageInfo {
                    trigger,
                    name: sc.name.clone(),
                    risk,
                }
            })
            .collect();

        // Build trigger list for IntentFilter.
        let trigger_refs: Vec<&str> = stage_infos
            .iter()
            .filter(|s| !s.trigger.is_empty())
            .map(|s| s.trigger.as_str())
            .collect();
        let intent_filter = IntentFilter::new(&trigger_refs);

        // Available actions for LLM classification.
        let available_actions: Vec<String> = stage_infos
            .iter()
            .filter(|s| !s.trigger.is_empty())
            .map(|s| s.trigger.clone())
            .collect();

        // Init LLM client (optional).
        let llm_client = if !self.config.continuous.llm.endpoint.is_empty() {
            match LlmClient::new(&self.config.continuous.llm) {
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

        let sample_rate = self.config.audio.sample_rate;
        let mut muted = false;

        log::info!("[continuous] ready");

        loop {
            // Check control messages (non-blocking).
            while let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::Shutdown => {
                        log::info!("[continuous] stopped");
                        return;
                    }
                    Message::MuteInput => {
                        muted = true;
                        log::debug!("[continuous] muted");
                    }
                    Message::UnmuteInput => {
                        muted = false;
                        log::debug!("[continuous] unmuted");
                    }
                    _ => {}
                }
            }

            // Read audio chunk.
            match self.audio_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) => {
                    if muted {
                        continue;
                    }
                    vad.feed(&chunk, &mut |segment| {
                        let dur = segment.len() as f32 / sample_rate as f32;
                        log::info!(
                            "[continuous] VAD segment: {dur:.1}s, \
                             {} samples",
                            segment.len()
                        );

                        // Speaker verification gate.
                        if let Some(ref _verifier) = speaker_verifier {
                            // TODO(task-8): extract embedding and verify.
                            // For now, speaker verifier is loaded but embedding
                            // extraction requires a model not yet integrated.
                            // Skip verification until enrollment CLI is done.
                            log::debug!(
                                "[continuous] speaker verify: \
                                 skipping (embedding extraction not yet wired)"
                            );
                        }

                        // Lazy-init ASR engine.
                        if asr_engine.is_none() {
                            log::info!(
                                "[continuous] initialising ASR engine (lazy)"
                            );
                            match AsrEngine::new(&self.config.asr) {
                                Ok(e) => asr_engine = Some(e),
                                Err(e) => {
                                    log::error!(
                                        "[continuous] ASR init failed: {e:#}"
                                    );
                                    return;
                                }
                            }
                        }

                        // Transcribe.
                        let transcript = match asr_engine
                            .as_mut()
                            .unwrap()
                            .transcribe(segment, sample_rate)
                        {
                            Ok(t) if !t.is_empty() => t,
                            Ok(_) => {
                                log::debug!("[continuous] empty transcript");
                                return;
                            }
                            Err(e) => {
                                log::error!(
                                    "[continuous] transcription failed: {e:#}"
                                );
                                return;
                            }
                        };

                        log::info!(
                            "[continuous] transcript: {transcript:?}"
                        );

                        // Classify intent.
                        let intent = intent_filter.classify(&transcript);
                        log::debug!(
                            "[continuous] intent: {intent:?}"
                        );

                        match intent {
                            Intent::Command => {
                                dispatch_command(
                                    &transcript,
                                    &stage_infos,
                                    &outbox,
                                );
                            }
                            Intent::Uncertain => {
                                if let Some(ref client) = llm_client {
                                    handle_uncertain(
                                        &transcript,
                                        client,
                                        &available_actions,
                                        &stage_infos,
                                        &outbox,
                                    );
                                } else {
                                    log::debug!(
                                        "[continuous] discarding uncertain \
                                         (no LLM): {transcript:?}"
                                    );
                                }
                            }
                            Intent::Ambient => {
                                log::debug!(
                                    "[continuous] ambient, discarding: \
                                     {transcript:?}"
                                );
                            }
                        }
                    });
                }
                Err(_) => {} // timeout, loop back to check inbox
            }
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

/// Load speaker enrollment from disk.
fn load_speaker_verifier(threshold: f32) -> Option<SpeakerVerifier> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!(
                "[continuous] cannot determine home dir for speaker enrollment"
            );
            return None;
        }
    };
    let path = home.join(SPEAKER_ENROLLMENT_PATH);
    if !path.exists() {
        log::warn!(
            "[continuous] speaker enrollment not found at {}; \
             speaker verification disabled",
            path.display()
        );
        return None;
    }

    match std::fs::read(&path) {
        Ok(bytes) => {
            // Enrollment file is raw f32 little-endian embedding.
            if bytes.len() % 4 != 0 {
                log::warn!(
                    "[continuous] invalid speaker enrollment file size"
                );
                return None;
            }
            let embedding: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            log::info!(
                "[continuous] loaded speaker enrollment ({} dims)",
                embedding.len()
            );
            Some(SpeakerVerifier::from_enrollment(embedding, threshold))
        }
        Err(e) => {
            log::warn!(
                "[continuous] failed to read speaker enrollment: {e}"
            );
            None
        }
    }
}

/// Dispatch a Command intent to the pipeline.
fn dispatch_command(
    transcript: &str,
    stage_infos: &[StageInfo],
    outbox: &Sender<Message>,
) {
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
            send_for_risk(transcript, &info.name, info.risk, outbox);
        }
        None => {
            // Imperative verb detected but no trigger matched — default inject (low risk).
            log::info!(
                "[continuous] command with no trigger match, default inject"
            );
            send_for_risk(transcript, "default", RiskLevel::Low, outbox);
        }
    }
}

/// Handle an Uncertain intent by calling the LLM.
fn handle_uncertain(
    transcript: &str,
    llm: &LlmClient,
    available_actions: &[String],
    stage_infos: &[StageInfo],
    outbox: &Sender<Message>,
) {
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

                match matched {
                    Some(info) => {
                        send_for_risk(text, &info.name, info.risk, outbox);
                    }
                    None => {
                        send_for_risk(text, "default", RiskLevel::Low, outbox);
                    }
                }
            }
            // intent == "ambient" → discard
        }
        Err(e) => {
            log::warn!(
                "[continuous] LLM classification failed: {e:#}; \
                 discarding uncertain"
            );
        }
    }
}

/// Send PipelineInput (low risk) or ConfirmAction (high risk).
fn send_for_risk(
    text: &str,
    stage_name: &str,
    risk: RiskLevel,
    outbox: &Sender<Message>,
) {
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
        send_for_risk("test", "default", RiskLevel::Low, &tx);
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::PipelineInput { text, .. } if text == "test"));
    }

    #[test]
    fn send_for_risk_high_sends_confirm_action() {
        let (tx, rx) = crossbeam::channel::bounded(8);
        send_for_risk("rm -rf /", "shell_stage", RiskLevel::High, &tx);
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
        dispatch_command("打开浏览器", &stages, &tx);
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::PipelineInput { .. }));
    }

    #[test]
    fn dispatch_command_matched_trigger() {
        let stages = vec![StageInfo {
            trigger: "搜索".to_string(),
            name: "search".to_string(),
            risk: RiskLevel::High,
        }];
        let (tx, rx) = crossbeam::channel::bounded(8);
        dispatch_command("搜索天气", &stages, &tx);
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::ConfirmAction { stage, .. } if stage == "search"));
    }
}
