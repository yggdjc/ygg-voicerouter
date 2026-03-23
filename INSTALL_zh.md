# 安装指南

## 系统要求

- **操作系统**: Linux（Ubuntu 22.04+、Fedora 38+、Arch 等）
- **音频服务**: PulseAudio 或 PipeWire
- **Rust**: 1.70+（从源码编译时需要）
- **内存**: 1 GB 空闲（运行时占用 635 MB）

## 1. 安装系统依赖

### Ubuntu / Debian

```bash
# 文字注入工具（选一组）
# Wayland（推荐）：
sudo apt install wl-clipboard ydotool

# X11：
sudo apt install xdotool xclip
```

### Fedora

```bash
# Wayland：
sudo dnf install wl-clipboard ydotool

# X11：
sudo dnf install xdotool xclip
```

### Arch

```bash
# Wayland：
sudo pacman -S wl-clipboard ydotool

# X11：
sudo pacman -S xdotool xclip
```

## 2. 从源码编译

```bash
git clone https://github.com/user/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release
```

二进制文件位于 `target/release/voicerouter`。

可选：复制到 PATH：

```bash
cp target/release/voicerouter ~/.local/bin/
```

## 3. 下载模型

### ASR 模型（必需）

默认模型：**Paraformer**（中英双语，243 MB）

```bash
mkdir -p ~/.cache/voicerouter/models
cd ~/.cache/voicerouter/models

# Paraformer（默认）
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
tar -xjf sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
mv sherpa-onnx-paraformer-zh-2023-09-14 paraformer-zh
```

备选模型：**FunASR Nano**（0.8B LLM 架构，751 MB，准确率更高但更慢）

```bash
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-funasr-nano-int8-2025-12-30.tar.bz2
tar -xjf sherpa-onnx-funasr-nano-int8-2025-12-30.tar.bz2
mv sherpa-onnx-funasr-nano-int8-2025-12-30 funasr-nano
```

### 标点模型（推荐）

**ct-transformer** 自动标点恢复（76 MB）：

```bash
cd ~/.cache/voicerouter/models
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
tar -xjf sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
mv sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8 ct-punc
```

## 4. 环境设置

sherpa-onnx 运行时需要动态库路径：

```bash
export LD_LIBRARY_PATH=/path/to/ygg-voicerouter/target/release:$LD_LIBRARY_PATH
```

将此行加入 `~/.bashrc` 或 `~/.zshrc` 使其永久生效。

## 5. 验证安装

```bash
voicerouter setup
```

检查所需工具、模型和音频设备。

## 6. 启动

```bash
# 预加载模型启动（首次识别更快）
voicerouter --preload

# 测试麦克风
voicerouter --test-audio

# 测试文字注入
voicerouter --test-inject "你好世界"
```

## 7. 开机自启动（可选）

安装为 systemd 用户服务：

```bash
voicerouter service install
voicerouter service start
```

查看日志：

```bash
journalctl --user -u voicerouter -f
```

## 切换模型

编辑 `~/.config/voicerouter/config.toml`：

```toml
[asr]
model = "funasr-nano"   # 切换到 FunASR Nano
```

可用模型：`paraformer-zh`（默认）、`funasr-nano`、`whisper-tiny-en`、`whisper-base-en`。

## 常见问题

### 麦克风无声音

```bash
voicerouter --test-audio
```

如果 RMS 一直为 0.0：
- 检查音频服务：`pactl list short sources`
- 选择麦克风：`pavucontrol` 或 `pactl set-default-source`
- 检查音量：`alsamixer`

### 文字无法注入

```bash
voicerouter --test-inject "测试"
```

- **Wayland**：确认 `echo $WAYLAND_DISPLAY` 有值，`wl-copy` 和 `ydotool` 已安装
- **X11**：确认 `echo $DISPLAY` 有值，`xdotool` 已安装

### 找不到模型

```bash
voicerouter setup
```

按照输出的下载指引操作。
