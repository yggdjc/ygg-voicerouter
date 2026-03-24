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

- `src/llm/client.rs` — add `chat()` method for structured JSON conversation
- `src/vad/mod.rs` — extract VAD logic from ContinuousActor into shared module
- `src/continuous/mod.rs` — refactor to use shared `src/vad/`
- `src/actor.rs` — add `StartConversation` / `EndConversation` messages
- `src/wakeword/mod.rs` — add `start_conversation` action
- `src/main.rs` — spawn ConversationActor, register bus subscriptions
- `src/config.rs` — add `[conversation]` config section

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
  AudioSource ──chunks──▶ VAD ──speech──▶ ASR
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
Speaking ──all sentences done──▶ Listening
Listening ──timeout──▶ Idle
Any ──EndConversation──▶ Idle
```

## LLM Communication Protocol

### Request

POST `{endpoint}/v1/chat/completions`:

```json
{
  "model": "qwen3.5:4b",
  "messages": [
    {"role": "system", "content": "你是一个简洁的语音助手。用口语化的中文回答，保持简短。"},
    {"role": "user", "content": "今天天气怎么样"}
  ],
  "stream": false,
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "chat_response",
      "schema": {
        "type": "object",
        "properties": {
          "reply": { "type": "string" },
          "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
        },
        "required": ["reply", "confidence"]
      }
    }
  }
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
- Dropped on timeout (configurable, default 30s) or end phrase match
- End phrases: configurable list (default: ["结束", "再见", "没事了"])

## Sentence Splitter

```rust
fn split_sentences(text: &str) -> Vec<&str>
```

- Split on Chinese punctuation (。！？) and English punctuation (. ! ?)
- Merge fragments shorter than 4 characters into the next sentence
- Trailing text without punctuation treated as a standalone sentence

## VAD Shared Module

Extract from ContinuousActor into `src/vad/mod.rs`:

```rust
pub struct VadDetector { /* silero state */ }
impl VadDetector {
    pub fn new(config: &VadConfig) -> Result<Self>;
    pub fn feed(&mut self, chunk: &[f32]) -> VadEvent; // Speech / Silence / None
}
```

ContinuousActor and ConversationActor each hold independent instances.

## Configuration

```toml
[conversation]
enabled = true
timeout_seconds = 30
end_phrases = ["结束", "再见", "没事了"]
confidence_high = 0.8
confidence_low = 0.5
low_confidence_prefix = "我不太确定，"
low_confidence_reject = "抱歉，我无法回答这个问题"
llm_timeout_seconds = 15

[conversation.llm]
endpoint = "http://localhost:11434/v1"
model = "qwen3.5:4b"
system_prompt = "你是一个简洁的语音助手。用口语化的中文回答，保持简短。"
```

Wake word action:

```toml
[wakeword]
action = "start_conversation"
```

ConversationActor and ContinuousActor are mutually exclusive (both do VAD listening).

## Mute Strategy

| State        | Mic    | Wakeword |
|--------------|--------|----------|
| Listening    | unmuted | muted   |
| Recording    | unmuted | muted   |
| Thinking     | muted   | muted   |
| Speaking     | muted   | muted   |

All mute states restored on return to Idle.

## Error Handling

| Scenario | Handling |
|----------|----------|
| Ollama unreachable / timeout | TTS "语音助手暂时不可用", end session |
| Ollama returns invalid JSON | Retry once, then TTS error message, end session |
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
| Integration | ConversationActor full loop (mock audio + mock LLM) |

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
- ConversationActor subscribes to: `StartConversation`, `EndConversation`, `SpeakDone`, audio chunks
