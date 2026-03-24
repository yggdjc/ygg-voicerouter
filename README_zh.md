# voicerouter

**Linux 语音交互框架** — 离线语音识别 + Actor 架构 + 可组合 Pipeline + IPC + 可扩展 Handler。单一二进制，纯 CPU 推理。

[English](README.md)

## 特性

- **离线语音识别** — 基于 [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)，支持 Paraformer（默认）和 FunASR Nano 模型
- **Actor 架构** — 每个组件独立线程运行，通过中央消息总线通信
- **可组合 Pipeline** — 链式 Handler + 条件匹配，或 DAG 工作流编排
- **内置 Handler** — inject（输入文字）、shell（执行命令）、pipe（子进程管道）、http（API 调用）、transform（正则/模板变换）、speak（TTS 语音输出）
- **IPC** — Unix socket + JSON-RPC 2.0，支持外部工具集成
- **TTS** — Kokoro v1.1 中文语音合成，sherpa-onnx OfflineTts 引擎，cpal 播放，自动静音麦克风
- **唤醒词** — 基于 ASR 的短语检测，AudioSource 广播共享音频流，滑动窗口持续监听
- **神经标点恢复** — ct-transformer 模型自动添加标点符号
- **后处理** — 填充词去除、口语转书面语、CJK 标点转换、断裂英文修复
- **三种热键模式** — 按住说话 (PTT)、切换、自动（短按切换/长按 PTT）
- **文字注入** — 识别结果直接输入到当前聚焦的窗口（Wayland + X11）
- **中英文混合** — Paraformer 双语模型，中英文无缝切换
- **音频反馈** — 录音开始/结束时播放提示音
- **systemd 服务** — 开机自启动

## 快速开始

详细安装步骤见 [INSTALL_zh.md](INSTALL_zh.md)。

```bash
# 编译
git clone https://github.com/user/ygg-voicerouter.git
cd ygg-voicerouter
cargo build --release

# 下载模型
voicerouter setup

# 启动
voicerouter --preload
```

按 **右 Alt** 键说话，松开后文字自动输入到当前窗口。

## 配置

配置文件位于 `~/.config/voicerouter/config.toml`，首次运行 `voicerouter setup` 自动创建。

### 热键

```toml
[hotkey]
key = "KEY_RIGHTALT"    # evdev 键名
mode = "auto"           # ptt | toggle | auto
hold_delay = 0.3        # auto 模式长按阈值（秒）
```

### 语音识别

```toml
[asr]
model = "paraformer-zh"   # paraformer-zh | funasr-nano | whisper-tiny-en | whisper-base-en
model_dir = "~/.cache/voicerouter/models"
```

### 后处理

```toml
[postprocess]
punct_mode = "strip_trailing"  # keep | strip_trailing | replace_space
fullwidth_punct = true         # CJK 全角标点转换
fix_english = true             # 修复断裂英文 token
remove_fillers = true          # 去除犹豫填充词（嗯、啊、呃）
normalize_spoken = true        # 口语转书面语（数字、文件名点号）
restore_punctuation = true     # ct-transformer 标点恢复
```

标点模式说明：
- `keep` — 保留所有标点（你好，世界。）
- `strip_trailing` — 去除末尾标点（你好，世界）
- `replace_space` — 标点替换为空格（你好，世界。再见 → 你好 世界 再见）

### Pipeline

Pipeline 替代旧版 `[router]` 配置。使用 handler + 条件匹配定义处理阶段：

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

可用 Handler：
- `inject` — 将文字输入到聚焦窗口
- `shell` — 执行 shell 命令（支持 `{text}` 模板）
- `pipe` — 通过子进程 stdin/stdout 管道传输文本
- `http` — 发送 HTTP 请求（GET/POST），支持 `{text}` 模板
- `transform` — 正则替换或模板变换
- `speak` — 将文字发送给 TTS Actor 进行语音播报

未配置 `[[pipeline.stages]]` 时自动使用默认 inject handler。旧版 `[[router.rules]]` 会自动迁移并显示弃用警告。

### 录音行为

录音停止行为取决于触发方式：

| 触发方式 | 静音自动停止 | 超时限制 |
|---------|------------|---------|
| 唤醒词 | 说话后 1.5s 静音 | 无 |
| 热键（PTT/toggle/auto） | 无 | 60s |

唤醒词录音在检测到说话后，静默 1.5 秒自动停止。热键录音不会因静音而停止——PTT 松键停止，toggle 二次按键停止，60 秒超时兜底。

### IPC

```toml
[ipc]
enabled = true
socket_path = ""          # 默认: $XDG_RUNTIME_DIR/voicerouter.sock
max_connections = 8
```

JSON-RPC 方法：`pipeline.send`、`recording.start`、`recording.stop`、`status`、`events.subscribe`。

示例：
```bash
echo '{"method":"status"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/voicerouter.sock
```

### TTS

Kokoro v1.1 中文语音合成。通过 `speak` pipeline handler 触发语音输出。

```toml
[tts]
enabled = true
engine = "sherpa-onnx"
model = "kokoro-tts"          # model_dir 下的模型目录名
model_dir = "~/.cache/voicerouter/models"
speed = 1.2
sid = 3                       # zf_001 — 中文女声
mute_mic_during_playback = true
```

TTS pipeline 示例：
```toml
[[pipeline.stages]]
name = "echo"
handler = "speak"
condition = "starts_with:echo "
```

说 "echo 你好世界"——触发前缀被去除，"你好世界" 由 TTS 播报。

### 唤醒词

基于 ASR 的短语检测，使用 AudioSource 广播共享音频流，在滑动窗口中持续监听。

```toml
[wakeword]
enabled = true
phrases = ["小助手"]
window_seconds = 2.0
stride_seconds = 1.0
action = "start_recording"   # start_recording | pipeline_passthrough
```

### 持续监听

始终开启的监听模式，集成 VAD 语音活动检测和意图分类。自动检测语音片段，完成转录后将语音分类为命令或环境音。

```toml
[continuous]
enabled = false              # 默认关闭，需显式启用
vad_model = "silero"

[continuous.llm]
endpoint = "http://localhost:8080/v1"
model = "claude-haiku"
api_key_env = "VOICEROUTER_LLM_KEY"
```

高风险操作（shell、http、pipe）需要热键确认后执行。低风险操作（inject、speak、transform）静默执行。

说话人验证（`speaker_verify`）计划在未来版本中实现。

### 注入方式

```toml
[inject]
method = "auto"   # auto | clipboard_paste | wtype | xdotool
```

## CLI 命令

```bash
voicerouter                      # 启动守护进程
voicerouter --preload            # 预加载模型后启动
voicerouter --test-audio         # 测试麦克风（录 3 秒，显示 RMS）
voicerouter --test-inject "你好"  # 测试文字注入
voicerouter setup                # 检查工具和模型
voicerouter download [model]     # 下载模型文件
voicerouter service install      # 安装 systemd 用户服务
voicerouter service start        # 启动服务
voicerouter service status       # 查看状态
```

## 架构

Actor 架构 + 中央消息总线：

```
┌──────────┐     ┌──────────┐     ┌──────────────┐     ┌──────────┐
│ Hotkey   │────▶│          │────▶│   Pipeline    │────▶│  IPC     │
│ Actor    │     │   Bus    │     │   Actor       │     │  Actor   │
└──────────┘     │          │     │ (线性/DAG)    │     └──────────┘
                 │ crossbeam│     └──────────────┘
┌──────────┐     │ channels │     ┌──────────────┐     ┌──────────┐
│  Core    │◀───▶│          │◀───▶│    TTS       │     │ Wakeword │
│  Actor   │     │          │     │   Actor       │     │  Actor   │
│(音频+ASR │     └──────────┘     └──────────────┘     └──────────┘
│+后处理)  │
└──────────┘
```

每个 Actor 在独立线程运行，Bus 通过 topic 订阅实现 1:N 消息路由。

## 已知限制

- 仅支持离线推理，不支持流式识别
- RNNoise 去噪可能过于激进，建议保持 `denoise = false`
- `wtype` 在 GNOME Wayland 下不可用（自动回退到 clipboard-paste）
- TTS 需下载 Kokoro 模型（约 500 MB）

## 许可

MIT
