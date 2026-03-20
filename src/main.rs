//! voicerouter CLI entry point.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use voicerouter::asr::AsrEngine;
use voicerouter::audio::AudioPipeline;
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
    /// Get or set a config value.
    Config {
        key: Option<String>,
        value: Option<String>,
    },
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    let config = Config::load(cli.config.as_deref())?;
    log::debug!("loaded config: {config:?}");

    // One-shot test/utility flags take priority over subcommands.
    if cli.test_audio {
        return run_test_audio(&config);
    }
    if let Some(text) = cli.test_inject {
        return run_test_inject(&text, &config);
    }

    match cli.command {
        None => run_daemon(config, cli.preload),
        Some(Commands::Setup) => run_setup(&config),
        Some(Commands::Config { key, value }) => run_config(key.as_deref(), value.as_deref()),
        Some(Commands::Service { action }) => run_service(&action),
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
    let overall_rms = if samples.is_empty() {
        0.0_f32
    } else {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    };

    println!("Done. Captured {} samples. Overall RMS: {overall_rms:.4}", samples.len());
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

    // Set up Ctrl+C handler.
    let running = Arc::new(AtomicBool::new(true));
    let running_ctrlc = Arc::clone(&running);
    ctrlc::set_handler(move || {
        log::info!("received Ctrl+C — shutting down");
        running_ctrlc.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    let mut audio = AudioPipeline::new(&config.audio)
        .context("failed to open audio device")?;

    let mut monitor = HotkeyMonitor::new(&config.hotkey)
        .context("failed to open hotkey monitor")?;

    let router = Router::new(&config);

    // Optionally pre-load the ASR model before entering the loop.
    let mut asr_engine: Option<AsrEngine> = if preload {
        log::info!("preloading ASR model '{}'", config.asr.model);
        Some(AsrEngine::new(&config.asr).context("ASR engine init failed (preload)")?)
    } else {
        None
    };

    let mut recording_start: Option<Instant> = None;

    log::info!("ready — listening for hotkey '{}'", config.hotkey.key);

    while running.load(Ordering::SeqCst) {
        if let Some(event) = monitor.poll() {
            handle_event(
                event,
                &mut audio,
                &mut asr_engine,
                &config,
                &router,
                &mut recording_start,
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    log::info!("voicerouter stopped");
    Ok(())
}

/// Dispatch a single hotkey event through the full pipeline.
fn handle_event(
    event: HotkeyEvent,
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    config: &Config,
    router: &Router,
    recording_start: &mut Option<Instant>,
) {
    match event {
        HotkeyEvent::StartRecording => on_start_recording(audio, config, recording_start),
        HotkeyEvent::StopRecording => {
            on_stop_recording(audio, asr_engine, config, router, recording_start);
        }
    }
}

fn on_start_recording(
    audio: &mut AudioPipeline,
    config: &Config,
    recording_start: &mut Option<Instant>,
) {
    log::info!("Recording started");
    if config.sound.feedback {
        sound::beep_start().ok();
    }
    if let Err(e) = audio.start_recording() {
        log::error!("start_recording failed: {e:#}");
        if config.sound.feedback {
            sound::beep_error().ok();
        }
        return;
    }
    *recording_start = Some(Instant::now());
}

fn on_stop_recording(
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    config: &Config,
    router: &Router,
    recording_start: &mut Option<Instant>,
) {
    let elapsed = recording_start.take().map_or(0.0, |t| t.elapsed().as_secs_f32());
    log::info!("Recording stopped ({elapsed:.1}s)");

    let Some(samples) = audio.stop_recording() else {
        log::warn!("stop_recording returned no data");
        return;
    };

    let text = match transcribe(samples, asr_engine, config) {
        Ok(t) => t,
        Err(e) => {
            log::error!("transcription failed: {e:#}");
            if config.sound.feedback {
                sound::beep_error().ok();
            }
            return;
        }
    };

    if text.is_empty() {
        log::info!("Transcribed: (empty)");
        if config.sound.feedback {
            sound::beep_done().ok();
        }
        return;
    }

    let processed = postprocess(&text, &config.postprocess);
    log::info!("Transcribed: {processed:?}");
    log::info!("Dispatching to handler");

    if let Err(e) = router.dispatch(&processed) {
        log::error!("dispatch failed: {e:#}");
        if config.sound.feedback {
            sound::beep_error().ok();
        }
        return;
    }

    if config.sound.feedback {
        sound::beep_done().ok();
    }
}

/// Run ASR on `samples`, lazily initialising the engine if not yet created.
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

// ---------------------------------------------------------------------------
// setup subcommand
// ---------------------------------------------------------------------------

fn run_setup(config: &Config) -> Result<()> {
    println!("voicerouter setup check");
    println!();
    check_tools();
    check_model(config);
    ensure_default_config()?;
    Ok(())
}

fn check_tools() {
    let tools = [
        ("wl-copy", "clipboard paste on Wayland"),
        ("wtype",   "Wayland typing"),
        ("xdotool", "X11 typing"),
        ("ydotool", "universal keystroke injection"),
        ("ffmpeg",  "audio format conversion (optional)"),
    ];
    println!("Tool availability:");
    for (tool, description) in &tools {
        let found = which_found(tool);
        let status = if found { "OK" } else { "MISSING" };
        println!("  [{status:^7}] {tool:<12} — {description}");
    }
    println!();
}

fn which_found(tool: &str) -> bool {
    std::process::Command::new("which")
        .arg(tool)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn check_model(config: &Config) {
    use voicerouter::asr::models::{expand_tilde, model_files_exist};

    let model_name = &config.asr.model;
    let model_dir = expand_tilde(&config.asr.model_dir).unwrap_or_default();
    let present = model_files_exist(model_name, &model_dir).unwrap_or(false);
    let status = if present { "OK     " } else { "MISSING" };
    println!("ASR model files:");
    println!("  [{status}] {model_name} in {}", model_dir.display());
    if !present {
        println!("  Run `voicerouter setup` after placing model files, or check");
        println!("  the docs for download instructions.");
    }
    println!();
}

fn ensure_default_config() -> Result<()> {
    let Some(path) = Config::default_path() else {
        println!("Could not determine config directory — skipping.");
        return Ok(());
    };

    if path.exists() {
        println!("Config file already exists: {}", path.display());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating config directory: {}", parent.display()))?;
    }

    let default_toml = include_str!("../config.default.toml");
    std::fs::write(&path, default_toml)
        .with_context(|| format!("writing default config: {}", path.display()))?;

    println!("Created default config: {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// config subcommand
// ---------------------------------------------------------------------------

fn run_config(key: Option<&str>, value: Option<&str>) -> Result<()> {
    match (key, value) {
        (None, _) => {
            println!("Usage: voicerouter config <key> [<value>]");
            println!("Config file: {}", Config::default_path()
                .map_or_else(|| "(unknown)".to_owned(), |p| p.display().to_string()));
        }
        (Some(k), None) => {
            println!("Reading config key '{k}' — not yet implemented.");
        }
        (Some(k), Some(v)) => {
            println!("Setting config key '{k}' = '{v}' — not yet implemented.");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// service subcommand
// ---------------------------------------------------------------------------

const SERVICE_NAME: &str = "voicerouter";
const SERVICE_UNIT: &str = "voicerouter.service";

fn run_service(action: &str) -> Result<()> {
    match action {
        "install" => service_install(),
        "uninstall" => service_uninstall(),
        "start" | "stop" | "status" | "restart" => systemctl(action),
        other => {
            anyhow::bail!(
                "unknown service action {other:?}. \
                 Valid: install, uninstall, start, stop, restart, status"
            );
        }
    }
}

fn service_install() -> Result<()> {
    let unit_dir = systemd_unit_dir()?;
    std::fs::create_dir_all(&unit_dir)
        .with_context(|| format!("creating unit dir: {}", unit_dir.display()))?;

    let binary = std::env::current_exe().context("cannot determine current binary path")?;
    let unit_content = format!(
        "[Unit]\n\
         Description=voicerouter — offline voice router\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        binary = binary.display(),
    );

    let unit_path = unit_dir.join(SERVICE_UNIT);
    std::fs::write(&unit_path, unit_content)
        .with_context(|| format!("writing service file: {}", unit_path.display()))?;

    println!("Installed: {}", unit_path.display());

    // Reload and enable.
    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", SERVICE_NAME])?;
    println!("Service enabled. Use `voicerouter service start` to start it.");
    Ok(())
}

fn service_uninstall() -> Result<()> {
    // Stop and disable — ignore errors (service may not be running).
    let _ = run_systemctl(&["--user", "stop", SERVICE_NAME]);
    let _ = run_systemctl(&["--user", "disable", SERVICE_NAME]);

    let unit_path = systemd_unit_dir()?.join(SERVICE_UNIT);
    if unit_path.exists() {
        std::fs::remove_file(&unit_path)
            .with_context(|| format!("removing unit file: {}", unit_path.display()))?;
        println!("Removed: {}", unit_path.display());
    } else {
        println!("Unit file not found — nothing to remove.");
    }

    let _ = run_systemctl(&["--user", "daemon-reload"]);
    Ok(())
}

fn systemctl(action: &str) -> Result<()> {
    run_systemctl(&["--user", action, SERVICE_NAME])
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .args(args)
        .status()
        .context("failed to run systemctl")?;

    if !status.success() {
        anyhow::bail!("systemctl {} exited with {}", args.join(" "), status);
    }
    Ok(())
}

fn systemd_unit_dir() -> Result<std::path::PathBuf> {
    let config_dir = dirs::config_dir().context("cannot determine config directory")?;
    Ok(config_dir.join("systemd").join("user"))
}
