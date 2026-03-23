//! Wakeword actor — continuous ASR-based wake word detection.

pub mod detector;

use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::AsrEngine;
use crate::config::Config;
use detector::WakewordDetector;

pub struct WakewordActor {
    config: Config,
}

impl WakewordActor {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Actor for WakewordActor {
    fn name(&self) -> &str {
        "wakeword"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.wakeword.enabled {
            log::info!("[wakeword] disabled, actor idle");
            for msg in inbox {
                if matches!(msg, Message::Shutdown) {
                    break;
                }
            }
            return;
        }

        let _detector = WakewordDetector::new(self.config.wakeword.phrases.clone());
        let stride = Duration::from_secs_f64(self.config.wakeword.stride_seconds);

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
        let mut _asr = match AsrEngine::new(&asr_config) {
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

        let _outbox = outbox;
        let mut muted = false;
        loop {
            match inbox.try_recv() {
                Ok(Message::Shutdown) => break,
                Ok(Message::MuteInput) => muted = true,
                Ok(Message::UnmuteInput) => muted = false,
                _ => {}
            }

            if muted {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            // TODO(#14): Read audio samples from AudioSource channel.
            // When AudioSource is implemented, this will:
            // 1. Collect window_samples from audio channel
            // 2. Run ASR on the window
            // 3. Check _detector.check() on ASR output
            // 4. If matched, emit StartListening or PipelineInput

            std::thread::sleep(stride);
        }

        log::info!("[wakeword] stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wakeword_actor_name() {
        let actor = WakewordActor::new(Config::default());
        assert_eq!(Actor::name(&actor), "wakeword");
    }
}
