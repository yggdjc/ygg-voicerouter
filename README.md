# voicerouter

**Voice router for Linux** — offline speech recognition with pluggable handlers. Single binary, CPU-only.

[中文文档](README_zh.md)

## Features

- **Offline ASR** via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) — Paraformer (default) or FunASR Nano
- **Neural punctuation** — ct-transformer model auto-inserts punctuation
- **Filler word removal** — strips hesitation markers (嗯、啊、呃) while preserving semantic uses
- **Spoken-to-written normalization** — converts Chinese numbers to digits, "readme 点 md" to "readme.md"
- **Three hotkey modes** — push-to-talk, toggle, auto (short-press toggle / long-press PTT)
- **Text injection** to any focused window (Wayland + X11)
- **Voice routing** — prefix-based dispatch to inject / shell handlers
- **Bilingual** — Chinese-English mixed recognition
- **CJK post-processing** — fullwidth punctuation, broken English token repair
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

### Voice Routing

```toml
[[router.rules]]
trigger = "搜索 "
handler = "shell"
```

Say "搜索 天气预报" to execute `天气预报` as a shell command. Unmatched text is injected as typed input.

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
voicerouter service install      # install systemd user service
voicerouter service start        # start service
voicerouter service status       # check status
```

## Architecture

```
Hotkey → Record → [Denoise] → ASR → Punctuation → Filler removal → Normalize → Post-process → Route
                                                                                                ├─ inject (default)
                                                                                                └─ shell
```

## Project Structure

```
src/
├── main.rs              # CLI entry and daemon loop
├── asr/                 # Speech recognition (sherpa-onnx)
│   ├── engine.rs        # Recognizer wrapper
│   └── models.rs        # Model registry and paths
├── audio/               # Audio capture and denoising
├── hotkey/              # Hotkey monitoring (evdev)
├── inject/              # Text injection (Wayland/X11)
├── postprocess/         # Text post-processing pipeline
│   ├── filler.rs        # Filler word removal
│   ├── normalize.rs     # Spoken-to-written normalization
│   ├── english_fix.rs   # Broken English token repair
│   └── punctuation.rs   # Punctuation handling
├── router/              # Voice routing and handlers
└── sound.rs             # Audio feedback
```

## Known Limitations

- Offline inference only, no streaming recognition
- RNNoise denoising may be too aggressive; keep `denoise = false` unless needed
- `wtype` unavailable on GNOME Wayland (auto-falls back to clipboard-paste)

## License

MIT
