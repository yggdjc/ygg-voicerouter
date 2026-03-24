//! `voicerouter setup` — check tools, model files, create default config.
//! `voicerouter download` — download ASR and punctuation models.

use std::process::Command;

use anyhow::{bail, Context, Result};

use voicerouter::asr::models::{expand_tilde, model_files_exist, model_info};
use voicerouter::config::Config;
use voicerouter::inject::linux::is_command_available;

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

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
        ("wtype", "Wayland typing"),
        ("xdotool", "X11 typing"),
        ("ydotool", "universal keystroke injection"),
        ("curl", "model download"),
        ("tar", "model extraction"),
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
    let model_dir = expand_tilde(&config.asr.model_dir).unwrap_or_default();

    // Check ASR model
    let asr_model = &config.asr.model;
    let asr_present = model_files_exist(asr_model, &model_dir).unwrap_or(false);
    let status = if asr_present { "OK     " } else { "MISSING" };
    println!("Models:");
    println!("  [{status}] {asr_model} (ASR)");

    // Check punctuation model
    let punc_present = model_files_exist("ct-punc", &model_dir).unwrap_or(false);
    let status = if punc_present { "OK     " } else { "MISSING" };
    println!("  [{status}] ct-punc (punctuation)");

    if !asr_present || !punc_present {
        println!();
        println!("  Missing models can be downloaded with:");
        println!("    voicerouter download");
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

// ---------------------------------------------------------------------------
// download
// ---------------------------------------------------------------------------

/// Download model files. If `model` is None, downloads the configured ASR
/// model + ct-punc. If "all", downloads all supported models.
///
/// Options: paraformer-zh, funasr-nano, whisper-tiny-en, whisper-base-en,
///          ct-punc, silero-vad, 3dspeaker, all
pub fn download(config: &Config, model: Option<&str>) -> Result<()> {
    if !is_command_available("curl") {
        bail!("curl is required for downloading models. Install it first.");
    }

    let model_dir = expand_tilde(&config.asr.model_dir)?;
    std::fs::create_dir_all(&model_dir)?;

    let models_to_download: Vec<&str> = match model {
        Some("all") => vec![
            "paraformer-zh",
            "funasr-nano",
            "whisper-tiny-en",
            "whisper-base-en",
            "ct-punc",
            "silero-vad",
            "3dspeaker",
        ],
        Some(name) => vec![name],
        None => vec![&config.asr.model, "ct-punc"],
    };

    // tar is only needed when downloading archive-based models.
    let needs_tar = models_to_download
        .iter()
        .any(|n| !matches!(*n, "silero-vad" | "3dspeaker"));

    if needs_tar && !is_command_available("tar") {
        bail!("tar is required for extracting archive models. Install it first.");
    }

    for name in &models_to_download {
        if model_files_exist(name, &model_dir).unwrap_or(false) {
            println!("[skip] {name} — already installed");
            continue;
        }

        let info = model_info(name, &model_dir)
            .with_context(|| format!("unknown model '{name}'"))?;

        // Find the first file with a non-empty URL.
        let Some(first_file) = info.files.iter().find(|f| !f.url.is_empty()) else {
            println!("[skip] {name} — no download URL");
            continue;
        };

        println!("[download] {name}");

        // Single-file ONNX models download directly; archive models extract.
        if first_file.url.ends_with(".onnx") {
            let dest_dir = model_dir.join(name);
            std::fs::create_dir_all(&dest_dir)
                .with_context(|| format!("creating directory {}", dest_dir.display()))?;
            download_file(&first_file.url, &first_file.local_path)?;
        } else {
            download_and_extract(&first_file.url, name, &model_dir)?;
        }

        println!("[ok] {name} installed");
    }

    println!();
    println!("Done. Run `voicerouter setup` to verify.");
    Ok(())
}

/// Download a single file to `dest` using curl.
fn download_file(url: &str, dest: &std::path::Path) -> Result<()> {
    let file_name = url.rsplit('/').next().unwrap_or("model.onnx");
    println!("  Downloading {file_name}...");
    let status = Command::new("curl")
        .args(["-L", "--progress-bar", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .context("failed to run curl")?;

    if !status.success() {
        bail!("curl failed with status {status}");
    }
    Ok(())
}

/// Download a tar.bz2 archive and extract it into model_dir/model_name.
fn download_and_extract(
    url: &str,
    model_name: &str,
    model_dir: &std::path::Path,
) -> Result<()> {
    let archive_name = url
        .rsplit('/')
        .next()
        .unwrap_or("model.tar.bz2");
    let archive_path = model_dir.join(archive_name);
    let target_dir = model_dir.join(model_name);

    // Download
    println!("  Downloading {archive_name}...");
    let status = Command::new("curl")
        .args(["-L", "--progress-bar", "-o"])
        .arg(&archive_path)
        .arg(url)
        .status()
        .context("failed to run curl")?;

    if !status.success() {
        bail!("curl failed with status {status}");
    }

    // Extract
    println!("  Extracting...");
    let status = Command::new("tar")
        .args(["-xjf"])
        .arg(&archive_path)
        .arg("-C")
        .arg(model_dir)
        .status()
        .context("failed to run tar")?;

    if !status.success() {
        bail!("tar failed with status {status}");
    }

    // Rename extracted directory to model_name.
    // Archives typically extract to sherpa-onnx-<name>-<date>/
    let extracted_stem = archive_name
        .strip_suffix(".tar.bz2")
        .unwrap_or(archive_name);
    let extracted_dir = model_dir.join(extracted_stem);

    if extracted_dir.exists() && extracted_dir != target_dir {
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir)
                .with_context(|| format!("removing old {}", target_dir.display()))?;
        }
        std::fs::rename(&extracted_dir, &target_dir).with_context(|| {
            format!(
                "renaming {} -> {}",
                extracted_dir.display(),
                target_dir.display()
            )
        })?;
    }

    // Clean up archive
    let _ = std::fs::remove_file(&archive_path);

    Ok(())
}
