# Conversation Mode Design

## Overview

Add multi-turn voice conversation support: wake word triggers a conversation session where the user speaks, ASR transcribes, Ollama (qwen3.5:4b) generates a reply in structured JSON with confidence, and TTS plays the response. The session loops until timeout or an end phrase.

## Requirements

- Multi-turn conversation with context preserved across turns
- Ollama via OpenAI-compatible `/v1/chat/completions` endpoint
- JSON structured output with `reply` + `confidence` fields
- Non-streaming LLM call, sentence-level TTS playback
- VAD-based auto-listening during active session (no repeated wake word)
- Configurable session timeout (default 30s), end phrases
- Confidence thresholds: >= 0.8 normal, 0.5–0.8 disclaimer prefix, < 0.5 reject

## Architecture

### New Components

```
src/conversation/
├── mod.rs          # ConversationActor — state machine + main loop
├── session.rs      # Session — multi-turn context management
└── sentence.rs     # SentenceSplitter — split reply into sentences for TTS
```

### Modified Components

- `src/llm/client.rs` — add `chat()` method for structured JSON conversation; make `ChatMessage` pub
- `src/vad/mod.rs` — extract VAD logic from ContinuousActor into shared module (energy-based, matching current impl)
- `src/continuous/mod.rs` — refactor to use shared `src/vad/`
- `src/actor.rs` — add `StartConversation` / `EndConversation` messages
- `src/wakeword/mod.rs` — add `start_conversation` action; add `StartConversation` variant to `WakewordAction` enum in config.rs; handle in `emit_action()`
- `src/main.rs` — spawn ConversationActor, register bus subscriptions; wire dedicated `audio_rx: Receiver<AudioChunk>` from AudioSource (audio chunks use direct crossbeam channels, NOT the bus)
- `src/config.rs` — add `[conversation]` config section; add `StartConversation` to `WakewordAction` enum; validate mutual exclusivity: `conversation.enabled` and `continuous.enabled` cannot both be true (error at config load)

### Data Flow

```
WakewordActor ──StartConversation──▶ ConversationActor
                                         │
                                    ┌────▼────┐
                                    │ Session  │ (conversation history)
                                    └────┬────┘
                                         │
          ┌──────────────────────────────┤ LOOP:
          │                              │
          ▼                              │
  AudioSource ──chunks (crossbeam channel)──▶ VAD ──speech──▶ ASR
                                         │
                                    transcript
                                         │
                                    ┌────▼────┐
                                    │ Ollama   │ non-streaming JSON
                                    │ /v1/chat │──────┐
                                    └─────────┘      │
                                                     ▼
                                              Parse JSON (reply + confidence)
                                                     │
                                              Confidence gate
                                                     │
                                              SentenceSplitter
                                                     │
                                              per-sentence SpeakRequest ──▶ TtsActor
                                                     │
                                              SpeakDone ◀── TtsActor
                                                     │
                                              (next turn or timeout → Idle)
```

### State Machine

```
Idle ──StartConversation──▶ Listening
Listening ──VAD speech──▶ Recording
Recording ──VAD silence──▶ Transcribing
Transcribing ──ASR done──▶ Thinking
Thinking ──LLM done──▶ Speaking
Speaking ──SpeakDone count == sentence count──▶ Listening
Listening ──timeout (last_activity based)──▶ Idle
Recording ──max_turn_seconds (30s default)──▶ Transcribing
Any ──EndConversation──▶ Idle
```

## LLM Communication Protocol

### Request

POST `{endpoint}/v1/chat/completions`:

```json
{
  "model": "qwen3.5:4b",
  "messages": [
    {"role": "system", "content": "你是一个简洁的语音助手。用口语化的中文回答，保持简短。必须以JSON格式回复，包含reply(回答文本)和confidence(0-1置信度)两个字段。"},
    {"role": "user", "content": "今天天气怎么样"}
  ],
  "stream": false,
  "response_format": { "type": "json_object" }
}
```

### Response (parsed from choices[0].message.content)

```json
{
  "reply": "今天北京晴天，最高温度25度，适合出行。",
  "confidence": 0.85
}
```

### Confidence Handling

| confidence | Behavior |
|------------|----------|
| >= 0.8     | Play reply as-is |
| 0.5 – 0.8 | Prepend configurable disclaimer (default: "我不太确定，") |
| < 0.5      | Play configurable rejection (default: "抱歉，我无法回答这个问题") |

## Session Management

```rust
struct Session {
    history: Vec<ChatMessage>,  // role + content pairs
    created_at: Instant,
    last_activity: Instant,
}
```

- Created on `StartConversation`
- Each user/assistant turn appended to history
- Timeout based on `last_activity` (reset each turn), not `created_at`. Default 30s, configurable.
- End phrases: configurable list (default: ["结束", "再见", "没事了"])

## Sentence Splitter

```rust
fn split_sentences(text: &str) -> Vec<String>
```

- Split on Chinese punctuation (。！？) and English punctuation (. ! ?)
- Merge fragments shorter than 4 characters into the next sentence
- Trailing text without punctuation treated as a standalone sentence

## VAD Shared Module

Extract energy-based VAD from ContinuousActor into `src/vad/mod.rs`:

```rust
pub struct VadDetector { /* energy threshold state */ }
impl VadDetector {
    pub fn new(config: &VadConfig) -> Result<Self>;
    pub fn feed(&mut self, chunk: &[f32]) -> VadEvent; // Speech / Silence / None
}
```

ContinuousActor and ConversationActor each hold independent instances. ConversationActor lazy-inits its own `AsrEngine` instance (AsrEngine is not Sync, cannot share across threads).

## Configuration

```toml
[conversation]
enabled = true
timeout_seconds = 30            # inactivity timeout (from last turn)
max_turn_seconds = 30           # max recording duration per turn
end_phrases = ["结束", "再见", "没事了"]
confidence_high = 0.8
confidence_low = 0.5
low_confidence_prefix = "我不太确定，"
low_confidence_reject = "抱歉，我无法回答这个问题"
llm_timeout_seconds = 15

[conversation.llm]
endpoint = "http://localhost:11434/v1"
model = "qwen3.5:4b"
system_prompt = "你是一个简洁的语音助手。用口语化的中文回答，保持简短。必须以JSON格式回复，包含reply(回答文本)和confidence(0-1置信度)两个字段。"
# No api_key_env needed — Ollama requires no auth by default
```

Wake word action:

```toml
[wakeword]
action = "start_conversation"
```

ConversationActor and ContinuousActor are mutually exclusive. Enforced at config load: if both `conversation.enabled` and `continuous.enabled` are true, emit error and exit.

## Mute Strategy

| State        | Mic    | Wakeword |
|--------------|--------|----------|
| Listening    | unmuted | muted   |
| Recording    | unmuted | muted   |
| Thinking     | muted   | muted   |
| Speaking     | muted   | muted   |

On `StartConversation`: emit `MuteInput` to suppress WakewordActor and CoreActor.
On return to Idle: emit `UnmuteInput` to restore all actors.

## Error Handling

| Scenario | Handling |
|----------|----------|
| Ollama unreachable / timeout | TTS "语音助手暂时不可用", end session |
| Ollama returns invalid JSON | Retry once, then TTS error message, end session |
| ASR engine init failure | TTS "语音识别初始化失败", end session |
| ASR returns empty text | Ignore turn, continue listening |
| TTS playback failure | Log error, continue next turn |
| VAD init failure | Actor fails to start, log error, daemon runs without conversation |
| Wake word during active session | Ignore |
| User says end phrase | TTS "好的，再见", end session |
| TTS still playing when new turn starts | Wait for SpeakDone before VAD listening |

## Testing Strategy

| Level | Target |
|-------|--------|
| Unit | Session — history management, timeout, end phrase matching |
| Unit | SentenceSplitter — Chinese/English splitting, short fragment merging |
| Unit | Confidence branching logic |
| Unit | State machine transitions (every state × event combination) |
| Integration | LLM client → mock HTTP server → JSON parsing |
| Unit | Mute/unmute signal emission per state transition |
| Unit | Timeout via injected clock (last_activity anchor) |
| Integration | ConversationActor full loop (mock audio + mock LLM) |
| Negative | Malformed JSON from Ollama, confidence outside 0-1, empty reply |
| Negative | Concurrent StartConversation while session active |
| Negative | Rapid EndConversation + StartConversation sequence |

## Messages

New variants in `Message` enum:

```rust
StartConversation { wakeword: Option<String> }
EndConversation
```

Bus routing:
- `StartConversation` → ConversationActor
- `EndConversation` → ConversationActor
- ConversationActor emits: `SpeakRequest`, `MuteInput`, `UnmuteInput`
- ConversationActor subscribes to: `StartConversation`, `EndConversation`, `SpeakDone`
- Audio chunks: via dedicated `crossbeam::channel` from AudioSource (NOT the bus)

### SpeakDone Correlation

`SpeakDone` carries no ID. ConversationActor tracks `pending_sentences: usize` (set to sentence count when entering Speaking state) and decrements on each `SpeakDone`. Transitions to Listening when count reaches 0. This is fragile if other actors also trigger TTS concurrently, but acceptable since ConversationActor mutes other actors during a session.

### Ollama Cold Start

First request after model load can take 10-30s (VRAM loading). `llm_timeout_seconds` default is 15s which may be tight. ConversationActor sends a warmup ping (`messages: [{"role":"user","content":"hi"}]`) on actor init when `conversation.enabled = true`. Warmup failure is logged as a warning, not fatal.
