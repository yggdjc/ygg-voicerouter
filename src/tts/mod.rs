//! TTS actor and engine abstraction.

pub mod sherpa;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::config::TtsConfig;

/// Abstract TTS engine.
pub trait TtsEngine: Send {
    fn synthesize(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    fn sample_rate(&self) -> u32;
}

pub struct TtsActor {
    config: TtsConfig,
}

impl TtsActor {
    #[must_use]
    pub fn new(config: TtsConfig) -> Self {
        Self { config }
    }
}

impl Actor for TtsActor {
    fn name(&self) -> &str {
        "tts"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.enabled {
            log::info!("[tts] disabled, actor idle");
            for msg in inbox {
                if matches!(msg, Message::Shutdown) {
                    break;
                }
            }
            return;
        }

        // Lazy-init engine on first SpeakRequest.
        let mut engine: Option<Box<dyn TtsEngine>> = None;

        for msg in inbox {
            match msg {
                Message::SpeakRequest { text, source: _source } => {
                    if engine.is_none() {
                        match self.config.engine.as_str() {
                            "sherpa-onnx" => match sherpa::SherpaTts::new(&self.config) {
                                Ok(e) => engine = Some(Box::new(e)),
                                Err(e) => {
                                    log::error!("[tts] engine init failed: {e:#}");
                                    continue;
                                }
                            },
                            other => {
                                log::error!("[tts] unknown engine: {other}");
                                continue;
                            }
                        }
                    }

                    if self.config.mute_mic_during_playback {
                        outbox.send(Message::MuteInput).ok();
                    }

                    log::info!("[tts] speaking: {text:?}");
                    if let Some(ref eng) = engine {
                        match eng.synthesize(&text) {
                            Ok(samples) => {
                                if let Err(e) = play_audio(&samples, eng.sample_rate()) {
                                    log::error!("[tts] playback failed: {e:#}");
                                }
                            }
                            Err(e) => log::error!("[tts] synthesis failed: {e:#}"),
                        }
                    }

                    if self.config.mute_mic_during_playback {
                        outbox.send(Message::UnmuteInput).ok();
                    }
                    outbox.send(Message::SpeakDone).ok();
                }
                Message::Shutdown => break,
                _ => {}
            }
        }

        log::info!("[tts] stopped");
    }
}

fn play_audio(samples: &[f32], sample_rate: u32) -> anyhow::Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no output device"))?;

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples = samples.to_vec();
    let pos = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let pos_clone = std::sync::Arc::clone(&pos);
    let len = samples.len();

    let (done_tx, done_rx) = crossbeam::channel::bounded(1);

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                let idx = pos_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *sample = if idx < len { samples[idx] } else { 0.0 };
            }
            if pos_clone.load(std::sync::atomic::Ordering::Relaxed) >= len {
                let _ = done_tx.try_send(());
            }
        },
        |err| log::error!("[tts] stream error: {err}"),
        None,
    )?;

    stream.play()?;
    let _ = done_rx.recv_timeout(std::time::Duration::from_secs(30));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_actor_name() {
        let actor = TtsActor::new(crate::config::TtsConfig::default());
        assert_eq!(Actor::name(&actor), "tts");
    }
}
