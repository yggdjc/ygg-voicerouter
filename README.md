# voicerouter

**Voice interaction framework for Linux** — offline speech recognition with actor-based architecture, composable pipeline, IPC, and extensible handlers. CPU-only.

[中文文档](README_zh.md)

## Features

- **Offline ASR** via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) — Paraformer (default) or FunASR Nano; optional **cloud ASR** via DashScope Qwen3-ASR-Flash-Realtime with local fallback
- **Actor architecture** — each component runs on its own thread with message-passing via central bus
- **Composable pipeline** — chain handlers with conditions, or build DAG workflows with fan-out
- **Built-in handlers** — inject (type text), shell (run commands), pipe (stdin/stdout), http (API calls), transform (regex/template), speak (TTS output)
- **IPC** — Unix socket with JSON-RPC 2.0 for external tool integration
- **TTS** — Kokoro v1.1 Chinese TTS via sherpa-onnx OfflineTts, cpal playback, auto mic-mute
- **Wake word** — ASR-based phrase detection with shared AudioSource broadcast, sliding window
- **Neural punctuation** — ct-transformer model auto-inserts punctuation
- **Post-processing** — filler removal, spoken-to-written normalization, CJK punctuation, English token repair
- **Three hotkey modes** — push-to-talk, toggle, auto (short-press toggle / long-press PTT)
- **Text injection** to any focused window (Wayland + X11)
- **Bilingual** — Chinese-English mixed recognition
- **Audio feedback** — beep on recording start/stop
- **Visual overlay** — GTK4 capsule HUD with waveform animation (separate `voicerouter-overlay` binary)
- **systemd service** — auto-start on login

## Quick Start

See [INSTALL.md](INSTALL.md) for detailed installation instructions.

```bash
# Build
git clone https://github.com/user/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release

# Build overlay (optional, requires libgtk-4-dev libgtk4-layer-shell-dev)
cd voicerouter-overlay && cargo build --release && cd ..

# Download models
voicerouter setup

# Run
voicerouter --preload
voicerouter-overlay    # optional: visual feedback overlay
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

#### Cloud ASR (optional)

DashScope Qwen3-ASR-Flash-Realtime is a real-time streaming cloud ASR service. When enabled, it is tried first; local ASR is used as a fallback on connection failure. Cloud ASR returns punctuated text, so `restore_punctuation` (ct-punc) is skipped.

Setup:
1. Export your API key: `export DASHSCOPE_API_KEY=your-key-here`
2. Enable in config:

```toml
[asr.cloud]
enabled = true
endpoint = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime"
model = "qwen3-asr-flash-realtime"
api_key_env = "DASHSCOPE_API_KEY"
language = "zh"   # zh | en | ja | etc.
```

The local model download is still required for fallback.

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
- `speak` — send text to the TTS actor for voice output

If no `[[pipeline.stages]]` are configured, a default inject handler is used. Legacy `[[router.rules]]` are auto-migrated with a deprecation warning.

### Recording Behavior

Recording stop behavior depends on how recording was triggered:

| Trigger | Silence auto-stop | Timeout |
|---------|-------------------|---------|
| Wakeword | 1.5s after speech | None |
| Hotkey (PTT/toggle/auto) | None | 60s |

Wakeword recordings auto-stop after 1.5 seconds of silence following detected speech. Hotkey recordings never auto-stop on silence — PTT stops on key release, toggle stops on second press, with a 60-second hard timeout as safety net.

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

### TTS

Kokoro v1.1 Chinese TTS via sherpa-onnx. Use the `speak` pipeline handler to trigger voice output.

```toml
[tts]
enabled = true
engine = "sherpa-onnx"
model = "kokoro-tts"          # model directory under model_dir
model_dir = "~/.cache/voicerouter/models"
speed = 1.2
sid = 3                       # zf_001 — Chinese female voice
mute_mic_during_playback = true
```

Example pipeline using TTS:
```toml
[[pipeline.stages]]
name = "echo"
handler = "speak"
condition = "starts_with:echo "
```

Say "echo 你好世界" — the trigger prefix is stripped and "你好世界" is spoken via TTS.

### Wake Word

ASR-based phrase detection using shared AudioSource broadcast. Continuously monitors audio in a sliding window.

```toml
[wakeword]
enabled = true
phrases = ["小助手"]
window_seconds = 2.0
stride_seconds = 1.0
action = "start_recording"   # start_recording | pipeline_passthrough
```

### Continuous Listening

Always-on mode with VAD and intent classification. Detects speech segments, transcribes, and classifies as command or ambient speech.

```toml
[continuous]
enabled = false              # off by default, enable explicitly
vad_model = "silero"

[continuous.llm]
endpoint = "http://localhost:8080/v1"
model = "claude-haiku"
api_key_env = "VOICEROUTER_LLM_KEY"
```

High-risk actions (shell, http, pipe) require hotkey confirmation. Low-risk actions (inject, speak, transform) execute silently.

Speaker verification (`speaker_verify`) is planned for a future release.

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
voicerouter-overlay              # start visual overlay (separate binary)
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
└────┬─────┘                  ┌──────────────────────────────────┐
     │ Unix socket            │ voicerouter-overlay (separate    │
     └───────────────────────▶│ process, GTK4 capsule HUD)       │
                              └──────────────────────────────────┘
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
│       ├── speak.rs     # TTS voice output handler
│       └── transform.rs # Regex/template transform handler
├── postprocess/         # Text post-processing pipeline
│   ├── filler.rs        # Filler word removal
│   ├── normalize.rs     # Spoken-to-written normalization
│   ├── english_fix.rs   # Broken English token repair
│   └── punctuation.rs   # Punctuation handling
├── audio_source.rs      # Shared cpal audio stream (broadcasts to Core + Wakeword)
├── overlay.rs           # Overlay client (fire-and-forget socket IPC to overlay process)
├── tts/                 # Text-to-speech
│   ├── mod.rs           # TtsActor, TtsEngine trait, cpal playback
│   └── sherpa.rs        # Kokoro v1.1 TTS via sherpa-onnx OfflineTts
├── wakeword/            # Wake word detection
│   ├── mod.rs           # WakewordActor
│   └── detector.rs      # Phrase prefix matching
├── conversation/        # Multi-turn voice conversation (LLM)
│   ├── mod.rs           # ConversationActor state machine
│   ├── session.rs       # Chat session and history management
│   └── sentence.rs      # Sentence splitting for TTS
├── continuous/          # Always-on intent classification
├── vad/                 # Voice activity detection (energy-based)
├── llm/                 # LLM client (OpenAI-compatible API)
└── sound.rs             # Audio feedback (beeps)

voicerouter-overlay/         # Separate crate: visual feedback overlay
├── Cargo.toml
└── src/
    ├── main.rs              # GTK4 app + socket message dispatch
    ├── protocol.rs          # JSON message types
    ├── window.rs            # Capsule window (GTK4 + layer-shell)
    ├── waveform.rs          # 5-bar animated waveform widget
    └── controller.rs        # Unix socket listener
```

## Known Limitations

- Offline inference only, no streaming recognition
- RNNoise denoising may be too aggressive; keep `denoise = false` unless needed
- `wtype` unavailable on GNOME Wayland (auto-falls back to clipboard-paste)
- TTS requires Kokoro model download (~500 MB)
- Overlay on GNOME Wayland: no layer-shell support, window position is compositor-controlled and may steal focus briefly during recording

## License

MIT
