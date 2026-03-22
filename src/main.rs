//! voicerouter CLI entry point.

mod service;
mod setup;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use voicerouter::asr::AsrEngine;
use voicerouter::audio::{self, AudioPipeline, NoiseTracker};
use voicerouter::config::Config;
use voicerouter::hotkey::{HotkeyEvent, HotkeyMonitor};
use voicerouter::postprocess::postprocess;
use voicerouter::router::Router;
use voicerouter::sound;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "voicerouter", version, about = "Voice router for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable verbose logging.
    #[arg(short, long)]
    verbose: bool,

    /// Path to config file.
    #[arg(short, long)]
    config: Option<String>,

    /// Record 3 s of audio, print RMS levels, then exit.
    #[arg(long)]
    test_audio: bool,

    /// Inject a string using the configured method, then exit.
    #[arg(long, value_name = "TEXT")]
    test_inject: Option<String>,

    /// Load the ASR model on startup instead of lazily on first use.
    #[arg(long)]
    preload: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Check tools, model files, and create default config if missing.
    Setup,
    /// Control the background systemd user service.
    Service {
        /// Action: install | uninstall | start | stop | status
        action: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_level),
    )
    .init();

    let config = Config::load(cli.config.as_deref())?;
    log::debug!("loaded config: {config:?}");

    if cli.test_audio {
        return run_test_audio(&config);
    }
    if let Some(text) = cli.test_inject {
        return run_test_inject(&text, &config);
    }

    match cli.command {
        None => run_daemon(config, cli.preload),
        Some(Commands::Setup) => setup::run(&config),
        Some(Commands::Service { action }) => service::run(&action),
    }
}

// ---------------------------------------------------------------------------
// --test-audio
// ---------------------------------------------------------------------------

fn run_test_audio(config: &Config) -> Result<()> {
    println!("Testing microphone — recording 3 seconds …");

    let mut pipeline = AudioPipeline::new(&config.audio)
        .context("failed to open audio device")?;

    pipeline.start_recording().context("start recording")?;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let rms = pipeline.rms();
        println!("RMS: {rms:.4}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let samples = pipeline.stop_recording().unwrap_or_default();
    let overall_rms = audio::compute_rms(&samples);
    println!(
        "Done. Captured {} samples. Overall RMS: {overall_rms:.4}",
        samples.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// --test-inject
// ---------------------------------------------------------------------------

fn run_test_inject(text: &str, config: &Config) -> Result<()> {
    println!("Injecting: {text:?}");
    voicerouter::inject::inject_text(text, config.inject.method)
        .context("inject_text failed")?;
    println!("Done.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Main daemon loop
// ---------------------------------------------------------------------------

fn run_daemon(config: Config, preload: bool) -> Result<()> {
    log::info!("voicerouter starting up");

    let running = Arc::new(AtomicBool::new(true));
    let running_ctrlc = Arc::clone(&running);
    ctrlc::set_handler(move || {
        log::info!("received Ctrl+C — shutting down");
        running_ctrlc.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    let mut audio = AudioPipeline::new(&config.audio)
        .context("failed to open audio device")?;

    let initial_floor = audio::calibrate_silence(
        &mut audio,
        config.audio.sample_rate,
        config.audio.silence_threshold as f32,
    );
    let mut noise_tracker = NoiseTracker::new(
        initial_floor,
        config.audio.silence_threshold as f32,
        config.audio.sample_rate,
    );

    let mut monitor = HotkeyMonitor::new(&config.hotkey)
        .context("failed to open hotkey monitor")?;
    let router = Router::new(&config);

    let mut asr_engine: Option<AsrEngine> = if preload {
        log::info!("preloading ASR model '{}'", config.asr.model);
        Some(AsrEngine::new(&config.asr).context("preload failed")?)
    } else {
        None
    };

    let mut punctuator: Option<sherpa_rs::punctuate::Punctuation> = None;
    let mut recording_start: Option<Instant> = None;

    log::info!("ready — listening for hotkey '{}'", config.hotkey.key);

    while running.load(Ordering::SeqCst) {
        if let Some(event) = monitor.poll() {
            handle_event(
                event,
                &mut audio,
                &mut asr_engine,
                &mut punctuator,
                &config,
                &router,
                &mut recording_start,
                &mut noise_tracker,
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    log::info!("voicerouter stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

fn handle_event(
    event: HotkeyEvent,
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_rs::punctuate::Punctuation>,
    config: &Config,
    router: &Router,
    recording_start: &mut Option<Instant>,
    noise_tracker: &mut NoiseTracker,
) {
    match event {
        HotkeyEvent::StartRecording => {
            on_start_recording(audio, config, recording_start);
        }
        HotkeyEvent::StopRecording => {
            on_stop_recording(
                audio, asr_engine, punctuator, config, router,
                recording_start, noise_tracker,
            );
        }
        HotkeyEvent::CancelAndToggle => {
            log::info!("Auto short press — switching to toggle mode");
            audio.stop_recording();
            if let Err(e) = audio.start_recording() {
                log::error!("failed to restart recording for toggle: {e:#}");
                return;
            }
            *recording_start = Some(Instant::now());
        }
    }
}

fn on_start_recording(
    audio: &mut AudioPipeline,
    config: &Config,
    recording_start: &mut Option<Instant>,
) {
    log::info!("Recording started");
    beep_if(config, sound::beep_start);
    if let Err(e) = audio.start_recording() {
        log::error!("start_recording failed: {e:#}");
        beep_if(config, sound::beep_error);
        return;
    }
    *recording_start = Some(Instant::now());
}

/// Minimum recording duration in seconds — shorter clips are discarded.
const MIN_RECORDING_SECS: f32 = 0.3;

fn on_stop_recording(
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_rs::punctuate::Punctuation>,
    config: &Config,
    router: &Router,
    recording_start: &mut Option<Instant>,
    noise_tracker: &mut NoiseTracker,
) {
    let elapsed = recording_start
        .take()
        .map_or(0.0, |t| t.elapsed().as_secs_f32());
    log::info!("Recording stopped ({elapsed:.1}s)");
    beep_if(config, sound::beep_done);

    let samples = match validate_recording(audio, elapsed, noise_tracker) {
        Some(s) => s,
        None => return,
    };

    let text = match transcribe_and_process(
        samples, asr_engine, punctuator, config,
    ) {
        Some(t) => t,
        None => return,
    };

    if let Err(e) = router.dispatch(&text) {
        log::error!("dispatch failed: {e:#}");
        beep_if(config, sound::beep_error);
    }
}

/// Stop recording and validate: returns samples if audio is long enough
/// and loud enough to process.
fn validate_recording(
    audio: &mut AudioPipeline,
    elapsed: f32,
    noise_tracker: &mut NoiseTracker,
) -> Option<Vec<f32>> {
    let samples = audio.stop_recording()?;

    if elapsed < MIN_RECORDING_SECS {
        log::info!(
            "recording too short ({elapsed:.1}s < {MIN_RECORDING_SECS}s), discarding"
        );
        return None;
    }

    let rms = audio::compute_rms(&samples);
    let threshold = noise_tracker.threshold();
    if rms < threshold {
        // Only update noise floor from recordings that are pure silence.
        // Speech recordings would inflate the estimate.
        noise_tracker.update(&samples);
        log::info!(
            "recording is silence (RMS {rms:.4} < {threshold:.4}), discarding"
        );
        return None;
    }

    Some(samples)
}

/// Run ASR, punctuation restoration, and post-processing. Returns the
/// final text or `None` on failure / empty result.
fn transcribe_and_process(
    samples: Vec<f32>,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_rs::punctuate::Punctuation>,
    config: &Config,
) -> Option<String> {
    let text = match transcribe(samples, asr_engine, config) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            log::info!("Transcribed: (empty)");
            return None;
        }
        Err(e) => {
            log::error!("transcription failed: {e:#}");
            beep_if(config, sound::beep_error);
            return None;
        }
    };

    let text = add_punctuation(&text, punctuator, config);
    let processed = postprocess(&text, &config.postprocess);
    log::info!("Transcribed: {processed:?}");
    Some(processed)
}

// ---------------------------------------------------------------------------
// ASR helpers
// ---------------------------------------------------------------------------

/// Run ASR on `samples`, lazily initialising the engine if needed.
fn transcribe(
    samples: Vec<f32>,
    asr_engine: &mut Option<AsrEngine>,
    config: &Config,
) -> Result<String> {
    if asr_engine.is_none() {
        log::info!("initialising ASR engine (lazy)");
        *asr_engine = Some(
            AsrEngine::new(&config.asr).context("ASR engine init failed")?,
        );
    }
    let engine = asr_engine.as_mut().expect("engine was just set");
    engine.transcribe(&samples, config.audio.sample_rate)
}

/// Restore punctuation using ct-transformer model (lazy init).
fn add_punctuation(
    text: &str,
    punctuator: &mut Option<sherpa_rs::punctuate::Punctuation>,
    config: &Config,
) -> String {
    if !config.postprocess.restore_punctuation {
        return text.to_owned();
    }

    if punctuator.is_none() {
        *punctuator = init_punctuator(config);
    }

    match punctuator.as_mut() {
        Some(punc) => punc.add_punctuation(text),
        None => text.to_owned(),
    }
}

fn init_punctuator(
    config: &Config,
) -> Option<sherpa_rs::punctuate::Punctuation> {
    let model_path = match voicerouter::asr::models::expand_tilde(
        &config.postprocess.punctuation_model,
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("punctuation model path error: {e}");
            return None;
        }
    };
    let model_file = model_path.join("model.int8.onnx");
    if !model_file.exists() {
        log::warn!(
            "punctuation model not found at {}; skipping",
            model_file.display()
        );
        return None;
    }
    log::info!("loading punctuation model from {}", model_file.display());
    let cfg = sherpa_rs::punctuate::PunctuationConfig {
        model: model_file.to_string_lossy().into_owned(),
        ..Default::default()
    };
    match sherpa_rs::punctuate::Punctuation::new(cfg) {
        Ok(p) => Some(p),
        Err(e) => {
            log::error!("punctuation model init failed: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

/// Play a beep if sound feedback is enabled, logging any failure.
fn beep_if(config: &Config, f: fn() -> Result<()>) {
    if config.sound.feedback {
        if let Err(e) = f() {
            log::debug!("beep failed: {e}");
        }
    }
}
