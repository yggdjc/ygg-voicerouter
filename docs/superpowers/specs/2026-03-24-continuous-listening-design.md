# Continuous Listening with Speaker Verification + Intent Detection

**Date:** 2026-03-24
**Status:** Draft

## Problem

Current voice interaction requires explicit triggers (hotkey or wakeword). A Jarvis-like experience needs the system to continuously listen, identify the user by voice, and determine whether speech is an actionable command — without false-firing on ambient conversation.

## Design

### Core Flow

```
Microphone → AudioSource → VAD → [speech segment] → Speaker Verify →
  ├─ [not enrolled user] → discard (skip ASR)
  └─ [enrolled user] → ASR → IntentFilter →
       ├─ Command (high confidence) → risk-graded execution
       ├─ Ambient (high confidence) → discard
       └─ Uncertain → LLM Judge → execute or discard
```

### Components

#### 1. VAD Actor

Voice Activity Detection using Silero VAD (~2MB model). Subscribes to AudioSource broadcast, detects speech onset/offset, emits complete speech segments delimited by silence.

- **Input:** AudioSource broadcast (shared with CoreActor, WakewordActor)
- **Output:** `Message::SpeechSegment { samples: Vec<f32>, duration: f32 }`
- **Idle cost:** Near-zero CPU when no speech detected
- **Model:** Silero VAD v5 ONNX (~2MB)

#### 2. Speaker Verifier

Speaker embedding extraction + cosine similarity against enrolled user profile. Uses sherpa-onnx speaker embedding API.

- **Model:** 3D-Speaker or ECAPA-TDNN (~10MB), produces 512-dim embedding
- **Enrollment:** CLI subcommand `voicerouter enroll` — user records 3-5 utterances, system stores mean embedding vector (~2KB file)
- **Runtime:** Extract segment embedding → cosine similarity against enrollment → threshold check
- **Threshold:** Configurable, default 0.6. Below threshold → discard segment without ASR.
- **Latency:** ~10-30ms per segment

#### 3. IntentFilter (local)

Rule-based classifier on ASR transcript. Three-class output: `Command`, `Ambient`, `Uncertain`.

**Rules (evaluated in order):**

1. Length < 2 chars → `Ambient`
2. Pure filler words (嗯、啊、哦、呃) → `Ambient`
3. Matches any pipeline stage condition (starts_with triggers) → `Command`
4. Imperative verb prefix (帮我、打开、搜索、关闭、播放、切换) → `Command`
5. Contains actionable entity (file path, URL, app name) → `Command`
6. No verb, no question word, short sentence → `Ambient`
7. Otherwise → `Uncertain`

Intent to filter 80%+ of segments locally. Only `Uncertain` segments hit remote LLM.

#### 4. LLM Judge (remote)

OpenAI-compatible API call for segments the local filter cannot classify.

- **Endpoint:** Configurable (user's proxy supports Claude + ChatGPT)
- **Prompt:** System prompt with available pipeline actions + transcript → returns JSON `{ "intent": "command"|"ambient", "action": "...", "text": "..." }`
- **Model:** Configurable (default: fast/cheap model like Haiku)
- **Timeout:** 5s, fallback to discard on timeout
- **Privacy:** Only uncertain segments are sent; can disable entirely for pure-local mode

#### 5. Risk-Graded Execution

Once a segment is classified as `Command`:

| Risk level | Handlers | Behavior |
|-----------|----------|----------|
| Low | inject, speak, transform | Silent execution |
| High | shell, http, pipe | Beep + wait for hotkey confirm/cancel (3s timeout → cancel) |

Risk level is determined by the pipeline stage's handler type, not the command content.

#### 6. Configuration

```toml
[continuous]
enabled = false
speaker_verify = true
speaker_threshold = 0.6
speaker_model = "3dspeaker"
vad_model = "silero"

[continuous.llm]
endpoint = "http://localhost:8080/v1"
model = "claude-haiku"
api_key_env = "VOICEROUTER_LLM_KEY"
```

### Coexistence with Existing Modes

| Mode | Trigger | Behavior |
|------|---------|----------|
| Hotkey | RIGHT ALT | Unchanged. Bypasses VAD/speaker/intent — direct record → ASR → pipeline. |
| Wakeword | "小助手" | Retained as fallback. No LLM dependency. |
| Continuous | `continuous.enabled = true` | New flow: VAD → speaker → ASR → intent → execute. |

All three modes can be active simultaneously. They share AudioSource broadcast but operate independently.

### New Message Types

```rust
/// A speech segment detected by VAD.
SpeechSegment { samples: Vec<f32>, duration: f32 },

/// Intent classification result from continuous listening pipeline.
IntentResult { text: String, intent: Intent, confidence: f32 },

/// Request user confirmation for high-risk action.
ConfirmAction { text: String, stage: String },
```

### Architecture Diagram

```
                    ┌───────────┐
                    │AudioSource│
                    │  (cpal)   │
                    └─────┬─────┘
                          │ broadcast
              ┌───────────┼───────────┐
              ▼           ▼           ▼
        ┌──────────┐ ┌────────┐ ┌──────────┐
        │   VAD    │ │  Core  │ │ Wakeword │
        │  Actor   │ │ Actor  │ │  Actor   │
        └────┬─────┘ └────────┘ └──────────┘
             │ SpeechSegment
             ▼
      ┌──────────────┐
      │   Speaker    │
      │  Verifier    │
      └──────┬───────┘
             │ [verified]
             ▼
      ┌──────────────┐
      │     ASR      │
      │  (reuse eng) │
      └──────┬───────┘
             │ transcript
             ▼
      ┌──────────────┐
      │ IntentFilter │──── Uncertain ───▶ ┌───────────┐
      │   (local)    │                    │ LLM Judge │
      └──────┬───────┘                    │ (remote)  │
             │ Command                    └─────┬─────┘
             ▼                                  │
      ┌──────────────┐◀────────────────────────┘
      │   Pipeline   │
      │   Actor      │
      └──────────────┘
```

### Data Flow Example

**User says: "帮我搜索 Rust VAD 库"**

1. AudioSource broadcasts audio chunks
2. VAD detects speech onset, collects until silence, emits SpeechSegment (1.8s)
3. Speaker Verifier: cosine similarity 0.82 > 0.6 → pass
4. ASR transcribes: "帮我搜索Rust VAD库"
5. IntentFilter: imperative prefix "帮我搜索" → `Command` (high confidence)
6. Pipeline: matches `starts_with:搜索` stage → shell handler (high risk)
7. Beep + await confirmation hotkey
8. User presses hotkey → executes `google-chrome 'https://...'`

**Coworker says: "今天中午吃什么"**

1. VAD emits SpeechSegment
2. Speaker Verifier: cosine similarity 0.31 < 0.6 → **discard** (no ASR, no cost)

**User mumbles: "嗯...这个bug好奇怪"**

1. VAD emits SpeechSegment
2. Speaker Verifier: 0.78 → pass
3. ASR: "嗯这个bug好奇怪"
4. IntentFilter: no imperative verb, no trigger match, declarative → `Ambient` → discard

### Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Bystander speech executed | Low | Speaker verification filters non-enrolled speakers |
| Privacy leak to remote LLM | Medium | Only uncertain segments sent; pure-local mode available |
| CPU usage | Low | VAD ~0%, speaker ~10ms/seg, ASR only on verified segments |
| Self-talk misclassified as command | Medium | Rule + LLM dual filter + high-risk confirmation |
| API cost | Low | Speaker + local filter eliminates 90%+; cheap model for remainder |
| Speaker verify fails in noise | Low | Adjustable threshold; low-confidence treated as Uncertain |
| ASR engine contention | Low | Reuse CoreActor's ASR engine with Arc<Mutex> or dedicated instance |

### Dependencies

| Dependency | Purpose | Size |
|-----------|---------|------|
| silero-vad ONNX | Voice activity detection | ~2MB |
| 3dspeaker/ECAPA-TDNN ONNX | Speaker embedding | ~10MB |
| reqwest (already in Cargo.toml) | LLM API calls | 0 (existing) |
| sherpa-onnx speaker API | Embedding extraction | 0 (already linked) |

### Out of Scope (future)

- Multi-speaker enrollment (family/team use)
- Continuous conversation context (multi-turn with LLM)
- Streaming ASR (blocked by sherpa-rs 0.6 limitation)
- Speaker diarization (who said what in group settings)
