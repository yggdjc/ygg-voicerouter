# voicerouter

**Voice router for Linux** — Offline speech recognition with pluggable handlers. Single binary, ~200MB RAM, CPU-only.

Transform your voice into text on any Linux system without sending audio to the cloud. Write, command, and automate entirely offline.

## Features

- **Offline speech recognition** via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Paraformer model)
- **Multiple hotkey modes**: push-to-talk, toggle, and auto detection
- **Audio denoising** with RNNoise for cleaner transcription
- **Text injection** to any window (Wayland + X11 support)
- **Voice router**: prefix-based routing to handlers (inject, LLM, shell)
- **CJK-aware post-processing**: fullwidth punctuation, broken token fixes
- **Audio feedback**: beep confirmation for recognition events
- **systemd service support** for auto-start
- **Zero-config** for basic voice input

## Requirements

- **Linux** (Ubuntu 22.04+, Fedora 38+, or similar)
- **PulseAudio** or **PipeWire** (audio server)
- **One of**:
  - Wayland: `wl-copy` + `wtype` (or `ydotool`)
  - X11: `xdotool`

Audio format conversion (optional): `ffmpeg`

## Installation

### From Release (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/yggdjc/ygg-voicerouter/main/scripts/install.sh | bash
```

This downloads the pre-built binary for your architecture (x86_64 or aarch64), installs to `~/.local/bin/`, and prompts for model download.

### From Source

```bash
cargo build --release
./target/release/voicerouter setup
```

Requires Rust 1.70+ and a C compiler.

### Model Download

voicerouter requires the Paraformer model. After installation, run:

```bash
voicerouter setup
```

This checks for model files and provides download instructions if missing. Models are cached in `~/.cache/voicerouter/models/`.

For manual download, visit [sherpa-onnx releases](https://github.com/k2-fsa/sherpa-onnx/releases) and download the `paraformer-zh` model files.

## Quick Start

1. **Install dependencies** (Wayland example):
   ```bash
   # Ubuntu/Debian
   sudo apt install wl-clipboard wtype

   # Fedora
   sudo dnf install wl-clipboard wtype
   ```

2. **Setup voicerouter**:
   ```bash
   voicerouter setup
   ```

3. **Start the daemon**:
   ```bash
   voicerouter
   ```

4. **Use it**:
   - Press and hold **Right Alt** — indicator beep
   - Speak your text
   - Release **Right Alt** — transcription beep, text appears in focused window

Try with a text editor, browser search, or chat app. Works anywhere you can type.

### Hotkey Modes

Configure in `~/.config/voicerouter/config.toml`:

- **`ptt`** (push-to-talk): Hold key to record, release to transcribe. Default.
- **`toggle`**: Press once to start, again to stop. Useful for hands-free.
- **`auto`**: Detect speech automatically. Records silence-boundary detection.

```toml
[hotkey]
mode = "auto"       # Options: ptt, toggle, auto
hold_delay = 0.3    # Debounce for ptt mode (seconds)
```

## Configuration

Configuration is read from `~/.config/voicerouter/config.toml`. A default is created on first run.

Key sections:

| Section | Purpose |
|---------|---------|
| `[hotkey]` | Hotkey binding, mode, hold delay |
| `[audio]` | Sample rate, denoise, silence thresholds |
| `[asr]` | ASR model path and streaming mode |
| `[postprocess]` | Punctuation handling, fullwidth, English fixes |
| `[inject]` | Text injection method (auto-detect or explicit) |
| `[router]` | Voice routing rules (see below) |
| `[llm]` | LLM handler config (optional) |
| `[sound]` | Audio feedback beeps |

See [defaults/config.toml](defaults/config.toml) for complete defaults.

### Audio Configuration

```toml
[audio]
sample_rate = 16000              # ASR model sample rate
channels = 1                     # Mono input
silence_threshold = 0.01         # RMS threshold for silence
silence_duration = 1.5           # Seconds of silence to end recording
max_record_seconds = 30          # Safety limit for recording
denoise = true                   # Enable RNNoise denoising
```

### Injection Method

```toml
[inject]
# Options: auto (detect), clipboard_paste, wtype, xdotool
method = "auto"
```

- **`auto`**: Tries wtype on Wayland, xdotool on X11, falls back to clipboard
- **`clipboard_paste`**: Copy to clipboard, Ctrl+V
- **`wtype`**: Direct Wayland keystroke injection
- **`xdotool`**: X11 XTest keystroke injection

## Voice Router Rules

Route recognized text to different handlers based on prefix matching.

```toml
[[router.rules]]
trigger = "^搜索"          # Regex: "search" prefix in Chinese
handler = "browser_search"

[[router.rules]]
trigger = "^remind me"     # English
handler = "reminder"

[[router.rules]]
# Default: no match → text injected to focused window
handler = "inject"
```

### Built-in Handlers

| Handler | Action |
|---------|--------|
| `inject` | Inject recognized text to focused window |
| `shell` | Execute shell command (e.g., `eval ${text}`) |
| `llm` | Send to LLM (requires config) |

Example shell handler:

```toml
[[router.rules]]
trigger = "^play"
handler = "shell"
# Expands to: shell_exec("mpv music.mp3")
```

Example LLM handler (requires OpenAI API key):

```toml
[llm]
enabled = true
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o-mini"

[[router.rules]]
trigger = "^ask"
handler = "llm"
# Extracts question, sends to LLM, speaks result (if TTS enabled)
```

## Service Management

Run voicerouter as a systemd user service:

```bash
# Install and enable
voicerouter service install

# Start/stop
voicerouter service start
voicerouter service stop

# Check status
voicerouter service status

# Uninstall
voicerouter service uninstall
```

Logs: `journalctl --user -u voicerouter -f`

## Troubleshooting

### Audio Input Not Working

Test microphone input:

```bash
voicerouter --test-audio
```

Should display RMS levels. If stuck at 0.0, check:
- PulseAudio/PipeWire running: `pactl list short sinks`
- Microphone selected: `pavucontrol` (GUI) or `pactl set-default-source`
- Volume: `alsamixer`

### Text Not Injecting

Test text injection:

```bash
voicerouter --test-inject "Hello, world!"
```

Should inject text to focused window. If nothing happens:
- **Wayland**: Ensure wtype/ydotool installed; check `echo $WAYLAND_DISPLAY`
- **X11**: Ensure xdotool installed; check `echo $DISPLAY`
- **Clipboard**: Try manually pasting: `echo "test" | wl-copy && Ctrl+V`

### Poor Transcription

- **Noisy environment**: Enable denoising in config:
  ```toml
  [audio]
  denoise = true
  ```
- **Wrong model**: Check ASR model matches language:
  ```bash
  voicerouter config asr.model
  # Options: paraformer-zh, paraformer-en, etc.
  ```
- **Low volume**: Check input levels: `voicerouter --test-audio`

### Service Not Starting

```bash
journalctl --user -u voicerouter -n 50
```

Common issues:
- Dependencies missing (see [Requirements](#requirements))
- Model files not found: run `voicerouter setup`
- Audio device unavailable: wait for PulseAudio/PipeWire startup

## Performance Comparison

| Metric | Python (ygg-voiceim) | Rust (ygg-voicerouter) |
|--------|---------------------|----------------------|
| RAM | 3.4 GB | ~200-300 MB |
| VRAM | 2.2 GB | 0 (CPU ONNX) |
| Startup | ~20s | ~2s |
| Binary | ~10GB (.venv) | ~50-100 MB |
| Transcription | Real-time | Real-time |

Rust version is 10x lighter and 10x faster to start.

## Development

### Building from Source

```bash
git clone https://github.com/yggdjc/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release
./target/release/voicerouter setup
./target/release/voicerouter
```

### Running Tests

```bash
cargo test
```

### Code Quality

```bash
# Format and lint
cargo fmt
cargo clippy
```

### Project Structure

```
src/
├── main.rs              # CLI entry point and daemon loop
├── lib.rs               # Public library interface
├── asr/                 # Speech recognition (sherpa-onnx)
├── audio/               # Audio capture and denoising
├── hotkey/              # Hotkey monitoring (evdev)
├── inject/              # Text injection (Wayland/X11)
├── router/              # Voice routing and handlers
├── postprocess/         # Text post-processing
└── sound.rs             # Audio feedback
```

## License

MIT License. See [LICENSE](LICENSE) for details.

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/something`)
3. Commit changes with clear messages
4. Open a pull request
5. Ensure tests pass: `cargo test && cargo clippy`

## Acknowledgments

- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) — Speech recognition
- [RNNoise](https://github.com/xiph/rnnoise) — Audio denoising
- [evdev](https://github.com/ndarilek/rdev) — Hotkey monitoring
- Community feedback and testing
