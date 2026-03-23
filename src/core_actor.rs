//! CoreActor — owns ASR engine, noise tracker, and postprocessing.
//! Receives audio chunks from a broadcast channel instead of owning an AudioPipeline.

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::AsrEngine;
use crate::audio::{self, NoiseTracker};
use crate::audio_source::AudioChunk;
use crate::config::Config;
use crate::postprocess::postprocess;
use crate::sound;

const MIN_RECORDING_SECS: f32 = 0.3;
/// Consecutive silence duration (seconds) before auto-stopping recording.
const SILENCE_AUTO_STOP_SECS: f32 = 1.5;

enum CoreState {
    Idle,
    Recording,
    Muted,
}

pub struct CoreActor {
    config: Config,
    preload: bool,
    audio_rx: Receiver<AudioChunk>,
}

impl CoreActor {
    #[must_use]
    pub fn new(
        config: Config,
        preload: bool,
        audio_rx: Receiver<AudioChunk>,
    ) -> Self {
        Self { config, preload, audio_rx }
    }
}

impl Actor for CoreActor {
    fn name(&self) -> &str {
        "core"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let initial_floor = audio::calibrate_silence_from_channel(
            &self.audio_rx,
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
        let mut recording_buffer: Vec<f32> = Vec::new();
        let denoise_enabled = self.config.audio.denoise;
        let mut state = CoreState::Idle;
        let max_record =
            Duration::from_secs(u64::from(self.config.audio.max_record_seconds));
        let silence_threshold = noise_tracker.threshold();
        let silence_auto_stop = Duration::from_secs_f32(SILENCE_AUTO_STOP_SECS);
        let mut silence_since: Option<Instant> = None;
        let mut speech_detected = false;
        // Cooldown after finalize to let inject complete before wakeword retriggers.
        let mut cooldown_until: Option<Instant> = None;
        let mut active_wakeword: Option<String> = None;

        log::info!("[core] ready");

        loop {
            match state {
                CoreState::Idle => {
                    // Drain audio to prevent backpressure on AudioSource.
                    while self.audio_rx.try_recv().is_ok() {}

                    match inbox.recv() {
                        Ok(Message::StartListening { wakeword }) => {
                            // Ignore during cooldown (inject still in progress).
                            if let Some(until) = cooldown_until {
                                if Instant::now() < until {
                                    log::debug!("[core] ignoring StartListening during cooldown");
                                    continue;
                                }
                                cooldown_until = None;
                            }
                            active_wakeword = wakeword;
                            log::info!("[core] recording started");
                            beep_if(&self.config, sound::beep_start);
                            recording_buffer.clear();
                            recording_start = Some(Instant::now());
                            state = CoreState::Recording;
                        }
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
                CoreState::Recording => {
                    crossbeam::select! {
                        recv(self.audio_rx) -> chunk => {
                            if let Ok(chunk) = chunk {
                                recording_buffer.extend_from_slice(&chunk);

                                // Silence detection: check RMS of latest chunk.
                                let rms = audio::compute_rms(&chunk);
                                if rms >= silence_threshold {
                                    speech_detected = true;
                                    silence_since = None;
                                } else if speech_detected {
                                    // Only start silence timer after speech was detected.
                                    if silence_since.is_none() {
                                        silence_since = Some(Instant::now());
                                    }
                                    if let Some(since) = silence_since {
                                        if since.elapsed() >= silence_auto_stop {
                                            log::info!(
                                                "[core] silence detected, auto-stopping"
                                            );
                                            let elapsed = recording_start
                                                .take()
                                                .map_or(0.0, |t| t.elapsed().as_secs_f32());
                                            finalize_recording(
                                                &recording_buffer, denoise_enabled,
                                                &mut asr_engine, &mut punctuator,
                                                &self.config, &outbox, elapsed,
                                                &mut noise_tracker, &active_wakeword,
                                            );
                                            recording_buffer.clear();
                                            silence_since = None;
                                            speech_detected = false;
                                            active_wakeword = None;
                                            // Cooldown: 2s for inject to complete.
                                            cooldown_until = Some(Instant::now() + Duration::from_secs(2));
                                            outbox.send(Message::StopListening).ok();
                                            state = CoreState::Idle;
                                        }
                                    }
                                }
                            }
                            // Check recording timeout.
                            if let Some(start) = recording_start {
                                if start.elapsed() >= max_record {
                                    log::warn!(
                                        "[core] recording exceeded {}s limit, \
                                         force-stopping",
                                        self.config.audio.max_record_seconds
                                    );
                                    finalize_recording(
                                        &recording_buffer, denoise_enabled,
                                        &mut asr_engine, &mut punctuator,
                                        &self.config, &outbox,
                                        recording_start.take().map_or(
                                            0.0, |t| t.elapsed().as_secs_f32(),
                                        ),
                                        &mut noise_tracker, &active_wakeword,
                                    );
                                    recording_buffer.clear();
                                    recording_start = None;
                                    silence_since = None;
                                    speech_detected = false;
                                    active_wakeword = None;
                                    outbox.send(Message::StopListening).ok();
                                    state = CoreState::Idle;
                                }
                            }
                        }
                        recv(inbox) -> msg => match msg {
                            Ok(Message::StopListening) => {
                                let elapsed = recording_start
                                    .take()
                                    .map_or(0.0, |t| t.elapsed().as_secs_f32());
                                finalize_recording(
                                    &recording_buffer, denoise_enabled,
                                    &mut asr_engine, &mut punctuator,
                                    &self.config, &outbox, elapsed,
                                    &mut noise_tracker, &active_wakeword,
                                );
                                recording_buffer.clear();
                                silence_since = None;
                                speech_detected = false;
                                active_wakeword = None;
                                state = CoreState::Idle;
                            }
                            Ok(Message::CancelRecording) => {
                                // Discard and restart silently (Auto mode toggle).
                                recording_buffer.clear();
                                recording_start = Some(Instant::now());
                                silence_since = None;
                                speech_detected = false;
                                log::info!(
                                    "[core] recording cancelled, restarting for toggle"
                                );
                            }
                            Ok(Message::MuteInput) => {
                                recording_buffer.clear();
                                recording_start = None;
                                silence_since = None;
                                speech_detected = false;
                                state = CoreState::Muted;
                            }
                            Ok(Message::Shutdown) => break,
                            _ => {}
                        },
                    }
                }
                CoreState::Muted => {
                    // Drain audio while muted.
                    while self.audio_rx.try_recv().is_ok() {}

                    match inbox.recv() {
                        Ok(Message::UnmuteInput) => {
                            state = CoreState::Idle;
                        }
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
            }
        }

        log::info!("[core] stopped");
    }
}

fn finalize_recording(
    samples: &[f32],
    denoise_enabled: bool,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
    outbox: &Sender<Message>,
    elapsed: f32,
    noise_tracker: &mut NoiseTracker,
    wakeword: &Option<String>,
) {
    log::info!("[core] recording stopped ({elapsed:.1}s)");
    beep_if(config, sound::beep_done);

    if samples.is_empty() {
        return;
    }

    if elapsed < MIN_RECORDING_SECS {
        log::info!("[core] too short ({elapsed:.1}s), discarding");
        return;
    }

    // Apply denoise if enabled.
    let samples = if denoise_enabled {
        log::debug!("[core] applying denoise to {} samples", samples.len());
        crate::audio::denoise::denoise(samples)
    } else {
        samples.to_vec()
    };

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

    // Strip wakeword prefix only for wakeword-triggered recordings.
    let text = if let Some(ref phrase) = wakeword {
        strip_wakeword_prefix(&text, std::slice::from_ref(phrase))
    } else {
        text
    };

    if text.is_empty() {
        log::info!("[core] transcribed: (only wakeword, no content)");
        return;
    }

    log::info!("[core] transcribed: {text:?}");
    outbox.send(Message::Transcript { text, raw }).ok();
}

/// Strip any configured wakeword phrase from the start of the transcription.
fn strip_wakeword_prefix(text: &str, phrases: &[String]) -> String {
    for phrase in phrases {
        if let Some(rest) = text.strip_prefix(phrase.as_str()) {
            let trimmed = rest.trim_start_matches(|c: char| c == '，' || c == '。' || c == ' ' || c == '、');
            return trimmed.trim().to_string();
        }
    }
    text.to_string()
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
