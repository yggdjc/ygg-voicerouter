//! `voicerouter setup` — check tools, model files, create default config.

use anyhow::{Context, Result};

use voicerouter::config::Config;
use voicerouter::inject::linux::is_command_available;

pub fn run(config: &Config) -> Result<()> {
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
        let found = is_command_available(tool);
        let status = if found { "OK" } else { "MISSING" };
        println!("  [{status:^7}] {tool:<12} — {description}");
    }
    println!();
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
        println!("  Run `voicerouter setup` after placing model files,");
        println!("  or check the docs for download instructions.");
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
        std::fs::create_dir_all(parent).with_context(|| {
            format!("creating config directory: {}", parent.display())
        })?;
    }

    let default_toml = include_str!("../config.default.toml");
    std::fs::write(&path, default_toml).with_context(|| {
        format!("writing default config: {}", path.display())
    })?;

    println!("Created default config: {}", path.display());
    Ok(())
}
