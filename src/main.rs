//! voicerouter CLI entry point.

mod service;
mod setup;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use voicerouter::audio::{self, AudioPipeline};
use voicerouter::config::Config;

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
    /// Download model files for the configured (or specified) model.
    Download {
        /// Model to download. Defaults to the configured ASR model + punctuation.
        /// Options: paraformer-zh, funasr-nano, whisper-tiny-en, whisper-base-en, ct-punc, all
        model: Option<String>,
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
        Some(Commands::Download { model }) => setup::download(&config, model.as_deref()),
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
// Main daemon loop (actor-based)
// ---------------------------------------------------------------------------

fn run_daemon(config: Config, preload: bool) -> Result<()> {
    log::info!("voicerouter starting up (actor mode)");

    use voicerouter::actor::{Actor, Bus, Message};
    use voicerouter::core_actor::CoreActor;
    use voicerouter::hotkey::HotkeyActor;
    use voicerouter::ipc::IpcActor;
    use voicerouter::pipeline::stage::Stage;
    use voicerouter::pipeline::{self, PipelineActor};
    use voicerouter::tts::TtsActor;

    // Build pipeline stages from config.
    let stage_configs = config.effective_pipeline_stages();
    let stages: Vec<Stage> = stage_configs
        .iter()
        .map(|sc| {
            let handler = pipeline::handlers::build_handler(&sc.handler, &config);
            let condition = sc.condition.as_ref().map(|c| parse_condition(c));
            let mut params = std::collections::HashMap::new();
            if let Some(ref cmd) = sc.command {
                params.insert("command".into(), cmd.clone());
            }
            if let Some(ref url) = sc.url {
                params.insert("url".into(), url.clone());
            }
            if let Some(ref method) = sc.method {
                params.insert("method".into(), method.clone());
            }
            if let Some(ref body) = sc.body {
                params.insert("body".into(), body.clone());
            }
            Stage {
                name: sc.name.clone(),
                handler,
                condition,
                after: sc.after.clone(),
                params,
                timeout: std::time::Duration::from_secs(sc.timeout),
            }
        })
        .collect();

    // Create channels for each actor.
    let (hotkey_tx, hotkey_rx) = crossbeam::channel::bounded::<Message>(32);
    let (core_tx, core_rx) = crossbeam::channel::bounded::<Message>(32);
    let (pipeline_tx, pipeline_rx) = crossbeam::channel::bounded::<Message>(32);
    let (tts_tx, tts_rx) = crossbeam::channel::bounded::<Message>(32);
    let (bus_tx, bus_rx) = crossbeam::channel::bounded::<Message>(128);

    // Set up bus subscriptions.
    let mut bus = Bus::new();
    bus.subscribe("StartListening", core_tx.clone());
    bus.subscribe("StopListening", core_tx.clone());
    bus.subscribe("StopListening", hotkey_tx.clone());
    bus.subscribe("CancelRecording", core_tx.clone());
    bus.subscribe("MuteInput", core_tx.clone());
    bus.subscribe("UnmuteInput", core_tx.clone());
    bus.subscribe("Transcript", pipeline_tx.clone());
    bus.subscribe("PipelineInput", pipeline_tx.clone());
    bus.subscribe("SpeakRequest", tts_tx.clone());
    bus.subscribe("SpeakDone", core_tx.clone());
    bus.subscribe("Shutdown", hotkey_tx.clone());
    bus.subscribe("Shutdown", core_tx.clone());
    bus.subscribe("Shutdown", pipeline_tx.clone());
    bus.subscribe("Shutdown", tts_tx.clone());

    // IPC subscriptions only when enabled.
    let ipc_channels = if config.ipc.enabled {
        let (ipc_tx, ipc_rx) = crossbeam::channel::bounded::<Message>(32);
        bus.subscribe("Transcript", ipc_tx.clone());
        bus.subscribe("PipelineOutput", ipc_tx.clone());
        bus.subscribe("Shutdown", ipc_tx.clone());
        Some((ipc_tx, ipc_rx))
    } else {
        None
    };

    // Spawn bus router thread.
    let bus_handle = std::thread::Builder::new()
        .name("bus".into())
        .spawn(move || {
            for msg in bus_rx {
                if matches!(msg, Message::Shutdown) {
                    bus.publish(msg);
                    break;
                }
                bus.publish(msg);
            }
        })?;

    // Spawn actors.
    let hotkey_actor = HotkeyActor::new(config.hotkey.clone());
    let core_actor = CoreActor::new(config.clone(), preload);

    let bus_tx_hotkey = bus_tx.clone();
    let bus_tx_core = bus_tx.clone();
    let bus_tx_pipeline = bus_tx.clone();
    let bus_tx_tts = bus_tx.clone();

    let hotkey_handle = std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || hotkey_actor.run(hotkey_rx, bus_tx_hotkey))?;

    let core_handle = std::thread::Builder::new()
        .name("core".into())
        .spawn(move || core_actor.run(core_rx, bus_tx_core))?;

    let pipeline_actor = PipelineActor::new(stages);
    let pipeline_handle = std::thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || pipeline_actor.run(pipeline_rx, bus_tx_pipeline))?;

    let tts_actor = TtsActor::new(config.tts.clone());
    let tts_handle = std::thread::Builder::new()
        .name("tts".into())
        .spawn(move || tts_actor.run(tts_rx, bus_tx_tts))?;

    let ipc_handle = if let Some((_ipc_tx, ipc_rx)) = ipc_channels {
        let ipc_actor = IpcActor::new(config.ipc.clone());
        let bus_tx_ipc = bus_tx.clone();
        Some(std::thread::Builder::new()
            .name("ipc".into())
            .spawn(move || ipc_actor.run(ipc_rx, bus_tx_ipc))?)
    } else {
        log::info!("IPC disabled");
        None
    };

    // Set up Ctrl+C to send Shutdown.
    let bus_tx_ctrlc = bus_tx.clone();
    ctrlc::set_handler(move || {
        log::info!("received Ctrl+C — shutting down");
        bus_tx_ctrlc.send(Message::Shutdown).ok();
    })
    .context("failed to set Ctrl+C handler")?;

    log::info!("voicerouter ready — all actors running");

    // Wait for actors to finish with 5s global timeout.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut handles: Vec<std::thread::JoinHandle<()>> =
        vec![hotkey_handle, core_handle, pipeline_handle, tts_handle];
    if let Some(h) = ipc_handle {
        handles.push(h);
    }
    handles.push(bus_handle);
    for handle in handles {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            log::warn!("shutdown timeout exceeded, force-exiting");
            break;
        }
        let _ = handle.join();
    }

    log::info!("voicerouter stopped");
    Ok(())
}

fn parse_condition(s: &str) -> voicerouter::pipeline::stage::Condition {
    use voicerouter::pipeline::stage::Condition;
    if let Some(prefix) = s.strip_prefix("starts_with:") {
        Condition::StartsWith(prefix.to_string())
    } else if let Some(val) = s.strip_prefix("output_eq:") {
        Condition::OutputEq(val.to_string())
    } else if let Some(val) = s.strip_prefix("output_contains:") {
        Condition::OutputContains(val.to_string())
    } else {
        Condition::Always
    }
}
