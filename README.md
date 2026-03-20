# voicerouter

**Linux 语音路由器** — 离线语音识别 + 可扩展的语音指令系统。单一二进制，~635MB 内存，纯 CPU 推理，零 VRAM。

[English](README_en.md)

## 特性

- **离线语音识别** — 基于 [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 的 Paraformer 模型，不需要网络
- **神经标点恢复** — ct-transformer 模型自动添加标点符号
- **三种热键模式** — 按住说话 (PTT)、切换、自动（短按切换/长按 PTT）
- **文字注入** — 识别结果直接输入到当前聚焦的窗口（Wayland + X11）
- **语音路由** — 基于前缀匹配的指令分发，支持 inject/LLM/shell 三种处理器
- **中英文混合** — Paraformer 双语模型，中英文无缝切换
- **CJK 后处理** — 全角标点转换、断裂英文 token 修复
- **音频反馈** — 录音开始/结束时播放提示音
- **systemd 服务** — 开机自启动

## 资源占用对比

| 指标 | voicerouter (Rust) | voice-input (Python) |
|------|-------------------|---------------------|
| RAM | ~635 MB | ~3,400 MB |
| VRAM | 0 | ~2,200 MB |
| 启动时间 | ~2 秒 | ~20 秒 |
| 二进制大小 | ~8 MB | ~10 GB (.venv) |
| 模型磁盘 | 319 MB | ~5 GB+ |
| 运行时依赖 | 无 | Python + PyTorch + CUDA |

## 系统要求

- **Linux**（Ubuntu 22.04+、Fedora 38+ 等）
- **PulseAudio** 或 **PipeWire**
- 文字注入工具（任选其一）：
  - Wayland：`wl-copy` + `ydotool`（推荐）或 `wtype`（仅 wlroots）
  - X11：`xdotool`
- 可选：`ffmpeg`（音频格式转换）

## 安装

### 从源码编译

```bash
git clone https://github.com/yggdjc/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release
```

### 下载模型

```bash
# ASR 模型（Paraformer 中英双语，243MB）
mkdir -p ~/.cache/voicerouter/models
cd ~/.cache/voicerouter/models
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
tar -xjf sherpa-onnx-paraformer-zh-2023-09-14.tar.bz2
mv sherpa-onnx-paraformer-zh-2023-09-14 paraformer-zh

# 标点模型（ct-transformer，76MB）
curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
tar -xjf sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2
mv sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8 ct-punc
```

### 环境检查

```bash
# 需要设置 LD_LIBRARY_PATH 指向 sherpa-onnx 动态库
export LD_LIBRARY_PATH=target/release:$LD_LIBRARY_PATH

voicerouter setup
```

## 快速开始

```bash
# 启动（预加载模型，首次识别更快）
voicerouter --preload

# 按右 Alt 键说话，松开后文字自动输入到当前窗口
```

默认配置：
- 热键：`KEY_RIGHTALT`（右 Alt）
- 模式：`auto`（短按切换，长按 PTT）
- 标点：保留中间标点，去除末尾标点

## 配置

配置文件位于 `~/.config/voicerouter/config.toml`，首次运行 `voicerouter setup` 自动创建。

### 热键

```toml
[hotkey]
key = "KEY_RIGHTALT"    # evdev 键名
mode = "auto"           # ptt | toggle | auto
hold_delay = 0.3        # auto 模式长按阈值（秒）
```

### 音频

```toml
[audio]
sample_rate = 16000
denoise = false         # RNNoise 去噪（实验性，可能降低识别率）
```

### 后处理

```toml
[postprocess]
punct_mode = "strip_trailing"  # keep | strip_trailing | replace_space
fullwidth_punct = true         # CJK 全角标点转换
fix_english = true             # 修复断裂英文 token
restore_punctuation = true     # ct-transformer 标点恢复
```

标点模式说明：
- `keep` — 保留所有标点（你好，世界。）
- `strip_trailing` — 去除末尾标点（你好，世界）
- `replace_space` — 用空格替代标点（你好 世界）

### 语音路由

```toml
[router]
# 前缀匹配规则，第一个匹配的生效
[[router.rules]]
trigger = "搜索 "
handler = "shell"

[[router.rules]]
trigger = "hey assistant "
handler = "llm"
```

说"搜索 天气预报"会执行 shell 命令 `天气预报`。不匹配任何规则时默认注入文字。

## CLI 命令

```bash
voicerouter                  # 启动守护进程
voicerouter --preload        # 预加载模型后启动
voicerouter --test-audio     # 测试麦克风（录 3 秒，显示 RMS）
voicerouter --test-inject "你好"  # 测试文字注入
voicerouter setup            # 检查工具和模型
voicerouter service install  # 安装 systemd 用户服务
voicerouter service start    # 启动服务
voicerouter service status   # 查看状态
```

## 架构

```
按键 → 录音 → [去噪] → ASR 识别 → 标点恢复 → 后处理 → 路由分发
                                                         ├─ inject（默认：注入文字）
                                                         ├─ llm（调用 LLM API）
                                                         └─ shell（执行命令）
```

## 已知限制

- sherpa-rs 0.6 仅支持离线推理，不支持流式识别
- 热词功能不可用（Paraformer 模型不支持）
- RNNoise 去噪可能过于激进，建议保持 `denoise = false`
- `wtype` 在 GNOME Wayland 下不可用（自动回退到 clipboard-paste）

## 许可

MIT
