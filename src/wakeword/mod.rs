//! Wakeword actor — continuous ASR-based wake word detection.

pub mod detector;

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::AsrEngine;
use crate::audio_source::AudioChunk;
use crate::config::Config;
use detector::WakewordDetector;

pub struct WakewordActor {
    config: Config,
    audio_rx: crossbeam::channel::Receiver<AudioChunk>,
}

impl WakewordActor {
    #[must_use]
    pub fn new(
        config: Config,
        audio_rx: crossbeam::channel::Receiver<AudioChunk>,
    ) -> Self {
        Self { config, audio_rx }
    }
}

impl Actor for WakewordActor {
    fn name(&self) -> &str {
        "wakeword"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.wakeword.enabled {
            log::info!("[wakeword] disabled, actor idle");
            loop {
                crossbeam::select! {
                    recv(inbox) -> msg => {
                        if matches!(msg, Ok(Message::Shutdown)) { break; }
                    }
                    recv(self.audio_rx) -> _ => {} // discard
                }
            }
            return;
        }

        let detector = WakewordDetector::new(self.config.wakeword.phrases.clone());
        let window_samples = (self.config.wakeword.window_seconds
            * self.config.audio.sample_rate as f64) as usize;
        let stride_samples = (self.config.wakeword.stride_seconds
            * self.config.audio.sample_rate as f64) as usize;

        // Init separate ASR engine for wakeword detection.
        let asr_model = if self.config.wakeword.model.is_empty() {
            self.config.asr.model.clone()
        } else {
            self.config.wakeword.model.clone()
        };
        let asr_config = crate::config::AsrConfig {
            model: asr_model,
            model_dir: self.config.asr.model_dir.clone(),
        };
        let mut asr = match AsrEngine::new(&asr_config) {
            Ok(e) => e,
            Err(e) => {
                log::error!("[wakeword] ASR init failed: {e:#}");
                return;
            }
        };

        log::info!(
            "[wakeword] ready, phrases: {:?}",
            self.config.wakeword.phrases
        );

        let mut window: Vec<f32> = Vec::with_capacity(window_samples);
        let mut muted = false;
        let mut samples_since_last_asr: usize = 0;

        loop {
            // Check control messages (non-blocking).
            while let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::Shutdown => return,
                    Message::MuteInput => {
                        muted = true;
                        window.clear();
                    }
                    Message::UnmuteInput => {
                        muted = false;
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
                    window.extend_from_slice(&chunk);
                    samples_since_last_asr += chunk.len();

                    // Trim window to max size (sliding).
                    if window.len() > window_samples {
                        let excess = window.len() - window_samples;
                        window.drain(..excess);
                    }

                    // Run ASR every stride_samples.
                    if samples_since_last_asr >= stride_samples
                        && window.len() >= window_samples
                    {
                        samples_since_last_asr = 0;

                        match asr.transcribe(&window, self.config.audio.sample_rate)
                        {
                            Ok(text) if !text.is_empty() => {
                                if let Some((phrase, remainder)) =
                                    detector.check(&text)
                                {
                                    log::info!(
                                        "[wakeword] detected '{phrase}', \
                                         remainder: {remainder:?}"
                                    );
                                    emit_action(
                                        &self.config,
                                        &outbox,
                                        remainder,
                                    );
                                    // Clear window after detection to avoid
                                    // re-triggering.
                                    window.clear();
                                }
                            }
                            Ok(_) => {} // empty transcript
                            Err(e) => {
                                log::debug!("[wakeword] ASR error: {e}");
                            }
                        }
                    }
                }
                Err(_) => {} // timeout, loop back to check inbox
            }
        }
    }
}

fn emit_action(config: &Config, outbox: &Sender<Message>, remainder: &str) {
    match config.wakeword.action {
        crate::config::WakewordAction::StartRecording => {
            outbox.send(Message::StartListening).ok();
        }
        crate::config::WakewordAction::PipelinePassthrough => {
            if !remainder.is_empty() {
                outbox
                    .send(Message::PipelineInput {
                        text: remainder.to_string(),
                        metadata: crate::actor::Metadata {
                            source: "wakeword".to_string(),
                            timestamp: std::time::Instant::now(),
                        },
                    })
                    .ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wakeword_actor_name() {
        let (_tx, rx) = crossbeam::channel::bounded(1);
        let actor = WakewordActor::new(Config::default(), rx);
        assert_eq!(Actor::name(&actor), "wakeword");
    }
}
