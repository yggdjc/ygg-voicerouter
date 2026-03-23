# voicerouter

**Linux 语音路由器** — 离线语音识别 + 可扩展的语音指令系统。单一二进制，纯 CPU 推理。

[English](README.md)

## 特性

- **离线语音识别** — 基于 [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)，支持 Paraformer（默认）和 FunASR Nano 模型
- **神经标点恢复** — ct-transformer 模型自动添加标点符号
- **填充词去除** — 去除犹豫停顿产生的口水词（嗯、啊、呃），保留语义用法
- **口语转书面语** — 中文数字转阿拉伯数字，"readme 点 md" 转 "readme.md"
- **三种热键模式** — 按住说话 (PTT)、切换、自动（短按切换/长按 PTT）
- **文字注入** — 识别结果直接输入到当前聚焦的窗口（Wayland + X11）
- **语音路由** — 基于前缀匹配的指令分发，支持 inject/shell 处理器
- **中英文混合** — Paraformer 双语模型，中英文无缝切换
- **CJK 后处理** — 全角标点转换、断裂英文 token 修复
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

### 语音路由

```toml
[[router.rules]]
trigger = "搜索 "
handler = "shell"
```

说"搜索 天气预报"会执行 shell 命令 `天气预报`。不匹配任何规则时默认注入文字。

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
voicerouter service install      # 安装 systemd 用户服务
voicerouter service start        # 启动服务
voicerouter service status       # 查看状态
```

## 架构

```
按键 → 录音 → [去噪] → ASR 识别 → 标点恢复 → 填充词去除 → 口语转写 → 后处理 → 路由分发
                                                                                    ├─ inject（默认：注入文字）
                                                                                    └─ shell（执行命令）
```

## 已知限制

- 仅支持离线推理，不支持流式识别
- RNNoise 去噪可能过于激进，建议保持 `denoise = false`
- `wtype` 在 GNOME Wayland 下不可用（自动回退到 clipboard-paste）

## 许可

MIT
