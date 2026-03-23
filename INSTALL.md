# Installation Guide

## System Requirements

- **OS**: Linux (Ubuntu 22.04+, Fedora 38+, Arch, or similar)
- **Audio**: PulseAudio or PipeWire
- **Rust**: 1.70+ (for building from source)
- **RAM**: 1 GB free (635 MB at runtime)

## 1. Install System Dependencies

### Ubuntu / Debian

```bash
# Text injection (pick one set)
# Wayland (recommended):
sudo apt install wl-clipboard ydotool

# X11:
sudo apt install xdotool xclip
```

### Fedora

```bash
# Wayland:
sudo dnf install wl-clipboard ydotool

# X11:
sudo dnf install xdotool xclip
```

### Arch

```bash
# Wayland:
sudo pacman -S wl-clipboard ydotool

# X11:
sudo pacman -S xdotool xclip
```

## 2. Build from Source

```bash
git clone https://github.com/user/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release
```

The binary is at `target/release/voicerouter`.

Optional: copy to PATH:

```bash
cp target/release/voicerouter ~/.local/bin/
```

## 3. Download Models

### ASR Model (required)

Default model: **Paraformer** (Chinese-English bilingual, 243 MB)

```bash
mkdir -p ~/.cache/voicerouter/models
cd ~/.cache/voicerouter/models

# Paraformer (default)
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
tar -xjf sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
mv sherpa-onnx-paraformer-zh-2023-09-14 paraformer-zh
```

Alternative model: **FunASR Nano** (0.8B LLM-based, 751 MB, higher accuracy but slower)

```bash
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-funasr-nano-int8-2025-12-30.tar.bz2
tar -xjf sherpa-onnx-funasr-nano-int8-2025-12-30.tar.bz2
mv sherpa-onnx-funasr-nano-int8-2025-12-30 funasr-nano
```

### Punctuation Model (recommended)

**ct-transformer** for automatic punctuation restoration (76 MB):

```bash
cd ~/.cache/voicerouter/models
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
tar -xjf sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
mv sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8 ct-punc
```

## 4. Environment Setup

sherpa-onnx requires its shared libraries at runtime:

```bash
export LD_LIBRARY_PATH=/path/to/ygg-voicerouter/target/release:$LD_LIBRARY_PATH
```

Add this to your `~/.bashrc` or `~/.zshrc` for persistence.

## 5. Verify Installation

```bash
voicerouter setup
```

This checks for required tools, models, and audio devices.

## 6. Run

```bash
# Start with model preloading (faster first transcription)
voicerouter --preload

# Test microphone
voicerouter --test-audio

# Test text injection
voicerouter --test-inject "Hello, world!"
```

## 7. Auto-start (Optional)

Install as a systemd user service:

```bash
voicerouter service install
voicerouter service start
```

Check logs:

```bash
journalctl --user -u voicerouter -f
```

## Switching Models

Edit `~/.config/voicerouter/config.toml`:

```toml
[asr]
model = "funasr-nano"   # switch to FunASR Nano
```

Available models: `paraformer-zh` (default), `funasr-nano`, `whisper-tiny-en`, `whisper-base-en`.

## Troubleshooting

### Audio not working

```bash
voicerouter --test-audio
```

If RMS stays at 0.0:
- Check audio server: `pactl list short sources`
- Select microphone: `pavucontrol` or `pactl set-default-source`
- Check volume: `alsamixer`

### Text not injecting

```bash
voicerouter --test-inject "test"
```

- **Wayland**: verify `echo $WAYLAND_DISPLAY` is set, `wl-copy` and `ydotool` installed
- **X11**: verify `echo $DISPLAY` is set, `xdotool` installed

### Model not found

```bash
voicerouter setup
```

Follow the download instructions printed in the output.
