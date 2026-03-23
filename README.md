# voicerouter

**Voice interaction framework for Linux** — offline speech recognition with actor-based architecture, composable pipeline, IPC, and extensible handlers. Single binary, CPU-only.

[中文文档](README_zh.md)

## Features

- **Offline ASR** via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) — Paraformer (default) or FunASR Nano
- **Actor architecture** — each component runs on its own thread with message-passing via central bus
- **Composable pipeline** — chain handlers with conditions, or build DAG workflows with fan-out
- **Built-in handlers** — inject (type text), shell (run commands), pipe (stdin/stdout), http (API calls), transform (regex/template)
- **IPC** — Unix socket with JSON-RPC 2.0 for external tool integration
- **TTS** — text-to-speech actor with cpal playback (sherpa-onnx engine, model integration pending)
- **Wake word** — ASR-based phrase detection actor (audio source integration pending)
- **Neural punctuation** — ct-transformer model auto-inserts punctuation
- **Post-processing** — filler removal, spoken-to-written normalization, CJK punctuation, English token repair
- **Three hotkey modes** — push-to-talk, toggle, auto (short-press toggle / long-press PTT)
- **Text injection** to any focused window (Wayland + X11)
- **Bilingual** — Chinese-English mixed recognition
- **Audio feedback** — beep on recording start/stop
- **systemd service** — auto-start on login

## Quick Start

See [INSTALL.md](INSTALL.md) for detailed installation instructions.

```bash
# Build
git clone https://github.com/user/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release

# Download models
voicerouter setup

# Run
voicerouter --preload
```

Press **Right Alt** to record, release to transcribe. Text is injected into the focused window.

## Configuration

Config file: `~/.config/voicerouter/config.toml` (created on first `voicerouter setup`).

### Hotkey

```toml
[hotkey]
key = "KEY_RIGHTALT"    # evdev key name
mode = "auto"           # ptt | toggle | auto
hold_delay = 0.3        # auto mode long-press threshold (seconds)
```

### ASR

```toml
[asr]
model = "paraformer-zh"   # paraformer-zh | funasr-nano | whisper-tiny-en | whisper-base-en
model_dir = "~/.cache/voicerouter/models"
```

### Post-processing

```toml
[postprocess]
punct_mode = "strip_trailing"  # keep | strip_trailing | replace_space
fullwidth_punct = true         # CJK fullwidth punctuation conversion
fix_english = true             # repair broken English tokens from ASR
remove_fillers = true          # remove hesitation fillers (嗯、啊、呃)
normalize_spoken = true        # convert spoken numbers/dots to written form
restore_punctuation = true     # ct-transformer punctuation restoration
```

Punctuation modes:
- `keep` — preserve all punctuation (你好，世界。)
- `strip_trailing` — remove trailing punctuation (你好，世界)
- `replace_space` — replace punctuation with a single space (你好，世界。再见 → 你好 世界 再见)

### Pipeline

The pipeline replaces the legacy `[router]` section. Define stages with handlers and optional conditions:

```toml
[[pipeline.stages]]
name = "search"
handler = "shell"
command = "google-chrome 'https://www.google.com/search?q={text}'"
condition = "starts_with:搜索"

[[pipeline.stages]]
name = "default"
handler = "inject"
```

Available handlers:
- `inject` — type text into focused window
- `shell` — run a shell command (with `{text}` template)
- `pipe` — pipe text through stdin/stdout of a subprocess
- `http` — send HTTP request (GET/POST) with `{text}` template
- `transform` — apply regex or template transformation

If no `[[pipeline.stages]]` are configured, a default inject handler is used. Legacy `[[router.rules]]` are auto-migrated with a deprecation warning.

### IPC

```toml
[ipc]
enabled = true
socket_path = ""          # default: $XDG_RUNTIME_DIR/voicerouter.sock
max_connections = 8
```

JSON-RPC methods: `pipeline.send`, `recording.start`, `recording.stop`, `status`, `events.subscribe`.

Example:
```bash
echo '{"method":"status"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/voicerouter.sock
```

### TTS (experimental)

```toml
[tts]
enabled = false
engine = "sherpa-onnx"
model = "vits-zh"
speed = 1.0
mute_mic_during_playback = true
```

### Wake Word (experimental)

```toml
[wakeword]
enabled = false
phrases = ["小助手"]
window_seconds = 2.0
stride_seconds = 1.0
action = "start_recording"   # start_recording | pipeline_passthrough
```

### Injection Method

```toml
[inject]
method = "auto"   # auto | clipboard_paste | wtype | xdotool
```

## CLI

```bash
voicerouter                      # start daemon
voicerouter --preload            # preload models then start
voicerouter --test-audio         # test microphone (3s recording, show RMS)
voicerouter --test-inject "text" # test text injection
voicerouter setup                # check tools and models
voicerouter download [model]     # download model files
voicerouter service install      # install systemd user service
voicerouter service start        # start service
voicerouter service status       # check status
```

## Architecture

Actor-based architecture with central message bus:

```
┌──────────┐     ┌──────────┐     ┌──────────────┐     ┌──────────┐
│ Hotkey   │────▶│          │────▶│   Pipeline    │────▶│  IPC     │
│ Actor    │     │   Bus    │     │   Actor       │     │  Actor   │
└──────────┘     │          │     │ (linear/DAG)  │     └──────────┘
                 │ crossbeam│     └──────────────┘
┌──────────┐     │ channels │     ┌──────────────┐     ┌──────────┐
│  Core    │◀───▶│          │◀───▶│    TTS       │     │ Wakeword │
│  Actor   │     │          │     │   Actor       │     │  Actor   │
│(audio+ASR│     └──────────┘     └──────────────┘     └──────────┘
│+postproc)│
└──────────┘
```

Each actor runs on its own thread. The Bus routes typed `Message` enums via topic-based 1:N subscriptions.

## Project Structure

```
src/
├── main.rs              # CLI entry and actor-based daemon
├── actor.rs             # Message enum, Actor trait, Bus
├── core_actor.rs        # Audio capture, ASR, postprocessing
├── ipc.rs               # Unix socket + JSON-RPC server
├── asr/                 # Speech recognition (sherpa-onnx)
│   ├── engine.rs        # Recognizer wrapper
│   └── models.rs        # Model registry and paths
├── audio/               # Audio capture and denoising
├── hotkey/              # Hotkey monitoring (evdev) + HotkeyActor
├── inject/              # Text injection (Wayland/X11)
├── pipeline/            # Composable handler pipeline
│   ├── mod.rs           # PipelineActor, linear execution
│   ├── dag.rs           # DAG topological sort and execution
│   ├── handler.rs       # Handler trait, HandlerResult
│   ├── stage.rs         # Stage, Condition, StageContext
│   └── handlers/        # Built-in handlers
│       ├── inject.rs    # Text injection handler
│       ├── shell.rs     # Shell command handler
│       ├── pipe.rs      # Stdin/stdout pipe handler
│       ├── http.rs      # HTTP request handler
│       └── transform.rs # Regex/template transform handler
├── postprocess/         # Text post-processing pipeline
│   ├── filler.rs        # Filler word removal
│   ├── normalize.rs     # Spoken-to-written normalization
│   ├── english_fix.rs   # Broken English token repair
│   └── punctuation.rs   # Punctuation handling
├── tts/                 # Text-to-speech
│   ├── mod.rs           # TtsActor, TtsEngine trait, cpal playback
│   └── sherpa.rs        # sherpa-onnx TTS engine (stub)
├── wakeword/            # Wake word detection
│   ├── mod.rs           # WakewordActor
│   └── detector.rs      # Phrase prefix matching
└── sound.rs             # Audio feedback (beeps)
```

## Known Limitations

- Offline inference only, no streaming recognition
- RNNoise denoising may be too aggressive; keep `denoise = false` unless needed
- `wtype` unavailable on GNOME Wayland (auto-falls back to clipboard-paste)
- TTS engine is a stub — returns silence until model integration is complete
- Wake word actor skeleton only — needs audio source integration for continuous detection

## License

MIT
