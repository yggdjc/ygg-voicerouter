//! CoreActor — owns audio pipeline, ASR engine, noise tracker, and postprocessing.

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::AsrEngine;
use crate::audio::{self, AudioPipeline, NoiseTracker};
use crate::config::Config;
use crate::postprocess::postprocess;
use crate::sound;

const MIN_RECORDING_SECS: f32 = 0.3;

enum CoreState {
    Idle,
    Recording,
    Muted,
}

pub struct CoreActor {
    config: Config,
    preload: bool,
}

impl CoreActor {
    #[must_use]
    pub fn new(config: Config, preload: bool) -> Self {
        Self { config, preload }
    }
}

impl Actor for CoreActor {
    fn name(&self) -> &str {
        "core"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let mut audio = match AudioPipeline::new(&self.config.audio) {
            Ok(a) => a,
            Err(e) => {
                log::error!("[core] failed to open audio: {e:#}");
                return;
            }
        };

        let initial_floor = audio::calibrate_silence(
            &mut audio,
            self.config.audio.sample_rate,
            self.config.audio.silence_threshold as f32,
        );
        let mut noise_tracker = NoiseTracker::new(
            initial_floor,
            self.config.audio.silence_threshold as f32,
            self.config.audio.sample_rate,
        );

        let mut asr_engine: Option<AsrEngine> = if self.preload {
            log::info!("[core] preloading ASR model '{}'", self.config.asr.model);
            match AsrEngine::new(&self.config.asr) {
                Ok(e) => Some(e),
                Err(e) => {
                    log::error!("[core] preload failed: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        let mut punctuator: Option<sherpa_onnx::OfflinePunctuation> = None;
        let mut recording_start: Option<Instant> = None;
        let mut state = CoreState::Idle;
        let max_record =
            Duration::from_secs(u64::from(self.config.audio.max_record_seconds));

        log::info!("[core] ready");

        loop {
            match state {
                CoreState::Idle => match inbox.recv() {
                    Ok(Message::StartListening) => {
                        log::info!("[core] recording started");
                        beep_if(&self.config, sound::beep_start);
                        if let Err(e) = audio.start_recording() {
                            log::error!("[core] start_recording failed: {e:#}");
                            beep_if(&self.config, sound::beep_error);
                            continue;
                        }
                        recording_start = Some(Instant::now());
                        state = CoreState::Recording;
                    }
                    Ok(Message::Shutdown) => break,
                    _ => {}
                },
                CoreState::Recording => {
                    crossbeam::select! {
                        recv(inbox) -> msg => match msg {
                            Ok(Message::StopListening) => {
                                finalize_recording(
                                    &mut audio, &mut asr_engine, &mut punctuator,
                                    &self.config, &outbox, &mut recording_start,
                                    &mut noise_tracker,
                                );
                                state = CoreState::Idle;
                            }
                            Ok(Message::CancelRecording) => {
                                audio.stop_recording();
                                recording_start = None;
                                log::info!(
                                    "[core] recording cancelled, audio discarded"
                                );
                                state = CoreState::Idle;
                            }
                            Ok(Message::MuteInput) => {
                                audio.stop_recording();
                                recording_start = None;
                                state = CoreState::Muted;
                            }
                            Ok(Message::Shutdown) => break,
                            _ => {}
                        },
                        default(Duration::from_millis(10)) => {
                            if let Some(start) = recording_start {
                                if start.elapsed() >= max_record {
                                    log::warn!(
                                        "[core] recording exceeded {}s limit, \
                                         force-stopping",
                                        self.config.audio.max_record_seconds
                                    );
                                    finalize_recording(
                                        &mut audio, &mut asr_engine,
                                        &mut punctuator, &self.config,
                                        &outbox, &mut recording_start,
                                        &mut noise_tracker,
                                    );
                                    outbox.send(Message::StopListening).ok();
                                    state = CoreState::Idle;
                                }
                            }
                        }
                    }
                }
                CoreState::Muted => match inbox.recv() {
                    Ok(Message::UnmuteInput) => {
                        state = CoreState::Idle;
                    }
                    Ok(Message::Shutdown) => break,
                    _ => {}
                },
            }
        }

        log::info!("[core] stopped");
    }
}

fn finalize_recording(
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
    outbox: &Sender<Message>,
    recording_start: &mut Option<Instant>,
    noise_tracker: &mut NoiseTracker,
) {
    let elapsed = recording_start
        .take()
        .map_or(0.0, |t| t.elapsed().as_secs_f32());
    log::info!("[core] recording stopped ({elapsed:.1}s)");
    beep_if(config, sound::beep_done);

    let samples = match audio.stop_recording() {
        Some(s) => s,
        None => return,
    };

    if elapsed < MIN_RECORDING_SECS {
        log::info!("[core] too short ({elapsed:.1}s), discarding");
        return;
    }

    let threshold = noise_tracker.threshold();
    let peak = audio::peak_rms(&samples, config.audio.sample_rate);
    if peak < threshold {
        log::info!(
            "[core] silence (peak {peak:.4} < {threshold:.4}), discarding"
        );
        return;
    }

    // Lazy-init ASR engine.
    if asr_engine.is_none() {
        log::info!("[core] initialising ASR engine (lazy)");
        match AsrEngine::new(&config.asr) {
            Ok(e) => *asr_engine = Some(e),
            Err(e) => {
                log::error!("[core] ASR init failed: {e:#}");
                beep_if(config, sound::beep_error);
                return;
            }
        }
    }

    let raw = match asr_engine
        .as_mut()
        .unwrap()
        .transcribe(&samples, config.audio.sample_rate)
    {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            log::info!("[core] transcribed: (empty)");
            return;
        }
        Err(e) => {
            log::error!("[core] transcription failed: {e:#}");
            beep_if(config, sound::beep_error);
            return;
        }
    };

    // Punctuation restoration.
    let with_punct = if config.postprocess.restore_punctuation {
        add_punctuation(&raw, punctuator, config)
    } else {
        raw.clone()
    };

    let text = postprocess(&with_punct, &config.postprocess);
    log::info!("[core] transcribed: {text:?}");
    outbox.send(Message::Transcript { text, raw }).ok();
}

fn add_punctuation(
    text: &str,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
) -> String {
    if punctuator.is_none() {
        let model_path =
            match crate::asr::models::expand_tilde(&config.postprocess.punctuation_model)
            {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("[core] punctuation model path error: {e}");
                    return text.to_owned();
                }
            };
        let model_file = model_path.join("model.int8.onnx");
        if !model_file.exists() {
            log::warn!(
                "[core] punctuation model not found at {}",
                model_file.display()
            );
            return text.to_owned();
        }
        log::info!(
            "[core] loading punctuation model from {}",
            model_file.display()
        );
        let cfg = sherpa_onnx::OfflinePunctuationConfig {
            model: sherpa_onnx::OfflinePunctuationModelConfig {
                ct_transformer: Some(model_file.to_string_lossy().into_owned()),
                ..Default::default()
            },
        };
        *punctuator = sherpa_onnx::OfflinePunctuation::create(&cfg);
    }

    match punctuator.as_ref() {
        Some(punc) => {
            punc.add_punctuation(text).unwrap_or_else(|| text.to_owned())
        }
        None => text.to_owned(),
    }
}

fn beep_if(config: &Config, f: fn() -> anyhow::Result<()>) {
    if config.sound.feedback {
        if let Err(e) = f() {
            log::debug!("[core] beep failed: {e}");
        }
    }
}
