# ygg-voicerouter: Rust Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite ygg-voiceim in Rust with sherpa-onnx, achieving 90% less RAM, streaming recognition, single-binary distribution, and voice router architecture.

**Architecture:** A voice router that captures audio via hotkey, runs streaming ASR through sherpa-onnx, then dispatches transcribed text to pluggable handlers (text injection by default, with extensible router for voice commands). Not an input method — works alongside fcitx5/rime without conflict.

**Tech Stack:** Rust, sherpa-rs (sherpa-onnx bindings), cpal (audio), evdev (hotkeys), nnnoiseless (denoise), toml/serde (config), clap (CLI), reqwest (LLM API)

**Reference:** Python prototype at `~/projects/voice-input/` (ygg-voiceim v0.x)

---

## File Structure

```
~/lab/ygg-voicerouter/
├── Cargo.toml
├── build.rs                    # Download sherpa-onnx models on first build
├── src/
│   ├── main.rs                 # CLI entry point (clap)
│   ├── config.rs               # TOML config loading
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── recorder.rs         # cpal audio capture
│   │   └── denoise.rs          # nnnoiseless RNNoise wrapper
│   ├── asr/
│   │   ├── mod.rs
│   │   ├── engine.rs           # sherpa-onnx ASR (streaming + offline)
│   │   └── models.rs           # Model download and path management
│   ├── hotkey/
│   │   ├── mod.rs
│   │   └── evdev.rs            # evdev hotkey monitor with debounce
│   ├── router/
│   │   ├── mod.rs              # Text router: match prefix → dispatch
│   │   ├── handler.rs          # Handler trait
│   │   └── handlers/
│   │       ├── mod.rs
│   │       ├── inject.rs       # Default: inject text to focused window
│   │       ├── llm.rs          # Optional: send to LLM API
│   │       └── shell.rs        # Optional: execute shell command
│   ├── inject/
│   │   ├── mod.rs
│   │   └── linux.rs            # wl-copy+ydotool / wtype / xdotool
│   ├── postprocess/
│   │   ├── mod.rs
│   │   ├── punctuation.rs      # Full-width conversion, punct mode
│   │   └── english_fix.rs      # Merge broken English tokens
│   └── sound.rs                # Beep generation and playback
├── tests/
│   ├── config_test.rs
│   ├── router_test.rs
│   ├── postprocess_test.rs
│   └── hotkey_test.rs
├── defaults/
│   ├── config.toml
│   └── hotwords.txt
├── scripts/
│   └── install.sh
├── README.md
└── LICENSE
```

---

## Task 1: Project Scaffold + Config

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/config.rs`
- Create: `defaults/config.toml`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Initialize Cargo project**

```bash
cd ~/lab/ygg-voicerouter
cargo init
```

- [ ] **Step 2: Write Cargo.toml with all dependencies**

```toml
[package]
name = "voicerouter"
version = "0.1.0"
edition = "2021"
description = "Voice router for Linux — offline ASR with pluggable handlers"
license = "MIT"

[dependencies]
sherpa-rs = "0.6"
cpal = "0.15"
evdev = "0.12"
nnnoiseless = "0.3"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["blocking", "json"] }
anyhow = "1"
log = "0.4"
env_logger = "0.11"
hound = "3.5"           # WAV reading for tests
dirs = "5"              # XDG directories
```

- [ ] **Step 3: Write default config.toml**

```toml
[hotkey]
key = "KEY_RIGHTALT"
mode = "auto"           # ptt | toggle | auto
hold_delay = 0.3

[audio]
sample_rate = 16000
channels = 1
silence_threshold = 0.01
silence_duration = 1.5
max_record_seconds = 30
denoise = true

[asr]
model = "paraformer-zh"  # paraformer-zh | whisper-large-v3
model_dir = "~/.cache/voicerouter/models"
streaming = true

[postprocess]
punct_mode = "strip_trailing"  # keep | strip_trailing | replace_space
fullwidth_punct = true
fix_english = true

[inject]
method = "auto"         # auto | clipboard-paste | wtype | xdotool

[router]
# Prefix-based routing rules
# [[router.rules]]
# trigger = "hey assistant"
# handler = "llm"
# [[router.rules]]
# trigger = "run"
# handler = "shell"

[llm]
enabled = false
# Credentials in .env file: LLM_BASE_URL, LLM_MODEL, LLM_API_KEY

[sound]
feedback = true
```

- [ ] **Step 4: Write config.rs with serde deserialization**

```rust
// src/config.rs
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub asr: AsrConfig,
    pub postprocess: PostprocessConfig,
    pub inject: InjectConfig,
    pub router: RouterConfig,
    pub llm: LlmConfig,
    pub sound: SoundConfig,
}

#[derive(Debug, Deserialize)]
pub struct HotkeyConfig {
    pub key: String,
    pub mode: String,
    #[serde(default = "default_hold_delay")]
    pub hold_delay: f64,
}

// ... (all sub-configs with defaults)

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("voicerouter/config.toml")
    }
}
```

- [ ] **Step 5: Write failing config test**

```rust
// tests/config_test.rs
use voicerouter::config::Config;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_load_default_config() {
    let mut f = NamedTempFile::new().unwrap();
    write!(f, include_str!("../defaults/config.toml")).unwrap();
    let cfg = Config::load(f.path()).unwrap();
    assert_eq!(cfg.hotkey.key, "KEY_RIGHTALT");
    assert_eq!(cfg.hotkey.mode, "auto");
    assert_eq!(cfg.asr.model, "paraformer-zh");
}
```

- [ ] **Step 6: Write minimal main.rs with clap**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "voicerouter", version, about = "Voice router for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    verbose: bool,
    #[arg(short, long)]
    config: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    Setup,
    Config { key: Option<String>, value: Option<String> },
    Service { action: String },
}
```

- [ ] **Step 7: Run tests, verify build**

```bash
cargo test
cargo build
```

- [ ] **Step 8: Commit**

```bash
git init && git add -A
git commit -m "feat: project scaffold with config and CLI"
```

---

## Task 2: Audio Capture + Denoise

**Files:**
- Create: `src/audio/mod.rs`
- Create: `src/audio/recorder.rs`
- Create: `src/audio/denoise.rs`

- [ ] **Step 1: Write recorder.rs — cpal audio capture**

Circular buffer, start/stop recording, return audio samples as `Vec<f32>`.

- [ ] **Step 2: Write denoise.rs — nnnoiseless wrapper**

Process audio through RNNoise. Input: `&[f32]` at 48kHz (nnnoiseless requirement). Handle 16kHz→48kHz resampling internally.

- [ ] **Step 3: Write mod.rs — public API**

```rust
pub struct AudioPipeline { ... }
impl AudioPipeline {
    pub fn start_recording(&mut self) -> anyhow::Result<()>;
    pub fn stop_recording(&mut self) -> Option<Vec<f32>>;
    pub fn rms(&self) -> f32;
}
```

- [ ] **Step 4: Test with real microphone**

```bash
cargo run -- --test-audio
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: audio capture with cpal + RNNoise denoise"
```

---

## Task 3: ASR Engine (sherpa-onnx)

**Files:**
- Create: `src/asr/mod.rs`
- Create: `src/asr/engine.rs`
- Create: `src/asr/models.rs`

- [ ] **Step 1: Write models.rs — model download and path management**

Auto-download Paraformer ONNX models from sherpa-onnx releases on first run.

- [ ] **Step 2: Write engine.rs — sherpa-onnx offline recognizer**

```rust
pub struct AsrEngine { ... }
impl AsrEngine {
    pub fn new(config: &AsrConfig) -> anyhow::Result<Self>;
    pub fn transcribe(&self, audio: &[f32]) -> anyhow::Result<String>;
}
```

Use `sherpa_rs::recognizer::OfflineRecognizer` with Paraformer model.

- [ ] **Step 3: Add streaming support**

```rust
pub struct StreamingAsrEngine { ... }
impl StreamingAsrEngine {
    pub fn new(config: &AsrConfig) -> anyhow::Result<Self>;
    pub fn feed_audio(&mut self, chunk: &[f32]);
    pub fn get_partial_result(&self) -> String;
    pub fn get_final_result(&mut self) -> String;
}
```

Use `sherpa_rs::recognizer::OnlineRecognizer` for streaming.

- [ ] **Step 4: Test with pre-recorded WAV**

```rust
#[test]
fn test_transcribe_silence() {
    let engine = AsrEngine::new(&default_config()).unwrap();
    let silence = vec![0.0f32; 16000 * 2];
    let result = engine.transcribe(&silence).unwrap();
    assert!(result.is_empty());
}
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: ASR engine with sherpa-onnx (offline + streaming)"
```

---

## Task 4: Hotkey Monitor

**Files:**
- Create: `src/hotkey/mod.rs`
- Create: `src/hotkey/evdev.rs`
- Test: `tests/hotkey_test.rs`

- [ ] **Step 1: Write evdev.rs — keyboard monitoring with debounce**

Port the Python evdev logic: find keyboards, PTT/toggle/auto modes, hold_delay, 50ms debounce for duplicate device events.

- [ ] **Step 2: Write hotkey_test.rs**

Test state machine transitions (PTT down/up, toggle on/off, auto short/long press).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: evdev hotkey monitor with PTT/toggle/auto modes"
```

---

## Task 5: Text Injection

**Files:**
- Create: `src/inject/mod.rs`
- Create: `src/inject/linux.rs`

- [ ] **Step 1: Write linux.rs — clipboard-paste + wtype + xdotool**

Port from Python: auto-detect method, clipboard polling, ydotool Ctrl+V, clipboard restore.

- [ ] **Step 2: Test injection**

```bash
cargo run -- --test-inject "你好世界"
```

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: text injection (clipboard-paste / wtype / xdotool)"
```

---

## Task 6: Post-Processing Pipeline

**Files:**
- Create: `src/postprocess/mod.rs`
- Create: `src/postprocess/punctuation.rs`
- Create: `src/postprocess/english_fix.rs`
- Test: `tests/postprocess_test.rs`

- [ ] **Step 1: Write punctuation.rs**

- `half_to_fullwidth()`: convert half-width punct to full-width near CJK
- `apply_punct_mode()`: keep / strip_trailing / replace_space

- [ ] **Step 2: Write english_fix.rs**

- `fix_broken_english()`: merge `T oken` → `Token`, `G P T` → `GPT`

- [ ] **Step 3: Write comprehensive tests**

```rust
#[test]
fn test_fullwidth_chinese() {
    assert_eq!(half_to_fullwidth("你好,世界"), "你好，世界");
}

#[test]
fn test_halfwidth_english_preserved() {
    assert_eq!(half_to_fullwidth("Hello, world"), "Hello, world");
}

#[test]
fn test_fix_token_split() {
    assert_eq!(fix_broken_english("T oken"), "Token");
}

#[test]
fn test_fix_acronym() {
    assert_eq!(fix_broken_english("G P T"), "GPT");
}
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: post-processing (fullwidth punct, english fix, punct modes)"
```

---

## Task 7: Voice Router

**Files:**
- Create: `src/router/mod.rs`
- Create: `src/router/handler.rs`
- Create: `src/router/handlers/mod.rs`
- Create: `src/router/handlers/inject.rs`
- Create: `src/router/handlers/llm.rs`
- Create: `src/router/handlers/shell.rs`
- Test: `tests/router_test.rs`

- [ ] **Step 1: Define Handler trait**

```rust
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, text: &str) -> anyhow::Result<()>;
}
```

- [ ] **Step 2: Write Router**

```rust
pub struct Router {
    rules: Vec<Rule>,
    default_handler: Box<dyn Handler>,
}

struct Rule {
    trigger: String,
    handler: Box<dyn Handler>,
}

impl Router {
    pub fn dispatch(&self, text: &str) -> anyhow::Result<()> {
        for rule in &self.rules {
            if text.starts_with(&rule.trigger) {
                let payload = text[rule.trigger.len()..].trim();
                return rule.handler.handle(payload);
            }
        }
        self.default_handler.handle(text)
    }
}
```

- [ ] **Step 3: Implement handlers**

- `inject.rs`: calls inject::linux::inject_text()
- `llm.rs`: reads .env, calls OpenAI-compatible API via reqwest
- `shell.rs`: runs command via `std::process::Command`

- [ ] **Step 4: Write router tests**

```rust
#[test]
fn test_default_handler() {
    let router = Router::new(vec![], MockHandler::new("default"));
    router.dispatch("hello").unwrap();
    // verify default handler was called
}

#[test]
fn test_prefix_routing() {
    let router = Router::new(
        vec![Rule { trigger: "run ".into(), handler: MockHandler::new("shell") }],
        MockHandler::new("default"),
    );
    router.dispatch("run ls -la").unwrap();
    // verify shell handler got "ls -la"
}
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: voice router with pluggable handlers (inject/llm/shell)"
```

---

## Task 8: Sound Feedback

**Files:**
- Create: `src/sound.rs`

- [ ] **Step 1: Write beep generation using cpal**

Generate sine wave beeps (880Hz start, 660Hz done, 330Hz error). Play non-blocking via cpal output stream.

- [ ] **Step 2: Commit**

```bash
git add -A && git commit -m "feat: audio feedback beeps via cpal"
```

---

## Task 9: App Orchestration + Main Loop

**Files:**
- Modify: `src/main.rs`
- Create: `src/app.rs` (if needed, or integrate in main.rs)

- [ ] **Step 1: Wire everything together**

```
main() → parse CLI → load config → init audio + ASR + hotkey + router
       → hotkey event loop:
           key_down → beep_start → start recording
           key_up   → beep_done → stop recording → denoise → ASR → postprocess → router.dispatch()
```

- [ ] **Step 2: Add --test-audio, --test-inject, --preload flags**

- [ ] **Step 3: Add setup subcommand (interactive)**

- [ ] **Step 4: Add service install/uninstall (systemd)**

- [ ] **Step 5: Full integration test — record, recognize, inject**

```bash
cargo run
# Press Right Alt, speak, release — text should appear
```

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: complete app with event loop, CLI, and service management"
```

---

## Task 10: Documentation + Distribution

**Files:**
- Create: `README.md`
- Create: `LICENSE`
- Create: `scripts/install.sh`
- Create: `.github/ISSUE_TEMPLATE/bug_report.md`

- [ ] **Step 1: Write README.md** (port and update from Python version)

- [ ] **Step 2: Write install.sh** (download binary + models + setup)

- [ ] **Step 3: Build release binary**

```bash
cargo build --release
ls -lh target/release/voicerouter
```

- [ ] **Step 4: Create GitHub repo and push**

```bash
gh repo create yggdjc/ygg-voicerouter --public
git remote add origin git@github.com:yggdjc/ygg-voicerouter.git
git push -u origin main
```

- [ ] **Step 5: Tag v0.1.0**

```bash
git tag -a v0.1.0 -m "v0.1.0: Rust rewrite with sherpa-onnx and voice router"
git push origin v0.1.0
```

---

## Key Design Decisions

### Hotwords Limitation
sherpa-onnx hotwords only work with **transducer** models, not Paraformer. Two options:
1. Use Paraformer (best Chinese) without hotwords
2. Use Zipformer transducer (slightly less accurate) with hotwords

**Decision:** Default to Paraformer for accuracy. Hotwords support as opt-in with transducer model.

### Streaming vs Batch
sherpa-onnx supports both. Streaming gives ~200ms partial results.

**Decision:** Streaming by default (config `streaming = true`). Batch mode available for simpler use cases.

### Voice Router Default Behavior
Without any router rules configured, 100% of text goes to the inject handler (text injection). Router is invisible until the user adds rules.

**Decision:** Zero-config for basic voice input. Router rules are opt-in.

---

## Expected Results After Completion

| Metric | Python (ygg-voiceim) | Rust (ygg-voicerouter) |
|--------|---------------------|----------------------|
| RAM | 3.4 GB | ~200-300 MB |
| VRAM | 2.2 GB | 0 (CPU ONNX) |
| Startup | ~20s | ~2s |
| Latency | ~1s (batch) | ~200ms (streaming) |
| Binary | ~10GB (.venv) | ~50-100 MB |
| Dependencies | Python + PyTorch + CUDA | None (single binary) |
| Streaming | No | Yes |
| Distribution | uv sync | Copy binary |
