//! CoreActor — owns ASR engine, noise tracker, and postprocessing.
//! Receives audio chunks from a broadcast channel instead of owning an AudioPipeline.

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::{AsrEngine, CloudAsr};
use crate::audio::{self, NoiseTracker};
use crate::audio_source::AudioChunk;
use crate::config::Config;
use crate::overlay::{self, OverlayClient};
use crate::postprocess::postprocess;
use crate::sound;

const MIN_RECORDING_SECS: f32 = 0.3;
/// Consecutive silence duration (seconds) before auto-stopping recording.
const SILENCE_AUTO_STOP_SECS: f32 = 1.5;

/// Why a recording was automatically stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Silence detected after speech (wakeword mode only).
    Silence,
    /// Recording exceeded max duration (hotkey mode only).
    Timeout,
}

/// Pure-logic check for whether a recording should auto-stop.
///
/// Wakeword-triggered recordings stop on silence; hotkey-triggered recordings
/// stop on timeout. This separation prevents silence auto-stop from cutting
/// off hotkey dictation mid-pause.
pub struct RecordingStopCheck {
    pub is_wakeword: bool,
    pub speech_detected: bool,
    pub silence_duration: Duration,
    pub max_record: Duration,
}

impl RecordingStopCheck {
    /// Returns `Some(reason)` if the recording should stop now.
    #[must_use]
    pub fn should_stop(
        &self,
        silence_since: Option<Instant>,
        recording_start: Instant,
    ) -> Option<StopReason> {
        if self.is_wakeword {
            // Wakeword: stop on silence after speech, no timeout.
            if self.speech_detected {
                if let Some(since) = silence_since {
                    if since.elapsed() >= self.silence_duration {
                        return Some(StopReason::Silence);
                    }
                }
            }
        } else {
            // Hotkey: stop on timeout only, no silence auto-stop.
            if recording_start.elapsed() >= self.max_record {
                return Some(StopReason::Timeout);
            }
        }
        None
    }
}

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

        let mut cloud_asr: Option<CloudAsr> = if self.config.asr.cloud.enabled {
            match CloudAsr::new(&self.config.asr.cloud) {
                Ok(c) => {
                    log::info!("[core] cloud ASR initialised");
                    Some(c)
                }
                Err(e) => {
                    log::warn!("[core] cloud ASR init failed: {e:#}, using local only");
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
        let mut overlay = OverlayClient::new();

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

                                // Update silence tracking from latest chunk RMS.
                                let rms = audio::compute_rms(&chunk);
                                overlay.send_recording(overlay::rms_to_level(rms));
                                if rms >= silence_threshold {
                                    speech_detected = true;
                                    silence_since = None;
                                } else if speech_detected && silence_since.is_none() {
                                    silence_since = Some(Instant::now());
                                }
                            }

                            // Check auto-stop conditions (mode-dependent).
                            if let Some(start) = recording_start {
                                let stop_check = RecordingStopCheck {
                                    is_wakeword: active_wakeword.is_some(),
                                    speech_detected,
                                    silence_duration: silence_auto_stop,
                                    max_record,
                                };
                                if let Some(reason) = stop_check.should_stop(silence_since, start) {
                                    match reason {
                                        StopReason::Silence => log::info!(
                                            "[core] silence detected, auto-stopping"
                                        ),
                                        StopReason::Timeout => log::warn!(
                                            "[core] recording exceeded {}s limit, \
                                             force-stopping",
                                            self.config.audio.max_record_seconds
                                        ),
                                    }
                                    let elapsed = recording_start
                                        .take()
                                        .map_or(0.0, |t| t.elapsed().as_secs_f32());
                                    finalize_recording(
                                        &recording_buffer, denoise_enabled,
                                        &mut cloud_asr, &mut asr_engine,
                                        &mut punctuator,
                                        &self.config, &outbox, elapsed,
                                        &mut noise_tracker, &active_wakeword,
                                        &mut overlay,
                                    );
                                    recording_buffer.clear();
                                    recording_start = None;
                                    silence_since = None;
                                    speech_detected = false;
                                    // Cooldown only for wakeword (prevent retrigger).
                                    if active_wakeword.is_some() {
                                        cooldown_until = Some(
                                            Instant::now() + Duration::from_secs(2),
                                        );
                                    }
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
                                    &mut cloud_asr, &mut asr_engine,
                                    &mut punctuator,
                                    &self.config, &outbox, elapsed,
                                    &mut noise_tracker, &active_wakeword,
                                    &mut overlay,
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
    cloud_asr: &mut Option<CloudAsr>,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
    outbox: &Sender<Message>,
    elapsed: f32,
    noise_tracker: &mut NoiseTracker,
    wakeword: &Option<String>,
    overlay: &mut OverlayClient,
) {
    log::info!("[core] recording stopped ({elapsed:.1}s)");
    beep_if(config, sound::beep_done);
    // Dismiss overlay immediately when recording stops so focus returns
    // to the target window before inject. On GNOME Wayland the overlay
    // steals focus, which breaks Ctrl+V paste.
    overlay.send_idle();

    if samples.is_empty() {
        overlay.send_idle();
        return;
    }

    if elapsed < MIN_RECORDING_SECS {
        log::info!("[core] too short ({elapsed:.1}s), discarding");
        overlay.send_idle();
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
        overlay.send_idle();
        return;
    }

    // Try cloud ASR first if available.
    let cloud_result = if let Some(ref mut cloud) = cloud_asr {
        match cloud.transcribe(&samples, config.audio.sample_rate) {
            Ok(t) if !t.is_empty() => {
                log::info!("[core] cloud ASR transcript: {t:?}");
                Some(t)
            }
            Ok(_) => {
                log::debug!("[core] cloud ASR returned empty, falling back to local");
                None
            }
            Err(e) => {
                log::warn!("[core] cloud ASR failed: {e:#}, falling back to local");
                None
            }
        }
    } else {
        None
    };

    let raw = if let Some(t) = cloud_result {
        t
    } else {
        // Lazy-init local ASR engine.
        if asr_engine.is_none() {
            log::info!("[core] initialising ASR engine (lazy)");
            match AsrEngine::new(&config.asr) {
                Ok(e) => *asr_engine = Some(e),
                Err(e) => {
                    log::error!("[core] ASR init failed: {e:#}");
                    beep_if(config, sound::beep_error);
                    overlay.send_idle();
                    return;
                }
            }
        }

        match asr_engine
            .as_mut()
            .unwrap()
            .transcribe(&samples, config.audio.sample_rate)
        {
            Ok(t) if !t.is_empty() => t,
            Ok(_) if config.asr.provider != "cpu" => {
                log::warn!("[core] GPU transcription returned empty, retrying with CPU");
                match cpu_fallback_transcribe(&samples, config) {
                    Some(t) => t,
                    None => {
                        overlay.send_idle();
                        return;
                    }
                }
            }
            Ok(_) => {
                log::info!("[core] transcribed: (empty)");
                overlay.send_idle();
                return;
            }
            Err(e) => {
                log::error!("[core] transcription failed: {e:#}");
                beep_if(config, sound::beep_error);
                overlay.send_idle();
                return;
            }
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
        overlay.send_idle();
        return;
    }

    log::info!("[core] transcribed: {text:?}");
    // Dismiss overlay BEFORE inject so focus returns to the target window.
    overlay.send_idle();
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
                provider: Some(config.asr.provider.clone()),
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

/// One-shot CPU fallback when GPU transcription returns empty.
fn cpu_fallback_transcribe(samples: &[f32], config: &Config) -> Option<String> {
    let cpu_config = crate::config::AsrConfig {
        provider: "cpu".to_owned(),
        ..config.asr.clone()
    };
    let mut engine = match AsrEngine::new(&cpu_config) {
        Ok(e) => e,
        Err(e) => {
            log::error!("[core] CPU fallback ASR init failed: {e:#}");
            return None;
        }
    };
    match engine.transcribe(samples, config.audio.sample_rate) {
        Ok(t) if !t.is_empty() => {
            log::info!("[core] CPU fallback transcribed: {t:?}");
            Some(t)
        }
        Ok(_) => {
            log::info!("[core] CPU fallback also returned empty");
            None
        }
        Err(e) => {
            log::error!("[core] CPU fallback transcription failed: {e:#}");
            None
        }
    }
}

fn beep_if(config: &Config, f: fn() -> anyhow::Result<()>) {
    if config.sound.feedback {
        if let Err(e) = f() {
            log::debug!("[core] beep failed: {e}");
        }
    }
}
