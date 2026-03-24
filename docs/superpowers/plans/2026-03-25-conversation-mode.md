# Conversation Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add multi-turn voice conversation with Ollama (qwen3.5:4b), triggered by wake word, with VAD auto-listening, JSON structured output with confidence, and sentence-level TTS playback.

**Architecture:** New `ConversationActor` with state machine (Idle→Listening→Recording→Transcribing→Thinking→Speaking→Listening loop). Reuses existing `AsrEngine`, `TtsActor` (via `SpeakRequest`), and `LlmClient` (extended with `chat()` method). VAD extracted from `ContinuousActor` into shared `src/vad/` module.

**Tech Stack:** Rust, crossbeam channels, ureq (HTTP), serde_json, sherpa-onnx (ASR/TTS), Ollama OpenAI-compatible API

**Spec:** `docs/superpowers/specs/2026-03-25-conversation-mode-design.md`

---

## File Map

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `src/conversation/mod.rs` | ConversationActor: state machine, main loop, mute orchestration |
| Create | `src/conversation/session.rs` | Session: multi-turn history (uses `crate::llm::ChatMessage`), timeout, end phrase matching |
| Create | `src/conversation/sentence.rs` | `split_sentences()`: split reply text into TTS-ready sentences |
| Create | `src/vad/mod.rs` | `VadDetector`: shared energy-based VAD (moved from continuous/vad.rs) |
| Modify | `src/continuous/vad.rs` | Delete (moved to src/vad/mod.rs) |
| Modify | `src/continuous/mod.rs` | Use `crate::vad::VadDetector` instead of local `EnergyVad` |
| Modify | `src/llm/client.rs` | Add `chat()` method, make `ChatMessage` pub, make api_key optional |
| Modify | `src/llm/mod.rs` | Re-export new public symbols: `ChatMessage`, `ConversationResponse`, `parse_chat_json` |
| Modify | `tests/vad_test.rs` | Update to use `voicerouter::vad::VadDetector` (new API) |
| Modify | `src/actor.rs` | Add `StartConversation` / `EndConversation` message variants |
| Modify | `src/config.rs` | Add `ConversationConfig`, `ConversationLlmConfig`, `StartConversation` to `WakewordAction`, mutual exclusivity validation |
| Modify | `src/wakeword/mod.rs` | Handle `StartConversation` action in `emit_action()` |
| Modify | `src/main.rs` | Spawn ConversationActor, wire audio channel, bus subscriptions |
| Modify | `src/lib.rs` | Add `pub mod conversation;` and `pub mod vad;` |

---

### Task 1: Sentence Splitter

Pure function with no dependencies — start here for easy TDD.

**Files:**
- Create: `src/conversation/sentence.rs`
- Create: `src/conversation/mod.rs` (stub)
- Modify: `src/lib.rs`

- [ ] **Step 1: Create module stubs**

Create `src/conversation/mod.rs` (only sentence for now; session added in Task 2):
```rust
pub mod sentence;
```

Add to `src/lib.rs`:
```rust
pub mod conversation;
```

- [ ] **Step 2: Write failing tests for split_sentences**

Create `src/conversation/sentence.rs`:
```rust
/// Split text into sentences for TTS playback.
///
/// Splits on Chinese (。！？) and English (. ! ?) sentence-ending punctuation.
/// Fragments shorter than 4 characters are merged into the next sentence.
/// Trailing text without punctuation is kept as a standalone sentence.
pub fn split_sentences(text: &str) -> Vec<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_punctuation() {
        let result = split_sentences("今天天气很好。明天会下雨！后天呢？");
        assert_eq!(result, vec![
            "今天天气很好。",
            "明天会下雨！",
            "后天呢？",
        ]);
    }

    #[test]
    fn english_punctuation() {
        let result = split_sentences("Hello world. How are you! Fine?");
        assert_eq!(result, vec![
            "Hello world.",
            "How are you!",
            "Fine?",
        ]);
    }

    #[test]
    fn merge_short_fragments() {
        // "好。" is 2 chars (< 4), merge into next sentence
        let result = split_sentences("好。今天天气不错。");
        assert_eq!(result, vec!["好。今天天气不错。"]);
    }

    #[test]
    fn trailing_without_punctuation() {
        let result = split_sentences("第一句话。然后这里没有标点");
        assert_eq!(result, vec!["第一句话。", "然后这里没有标点"]);
    }

    #[test]
    fn single_sentence_no_punctuation() {
        let result = split_sentences("就这样吧");
        assert_eq!(result, vec!["就这样吧"]);
    }

    #[test]
    fn empty_input() {
        let result = split_sentences("");
        assert!(result.is_empty());
    }

    #[test]
    fn mixed_chinese_english() {
        let result = split_sentences("你好。Hello world. 再见！");
        assert_eq!(result, vec!["你好。", "Hello world.", "再见！"]);
    }

    #[test]
    fn decimal_numbers_not_split() {
        let result = split_sentences("温度是25.5度。");
        assert_eq!(result, vec!["温度是25.5度。"]);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib conversation::sentence -- --nocapture`
Expected: FAIL with "not yet implemented"

- [ ] **Step 4: Implement split_sentences**

Replace the `todo!()` in `split_sentences`:
```rust
pub fn split_sentences(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?') {
            sentences.push(std::mem::take(&mut current));
        } else if ch == '.' {
            // Don't split on '.' between digits (e.g. "25.5")
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
            if !(prev_digit && next_digit) {
                sentences.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    // Merge short fragments (< 4 chars) into the next sentence.
    let mut merged: Vec<String> = Vec::new();
    let mut carry = String::new();
    for s in sentences {
        carry.push_str(&s);
        if carry.chars().count() >= 4 || carry == text {
            merged.push(std::mem::take(&mut carry));
        }
    }
    if !carry.is_empty() {
        if let Some(last) = merged.last_mut() {
            last.push_str(&carry);
        } else {
            merged.push(carry);
        }
    }

    merged
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib conversation::sentence -- --nocapture`
Expected: all 7 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/conversation/sentence.rs src/conversation/mod.rs src/lib.rs
git commit -m "feat(conversation): add sentence splitter for TTS playback"
```

---

### Task 2: Session Management

**Depends on:** Task 4 (LLM client must be done first — Session uses `crate::llm::ChatMessage`).

Pure struct with no I/O — easy to test.

**Files:**
- Create: `src/conversation/session.rs`
- Modify: `src/conversation/mod.rs` (add `pub mod session;`)

- [ ] **Step 1: Write failing tests for Session**

First, add `pub mod session;` to `src/conversation/mod.rs`.

Create `src/conversation/session.rs`:
```rust
use std::time::Instant;

use crate::llm::ChatMessage;

/// Multi-turn conversation session with history and timeout tracking.
/// Uses `crate::llm::ChatMessage` directly to avoid type duplication.
pub struct Session {
    history: Vec<ChatMessage>,
    system_prompt: String,
    pub created_at: Instant,
    pub last_activity: Instant,
    end_phrases: Vec<String>,
}

impl Session {
    pub fn new(system_prompt: String, end_phrases: Vec<String>) -> Self {
        todo!()
    }

    /// Add a user message to history, update last_activity.
    pub fn add_user_message(&mut self, content: &str) {
        todo!()
    }

    /// Add an assistant reply to history, update last_activity.
    pub fn add_assistant_message(&mut self, content: &str) {
        todo!()
    }

    /// Build the full message list (system + history) for the LLM request.
    pub fn messages(&self) -> Vec<ChatMessage> {
        todo!()
    }

    /// Check if the given text matches any end phrase.
    pub fn is_end_phrase(&self, text: &str) -> bool {
        todo!()
    }

    /// Check if the session has timed out based on last_activity.
    pub fn is_timed_out(&self, timeout_secs: f64) -> bool {
        todo!()
    }

    /// Number of turns (user messages) in this session.
    pub fn turn_count(&self) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_session_has_empty_history() {
        let s = Session::new("system".into(), vec!["结束".into()]);
        assert_eq!(s.turn_count(), 0);
        let msgs = s.messages();
        assert_eq!(msgs.len(), 1); // system prompt only
        assert_eq!(msgs[0].role, "system");
    }

    #[test]
    fn add_messages_builds_history() {
        let mut s = Session::new("system".into(), vec![]);
        s.add_user_message("hello");
        s.add_assistant_message("hi there");
        assert_eq!(s.turn_count(), 1);
        let msgs = s.messages();
        assert_eq!(msgs.len(), 3); // system + user + assistant
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn end_phrase_matching() {
        let s = Session::new("sys".into(), vec!["结束".into(), "再见".into()]);
        assert!(s.is_end_phrase("结束"));
        assert!(s.is_end_phrase("再见"));
        assert!(!s.is_end_phrase("继续"));
    }

    #[test]
    fn end_phrase_trimmed() {
        let s = Session::new("sys".into(), vec!["结束".into()]);
        assert!(s.is_end_phrase(" 结束 "));
    }

    #[test]
    fn timeout_check() {
        let mut s = Session::new("sys".into(), vec![]);
        // Force last_activity to be in the past.
        s.last_activity = Instant::now() - Duration::from_secs(60);
        assert!(s.is_timed_out(30.0));
        assert!(!s.is_timed_out(120.0));
    }

    #[test]
    fn activity_resets_on_message() {
        let mut s = Session::new("sys".into(), vec![]);
        s.last_activity = Instant::now() - Duration::from_secs(60);
        s.add_user_message("new input");
        assert!(!s.is_timed_out(30.0));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib conversation::session -- --nocapture`
Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement Session**

Replace all `todo!()` calls (note: uses `crate::llm::ChatMessage` — no separate type):
```rust
impl Session {
    pub fn new(system_prompt: String, end_phrases: Vec<String>) -> Self {
        let now = Instant::now();
        Self {
            history: Vec::new(),
            system_prompt,
            created_at: now,
            last_activity: now,
            end_phrases,
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.history.push(ChatMessage {
            role: "user".into(),
            content: content.to_string(),
        });
        self.last_activity = Instant::now();
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: content.to_string(),
        });
        self.last_activity = Instant::now();
    }

    pub fn messages(&self) -> Vec<ChatMessage> {
        let mut msgs = vec![ChatMessage {
            role: "system".into(),
            content: self.system_prompt.clone(),
        }];
        msgs.extend(self.history.clone());
        msgs
    }

    pub fn is_end_phrase(&self, text: &str) -> bool {
        let trimmed = text.trim();
        self.end_phrases.iter().any(|p| trimmed == p)
    }

    pub fn is_timed_out(&self, timeout_secs: f64) -> bool {
        self.last_activity.elapsed().as_secs_f64() >= timeout_secs
    }

    pub fn turn_count(&self) -> usize {
        self.history.iter().filter(|m| m.role == "user").count()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib conversation::session -- --nocapture`
Expected: all 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/conversation/session.rs
git commit -m "feat(conversation): add session management with history and timeout"
```

---

### Task 3: Extract VAD to Shared Module

Move existing code, adapt API, update ContinuousActor.

**Files:**
- Create: `src/vad/mod.rs`
- Delete: `src/continuous/vad.rs`
- Modify: `src/continuous/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create src/vad/mod.rs with new API and tests**

```rust
//! Energy-based Voice Activity Detection (shared module).
//!
//! Detects speech segments by tracking RMS energy above a threshold.
//! Returns complete speech segments via `VadEvent::Segment`.

use crate::audio;

const MIN_SEGMENT_SECS: f32 = 0.3;
const SILENCE_AFTER_SPEECH_SECS: f32 = 0.5;
const WINDOW_SAMPLES: usize = 800;

/// Events emitted by the VAD detector.
#[derive(Debug, PartialEq)]
pub enum VadEvent {
    /// A complete speech segment was detected.
    Segment(Vec<f32>),
}

/// Configuration for the VAD detector.
pub struct VadConfig {
    pub sample_rate: u32,
    pub threshold: f32,
}

pub struct VadDetector {
    sample_rate: u32,
    threshold: f32,
    in_speech: bool,
    onset_sample: usize,
    speech_end: usize,
    buffer: Vec<f32>,
    silence_samples: usize,
}

impl VadDetector {
    pub fn new(config: &VadConfig) -> Self {
        Self {
            sample_rate: config.sample_rate,
            threshold: config.threshold,
            in_speech: false,
            onset_sample: 0,
            speech_end: 0,
            buffer: Vec::new(),
            silence_samples: 0,
        }
    }

    /// Whether the detector is currently tracking an active speech segment.
    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    /// Feed audio samples. Returns segments as they are detected.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for chunk in samples.chunks(WINDOW_SAMPLES) {
            let rms = audio::compute_rms(chunk);
            let is_speech = rms >= self.threshold;

            if !self.in_speech {
                if is_speech {
                    self.onset_sample = self.buffer.len();
                    self.buffer.extend_from_slice(chunk);
                    self.speech_end = self.buffer.len();
                    self.silence_samples = 0;
                    self.in_speech = true;
                }
            } else {
                self.buffer.extend_from_slice(chunk);

                if is_speech {
                    self.silence_samples = 0;
                    self.speech_end = self.buffer.len();
                } else {
                    self.silence_samples += chunk.len();
                    let silence_secs =
                        self.silence_samples as f32 / self.sample_rate as f32;

                    if silence_secs >= SILENCE_AFTER_SPEECH_SECS {
                        let segment = &self.buffer[self.onset_sample..self.speech_end];
                        let dur = segment.len() as f32 / self.sample_rate as f32;

                        if dur >= MIN_SEGMENT_SECS {
                            events.push(VadEvent::Segment(segment.to_vec()));
                        }

                        self.buffer.clear();
                        self.silence_samples = 0;
                        self.in_speech = false;
                    }
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> VadConfig {
        VadConfig { sample_rate: 16000, threshold: 0.01 }
    }

    #[test]
    fn silence_produces_no_events() {
        let mut vad = VadDetector::new(&make_config());
        let silence = vec![0.0f32; 16000]; // 1 second of silence
        let events = vad.feed(&silence);
        assert!(events.is_empty());
    }

    #[test]
    fn speech_then_silence_produces_segment() {
        let mut vad = VadDetector::new(&make_config());
        // 0.5s of "speech" (above threshold)
        let speech: Vec<f32> = vec![0.1; 8000];
        // 1s of silence (below threshold) to trigger end-of-segment
        let silence: Vec<f32> = vec![0.0; 16000];

        let mut all_events = vad.feed(&speech);
        all_events.extend(vad.feed(&silence));

        assert_eq!(all_events.len(), 1);
        assert!(matches!(&all_events[0], VadEvent::Segment(s) if !s.is_empty()));
    }

    #[test]
    fn short_speech_is_discarded() {
        let mut vad = VadDetector::new(&make_config());
        // 0.1s of speech (below MIN_SEGMENT_SECS = 0.3)
        let speech: Vec<f32> = vec![0.1; 1600];
        let silence: Vec<f32> = vec![0.0; 16000];

        let mut all_events = vad.feed(&speech);
        all_events.extend(vad.feed(&silence));

        assert!(all_events.is_empty());
    }
}
```

- [ ] **Step 2: Add `pub mod vad;` to src/lib.rs**

Add after the `pub mod continuous;` line:
```rust
pub mod vad;
```

- [ ] **Step 3: Run VAD tests to verify they pass**

Run: `cargo test --lib vad -- --nocapture`
Expected: all 3 tests PASS

- [ ] **Step 4: Update ContinuousActor to use shared VadDetector**

In `src/continuous/mod.rs`:
- Replace `use vad::EnergyVad;` with `use crate::vad::{VadConfig, VadDetector};`
- Remove `pub mod vad;` from the module declarations
- In `init_runtime()`, replace:
  ```rust
  let vad = EnergyVad::new(
      config.audio.sample_rate,
      config.audio.silence_threshold as f32,
  );
  ```
  with:
  ```rust
  let vad = VadDetector::new(&VadConfig {
      sample_rate: config.audio.sample_rate,
      threshold: config.audio.silence_threshold as f32,
  });
  ```
- Change `RuntimeState` field type from `vad: EnergyVad` to `vad: VadDetector`
- In `vad_feed()`, replace:
  ```rust
  let mut segments: Vec<Vec<f32>> = Vec::new();
  state.vad.feed(chunk, &mut |segment| {
      segments.push(segment.to_vec());
  });
  ```
  with:
  ```rust
  let events = state.vad.feed(chunk);
  let segments: Vec<Vec<f32>> = events
      .into_iter()
      .map(|e| match e {
          crate::vad::VadEvent::Segment(s) => s,
      })
      .collect();
  ```

- [ ] **Step 5: Delete src/continuous/vad.rs**

Run: `rm src/continuous/vad.rs`

- [ ] **Step 5b: Update tests/vad_test.rs to use new VadDetector API**

Replace `tests/vad_test.rs` contents:
```rust
//! Tests for VAD (Voice Activity Detection) module.

use voicerouter::vad::{VadConfig, VadDetector, VadEvent};

#[test]
fn detects_speech_segment() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });

    // Feed 500ms silence
    let silence = vec![0.001f32; 8000];
    let events = vad.feed(&silence);
    assert!(events.is_empty());

    // Feed 500ms speech (loud signal)
    let speech: Vec<f32> = (0..8000).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    let events = vad.feed(&speech);
    assert!(events.is_empty(), "segment should not emit during speech");

    // Feed 1s silence to trigger end-of-speech
    let silence = vec![0.001f32; 16000];
    let mut events = vad.feed(&silence);

    if events.is_empty() {
        events = vad.feed(&silence);
    }

    assert_eq!(events.len(), 1, "should emit exactly one segment");
    assert!(matches!(&events[0], VadEvent::Segment(s) if !s.is_empty()));
}

#[test]
fn ignores_pure_silence() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    let silence = vec![0.001f32; 32000];
    let events = vad.feed(&silence);
    assert!(events.is_empty());
}

#[test]
fn minimum_segment_length() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    // Feed very short speech (50ms) — too short, should be discarded
    let short_speech: Vec<f32> = (0..800).map(|i| 0.3 * (i as f32 * 0.1).sin()).collect();
    let events = vad.feed(&short_speech);
    assert!(events.is_empty());

    let silence = vec![0.001f32; 16000];
    let mut events = vad.feed(&silence);
    events.extend(vad.feed(&silence));
    assert!(events.is_empty(), "very short speech should be discarded");
}

#[test]
fn in_speech_accessor() {
    let mut vad = VadDetector::new(&VadConfig { sample_rate: 16000, threshold: 0.02 });
    assert!(!vad.in_speech());
    let speech: Vec<f32> = vec![0.1; 800];
    let _ = vad.feed(&speech);
    assert!(vad.in_speech());
}
```

- [ ] **Step 6: Run all tests to verify nothing is broken**

Run: `cargo test`
Expected: all tests PASS (continuous tests + new vad tests)

- [ ] **Step 7: Commit**

```bash
git add src/vad/mod.rs src/continuous/mod.rs src/lib.rs
git rm src/continuous/vad.rs
git commit -m "refactor(vad): extract energy VAD to shared module"
```

---

### Task 4: Extend LLM Client

Add `chat()` method, make `ChatMessage` pub, make api_key optional.

**Files:**
- Modify: `src/llm/client.rs`

- [ ] **Step 1: Write failing tests for chat()**

Add to the bottom of `src/llm/client.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chat_response_valid() {
        let json = r#"{"reply": "你好", "confidence": 0.9}"#;
        let resp = parse_chat_json(json).unwrap();
        assert_eq!(resp.reply, "你好");
        assert!((resp.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_chat_response_missing_confidence() {
        let json = r#"{"reply": "你好"}"#;
        let resp = parse_chat_json(json).unwrap();
        assert_eq!(resp.confidence, 0.0);
    }

    #[test]
    fn parse_chat_response_invalid_json() {
        let result = parse_chat_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn clamp_confidence() {
        let json = r#"{"reply": "ok", "confidence": 1.5}"#;
        let resp = parse_chat_json(json).unwrap();
        assert!((resp.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn negative_confidence() {
        let json = r#"{"reply": "ok", "confidence": -0.5}"#;
        let resp = parse_chat_json(json).unwrap();
        assert!((resp.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn chat_message_is_pub() {
        // Compile-time check: ChatMessage is accessible.
        let msg = ChatMessage { role: "user".into(), content: "hi".into() };
        assert_eq!(msg.role, "user");
    }

    #[test]
    fn llm_client_no_api_key_succeeds() {
        // When api_key_env is empty, client creation should succeed (no env var lookup).
        let config = LlmConfig {
            endpoint: "http://localhost:11434/v1".into(),
            model: "test".into(),
            api_key_env: String::new(),
        };
        let client = LlmClient::new(&config);
        assert!(client.is_ok(), "should create client without api_key");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib llm -- --nocapture`
Expected: FAIL — `parse_chat_json` and `ChatMessage` not found

- [ ] **Step 3: Implement changes**

In `src/llm/client.rs`:

1. Make `ChatMessage` pub and add `Deserialize`:
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
```

2. Add `ConversationResponse` and `parse_chat_json`:
```rust
#[derive(Debug, Deserialize)]
pub struct ConversationResponse {
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub confidence: f64,
}

pub fn parse_chat_json(json: &str) -> Result<ConversationResponse> {
    let mut resp: ConversationResponse =
        serde_json::from_str(json).context("failed to parse conversation JSON")?;
    resp.confidence = resp.confidence.clamp(0.0, 1.0);
    Ok(resp)
}
```

3. Add `ConversationChatRequest` struct:
```rust
#[derive(Serialize)]
struct ConversationChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    fmt_type: String,
}
```

4. Make api_key optional in `LlmClient::new()`:
```rust
pub fn new(config: &LlmConfig) -> Result<Self> {
    let api_key = if config.api_key_env.is_empty() {
        String::new()
    } else {
        std::env::var(&config.api_key_env)
            .with_context(|| format!("missing env var: {}", config.api_key_env))?
    };
    Ok(Self {
        endpoint: config.endpoint.clone(),
        model: config.model.clone(),
        api_key,
    })
}
```

5. Add `chat()` method to `LlmClient`:
```rust
/// Send a multi-turn conversation request and parse the JSON response.
pub fn chat(
    &self,
    messages: &[ChatMessage],
    timeout_secs: u64,
) -> Result<ConversationResponse> {
    let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
    let request = ConversationChatRequest {
        model: self.model.clone(),
        messages: messages.to_vec(),
        stream: false,
        response_format: ResponseFormat { fmt_type: "json_object".into() },
    };
    let body = serde_json::to_string(&request)
        .context("failed to serialize conversation request")?;

    let mut req = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(timeout_secs));

    if !self.api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", self.api_key));
    }

    let response = req.send_string(&body).context("conversation API request failed")?;
    let response_text = response.into_string()
        .context("failed to read conversation API response body")?;
    let chat_resp: ChatResponse =
        serde_json::from_str(&response_text).context("failed to parse API response")?;
    let content = chat_resp
        .choices
        .first()
        .map(|c| c.message.content.as_str())
        .unwrap_or("{}");
    parse_chat_json(content)
}
```

6. Update `classify()` to also skip Authorization when api_key is empty (same pattern).

7. Update `src/llm/mod.rs` re-exports:
```rust
pub use client::{
    LlmClient, LlmResponse, ChatMessage, ConversationResponse,
    build_system_prompt, parse_llm_response, parse_chat_json,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib llm -- --nocapture`
Expected: all tests PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/llm/client.rs src/llm/mod.rs
git commit -m "feat(llm): add chat() method for conversation mode with optional api_key"
```

---

### Task 5: Config Changes

Add `ConversationConfig`, `WakewordAction::StartConversation`, mutual exclusivity validation.

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/config.rs` tests module:
```rust
#[test]
fn conversation_config_defaults() {
    let config = Config::default();
    assert!(!config.conversation.enabled);
    assert_eq!(config.conversation.timeout_seconds, 30.0);
    assert_eq!(config.conversation.max_turn_seconds, 30.0);
    assert_eq!(config.conversation.confidence_high, 0.8);
    assert_eq!(config.conversation.confidence_low, 0.5);
}

#[test]
fn conversation_config_deserializes() {
    let toml = r#"
[conversation]
enabled = true
timeout_seconds = 60
[conversation.llm]
endpoint = "http://localhost:11434/v1"
model = "qwen3.5:4b"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert!(config.conversation.enabled);
    assert_eq!(config.conversation.timeout_seconds, 60.0);
    assert_eq!(config.conversation.llm.endpoint, "http://localhost:11434/v1");
}

#[test]
fn wakeword_action_start_conversation() {
    let toml = "[wakeword]\naction = \"start_conversation\"\n";
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert_eq!(config.wakeword.action, WakewordAction::StartConversation);
}

#[test]
fn mutual_exclusivity_validation() {
    let mut config = Config::default();
    config.conversation.enabled = true;
    config.continuous.enabled = true;
    assert!(config.validate().is_err());
}

#[test]
fn mutual_exclusivity_one_enabled_ok() {
    let mut config = Config::default();
    config.conversation.enabled = true;
    config.continuous.enabled = false;
    assert!(config.validate().is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config -- --nocapture`
Expected: FAIL — no `conversation` field, no `validate()`, no `StartConversation`

- [ ] **Step 3: Implement config changes**

Add to `src/config.rs`:

1. Add `StartConversation` to `WakewordAction` enum:
```rust
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WakewordAction {
    #[default]
    StartRecording,
    PipelinePassthrough,
    StartConversation,
}
```

2. Add `ConversationLlmConfig`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversationLlmConfig {
    pub endpoint: String,
    pub model: String,
    pub system_prompt: String,
}

impl Default for ConversationLlmConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:11434/v1".to_owned(),
            model: "qwen3.5:4b".to_owned(),
            system_prompt: "你是一个简洁的语音助手。用口语化的中文回答，保持简短。\
                必须以JSON格式回复，包含reply(回答文本)和confidence(0-1置信度)两个字段。"
                .to_owned(),
        }
    }
}
```

3. Add `ConversationConfig`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversationConfig {
    pub enabled: bool,
    pub timeout_seconds: f64,
    pub max_turn_seconds: f64,
    pub end_phrases: Vec<String>,
    pub confidence_high: f64,
    pub confidence_low: f64,
    pub low_confidence_prefix: String,
    pub low_confidence_reject: String,
    pub llm_timeout_seconds: u64,
    pub llm: ConversationLlmConfig,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_seconds: 30.0,
            max_turn_seconds: 30.0,
            end_phrases: vec!["结束".into(), "再见".into(), "没事了".into()],
            confidence_high: 0.8,
            confidence_low: 0.5,
            low_confidence_prefix: "我不太确定，".into(),
            low_confidence_reject: "抱歉，我无法回答这个问题".into(),
            llm_timeout_seconds: 15,
            llm: ConversationLlmConfig::default(),
        }
    }
}
```

4. Add `conversation` field to `Config`:
```rust
pub struct Config {
    // ... existing fields ...
    pub conversation: ConversationConfig,
}
```

5. Add `validate()` method to `Config`:
```rust
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.conversation.enabled && self.continuous.enabled {
            anyhow::bail!(
                "conversation and continuous modes are mutually exclusive; \
                 disable one of them in config"
            );
        }
        Ok(())
    }
    // ... existing methods ...
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config -- --nocapture`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add conversation config, WakewordAction::StartConversation, validation"
```

---

### Task 6: Message Types

Add `StartConversation` and `EndConversation` to the Message enum.

**Files:**
- Modify: `src/actor.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/actor.rs` tests:
```rust
#[test]
fn conversation_message_topics() {
    assert_eq!(
        Message::StartConversation { wakeword: Some("hey".into()) }.topic(),
        "StartConversation"
    );
    assert_eq!(Message::EndConversation.topic(), "EndConversation");
}

#[test]
fn bus_routes_start_conversation() {
    let (tx, rx) = crossbeam::channel::bounded(8);
    let mut bus = Bus::new();
    bus.subscribe("StartConversation", tx);
    bus.publish(Message::StartConversation { wakeword: None });
    let received = rx.try_recv().unwrap();
    assert!(matches!(received, Message::StartConversation { .. }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib actor -- --nocapture`
Expected: FAIL — no `StartConversation` variant

- [ ] **Step 3: Add message variants**

In `src/actor.rs`, add to the `Message` enum:
```rust
/// Wake word triggers multi-turn conversation mode in ConversationActor.
/// Distinct from StartListening which triggers single-shot recording in CoreActor.
StartConversation { wakeword: Option<String> },
/// End the active conversation session.
EndConversation,
```

Add to the `topic()` match:
```rust
Self::StartConversation { .. } => "StartConversation",
Self::EndConversation => "EndConversation",
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib actor -- --nocapture`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/actor.rs
git commit -m "feat(actor): add StartConversation and EndConversation messages"
```

---

### Task 7: Wakeword Action Handler

Add `StartConversation` handling to `emit_action()`.

**Files:**
- Modify: `src/wakeword/mod.rs`

- [ ] **Step 1: Write failing test**

Add to `src/wakeword/mod.rs` tests:
```rust
#[test]
fn emit_action_start_conversation() {
    let (tx, rx) = crossbeam::channel::bounded(8);
    let mut config = Config::default();
    config.wakeword.action = crate::config::WakewordAction::StartConversation;
    emit_action(&config, &tx, "小助手", "你好");
    let msg = rx.try_recv().unwrap();
    assert!(matches!(msg, Message::StartConversation { wakeword } if wakeword == Some("小助手".into())));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib wakeword -- --nocapture`
Expected: FAIL — non-exhaustive match

- [ ] **Step 3: Add StartConversation arm to emit_action**

In `src/wakeword/mod.rs`, add to `emit_action()` match:
```rust
crate::config::WakewordAction::StartConversation => {
    outbox
        .send(Message::StartConversation {
            wakeword: Some(phrase.to_string()),
        })
        .ok();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib wakeword -- --nocapture`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/wakeword/mod.rs
git commit -m "feat(wakeword): handle StartConversation action"
```

---

### Task 8: ConversationActor

The main actor with state machine, VAD loop, LLM call, TTS dispatch.

**Files:**
- Modify: `src/conversation/mod.rs`

- [ ] **Step 1: Write failing unit tests for state machine and confidence logic**

Add to `src/conversation/mod.rs`:
```rust
pub mod sentence;
pub mod session;

use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message, SpeakSource};
use crate::asr::AsrEngine;
use crate::audio_source::AudioChunk;
use crate::config::Config;
use crate::llm::{LlmClient, ConversationResponse};
use crate::vad::{VadConfig, VadDetector, VadEvent};

use sentence::split_sentences;
use session::Session;

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Listening,
    Recording,
    Transcribing,
    Thinking,
    Speaking,
}

/// Determine the TTS text based on confidence thresholds.
fn apply_confidence(
    reply: &str,
    confidence: f64,
    high: f64,
    low: f64,
    prefix: &str,
    reject: &str,
) -> String {
    if confidence >= high {
        reply.to_string()
    } else if confidence >= low {
        format!("{prefix}{reply}")
    } else {
        reject.to_string()
    }
}

pub struct ConversationActor {
    config: Config,
    audio_rx: Receiver<AudioChunk>,
}

impl ConversationActor {
    #[must_use]
    pub fn new(config: Config, audio_rx: Receiver<AudioChunk>) -> Self {
        Self { config, audio_rx }
    }
}

// Actor::run implementation will be added in the next step.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_high_returns_reply() {
        let result = apply_confidence("你好", 0.9, 0.8, 0.5, "前缀", "拒绝");
        assert_eq!(result, "你好");
    }

    #[test]
    fn confidence_medium_adds_prefix() {
        let result = apply_confidence("你好", 0.6, 0.8, 0.5, "我不确定，", "拒绝");
        assert_eq!(result, "我不确定，你好");
    }

    #[test]
    fn confidence_low_returns_reject() {
        let result = apply_confidence("你好", 0.3, 0.8, 0.5, "前缀", "无法回答");
        assert_eq!(result, "无法回答");
    }

    #[test]
    fn confidence_at_boundary_high() {
        let result = apply_confidence("ok", 0.8, 0.8, 0.5, "pfx", "rej");
        assert_eq!(result, "ok");
    }

    #[test]
    fn confidence_at_boundary_low() {
        let result = apply_confidence("ok", 0.5, 0.8, 0.5, "pfx:", "rej");
        assert_eq!(result, "pfx:ok");
    }

    #[test]
    fn conversation_actor_name() {
        let (_tx, rx) = crossbeam::channel::bounded(1);
        let actor = ConversationActor::new(Config::default(), rx);
        assert_eq!(Actor::name(&actor), "conversation");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib conversation -- --nocapture`
Expected: FAIL — `Actor::name` not implemented

- [ ] **Step 3: Implement the Actor trait and full run() loop**

Add the full implementation to `src/conversation/mod.rs`. The `run()` method implements the state machine:

```rust
impl Actor for ConversationActor {
    fn name(&self) -> &str {
        "conversation"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let conv = &self.config.conversation;
        if !conv.enabled {
            log::info!("[conversation] disabled, actor idle");
            idle_loop(&inbox, &self.audio_rx);
            return;
        }

        // Warmup ping to Ollama.
        let llm_config = crate::config::LlmConfig {
            endpoint: conv.llm.endpoint.clone(),
            model: conv.llm.model.clone(),
            api_key_env: String::new(),
        };
        let llm = match LlmClient::new(&llm_config) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[conversation] LLM client init failed: {e:#}");
                idle_loop(&inbox, &self.audio_rx);
                return;
            }
        };
        warmup_ping(&llm, conv.llm_timeout_seconds);

        let mut state = State::Idle;
        let mut session: Option<Session> = None;
        let mut vad: Option<VadDetector> = None;
        let mut asr_engine: Option<AsrEngine> = None;
        let mut audio_buffer: Vec<f32> = Vec::new();
        let mut pending_sentences: usize = 0;
        let mut recording_start: Option<Instant> = None;
        let mut muted = false;

        log::info!("[conversation] ready");

        loop {
            // Drain control messages.
            while let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::Shutdown => {
                        if session.is_some() {
                            end_session(&outbox);
                        }
                        log::info!("[conversation] stopped");
                        return;
                    }
                    Message::StartConversation { wakeword } => {
                        if state == State::Idle {
                            log::info!("[conversation] starting session (wakeword: {wakeword:?})");
                            session = Some(Session::new(
                                conv.llm.system_prompt.clone(),
                                conv.end_phrases.clone(),
                            ));
                            vad = Some(VadDetector::new(&VadConfig {
                                sample_rate: self.config.audio.sample_rate,
                                threshold: self.config.audio.silence_threshold as f32,
                            }));
                            state = State::Listening;
                            outbox.send(Message::MuteInput).ok();
                        }
                    }
                    Message::EndConversation => {
                        if state != State::Idle {
                            speak_text("好的，再见", &outbox);
                            end_session(&outbox);
                            state = State::Idle;
                            session = None;
                            vad = None;
                            pending_sentences = 0;
                        }
                    }
                    Message::SpeakDone => {
                        if state == State::Speaking {
                            pending_sentences = pending_sentences.saturating_sub(1);
                            if pending_sentences == 0 {
                                state = State::Listening;
                                if let Some(ref mut s) = session {
                                    s.last_activity = Instant::now();
                                }
                                log::debug!("[conversation] all sentences spoken, listening");
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Check session timeout.
            if state == State::Listening {
                if let Some(ref s) = session {
                    if s.is_timed_out(conv.timeout_seconds) {
                        log::info!("[conversation] session timed out");
                        end_session(&outbox);
                        state = State::Idle;
                        session = None;
                        vad = None;
                        continue;
                    }
                }
            }

            // Process audio when in active states.
            if matches!(state, State::Listening | State::Recording) {
                match self.audio_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(chunk) => {
                        if let Some(ref mut v) = vad {
                            let events = v.feed(&chunk);
                            match state {
                                State::Listening => {
                                    // Check for speech onset.
                                    for event in events {
                                        if let VadEvent::Segment(segment) = event {
                                            // VAD already delivered a complete segment.
                                            state = State::Transcribing;
                                            audio_buffer = segment;
                                            break;
                                        }
                                    }
                                    // If VAD is in-speech but hasn't emitted segment yet,
                                    // we transition to Recording (accumulating).
                                    if state == State::Listening && v.in_speech() {
                                        state = State::Recording;
                                        recording_start = Some(Instant::now());
                                    }
                                }
                                State::Recording => {
                                    for event in events {
                                        if let VadEvent::Segment(segment) = event {
                                            state = State::Transcribing;
                                            audio_buffer = segment;
                                            break;
                                        }
                                    }
                                    // Max turn duration guard.
                                    if state == State::Recording {
                                        if let Some(start) = recording_start {
                                            if start.elapsed().as_secs_f64()
                                                >= conv.max_turn_seconds
                                            {
                                                log::warn!(
                                                    "[conversation] max turn duration reached"
                                                );
                                                // Force transcribe with VadDetector's buffer.
                                                // Per spec: Recording → Transcribing on max_turn.
                                                state = State::Transcribing;
                                                recording_start = None;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(_) => {} // timeout
                }
            } else if state == State::Idle {
                // In Idle, just drain audio to prevent channel backup.
                let _ = self.audio_rx.recv_timeout(Duration::from_millis(50));
            } else {
                // Transcribing/Thinking/Speaking — sleep briefly.
                std::thread::sleep(Duration::from_millis(10));
            }

            // Handle Transcribing state.
            if state == State::Transcribing {
                recording_start = None;

                // Lazy-init ASR.
                if asr_engine.is_none() {
                    match AsrEngine::new(&self.config.asr) {
                        Ok(e) => asr_engine = Some(e),
                        Err(e) => {
                            log::error!("[conversation] ASR init failed: {e:#}");
                            speak_text("语音识别初始化失败", &outbox);
                            end_session(&outbox);
                            state = State::Idle;
                            session = None;
                            vad = None;
                            continue;
                        }
                    }
                }

                let transcript = match asr_engine
                    .as_mut()
                    .unwrap()
                    .transcribe(&audio_buffer, self.config.audio.sample_rate)
                {
                    Ok(t) if !t.is_empty() => t,
                    Ok(_) => {
                        log::debug!("[conversation] empty transcript, back to listening");
                        state = State::Listening;
                        continue;
                    }
                    Err(e) => {
                        log::error!("[conversation] transcription failed: {e:#}");
                        state = State::Listening;
                        continue;
                    }
                };
                audio_buffer.clear();

                log::info!("[conversation] transcript: {transcript:?}");

                // Check end phrase.
                if let Some(ref s) = session {
                    if s.is_end_phrase(&transcript) {
                        log::info!("[conversation] end phrase detected");
                        speak_text("好的，再见", &outbox);
                        end_session(&outbox);
                        state = State::Idle;
                        session = None;
                        vad = None;
                        continue;
                    }
                }

                // Add to session and move to Thinking.
                if let Some(ref mut s) = session {
                    s.add_user_message(&transcript);
                }
                state = State::Thinking;

                // Call LLM (with retry).
                // Session::messages() returns Vec<crate::llm::ChatMessage> directly.
                let messages = session.as_ref().unwrap().messages();

                let resp = llm.chat(&messages, conv.llm_timeout_seconds)
                    .or_else(|e| {
                        log::warn!("[conversation] LLM request failed: {e:#}, retrying");
                        llm.chat(&messages, conv.llm_timeout_seconds)
                    });

                match resp {
                    Ok(resp) => {
                        log::info!(
                            "[conversation] LLM reply (confidence={:.2}): {:?}",
                            resp.confidence, resp.reply
                        );
                        let text = apply_confidence(
                            &resp.reply, resp.confidence,
                            conv.confidence_high, conv.confidence_low,
                            &conv.low_confidence_prefix, &conv.low_confidence_reject,
                        );
                        if let Some(ref mut s) = session {
                            s.add_assistant_message(&resp.reply);
                        }
                        let sentences = split_sentences(&text);
                        if sentences.is_empty() {
                            state = State::Listening;
                            continue;
                        }
                        pending_sentences = sentences.len();
                        state = State::Speaking;
                        for sentence in &sentences {
                            speak_reply(sentence, &outbox);
                        }
                    }
                    Err(e) => {
                        log::error!("[conversation] LLM failed after retry: {e:#}");
                        speak_text("语音助手暂时不可用", &outbox);
                        end_session(&outbox);
                        state = State::Idle;
                        session = None;
                        vad = None;
                    }
                }
            }
        }
    }
}

fn speak_reply(text: &str, outbox: &Sender<Message>) {
    outbox
        .send(Message::SpeakRequest {
            text: text.to_string(),
            source: SpeakSource::LlmReply,
        })
        .ok();
}

fn speak_text(text: &str, outbox: &Sender<Message>) {
    outbox
        .send(Message::SpeakRequest {
            text: text.to_string(),
            source: SpeakSource::SystemFeedback,
        })
        .ok();
}

fn end_session(outbox: &Sender<Message>) {
    outbox.send(Message::UnmuteInput).ok();
}

fn idle_loop(inbox: &Receiver<Message>, audio_rx: &Receiver<AudioChunk>) {
    loop {
        crossbeam::select! {
            recv(inbox) -> msg => {
                if matches!(msg, Ok(Message::Shutdown)) { break; }
            }
            recv(audio_rx) -> _ => {} // discard
        }
    }
}

fn warmup_ping(llm: &LlmClient, timeout_secs: u64) {
    let msgs = vec![crate::llm::ChatMessage {
        role: "user".into(),
        content: "hi".into(),
    }];
    match llm.chat(&msgs, timeout_secs) {
        Ok(_) => log::info!("[conversation] Ollama warmup OK"),
        Err(e) => log::warn!("[conversation] Ollama warmup failed (non-fatal): {e:#}"),
    }
}
```

- [ ] **Step 4: Run conversation tests**

Run: `cargo test --lib conversation -- --nocapture`
Expected: all tests PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/conversation/mod.rs src/vad/mod.rs
git commit -m "feat(conversation): implement ConversationActor with state machine"
```

---

### Task 9: Wire Into Daemon

Connect ConversationActor in `main.rs` with audio channel and bus subscriptions.

**Files:**
- Modify: `src/main.rs`
- Modify: `src/config.rs` (call validate)

- [ ] **Step 1: Add config validation to main**

In `run_daemon()`, after loading config:
```rust
config.validate()?;
```

- [ ] **Step 2: Add ConversationActor channel and bus subscriptions**

In `run_daemon()`, after the continuous actor channel block, add:
```rust
// Conversation actor channel + bus subscriptions (only when enabled).
let conversation_channels = if config.conversation.enabled {
    let (tx, rx) = crossbeam::channel::bounded::<Message>(32);
    bus.subscribe("StartConversation", tx.clone());
    bus.subscribe("EndConversation", tx.clone());
    bus.subscribe("SpeakDone", tx.clone());
    bus.subscribe("Shutdown", tx.clone());
    Some((tx, rx))
} else {
    None
};
```

- [ ] **Step 3: Add conversation audio subscriber**

After the `continuous_audio_rx` block, add:
```rust
let conversation_audio_rx = if config.conversation.enabled {
    let (tx, rx) = crossbeam::channel::bounded::<voicerouter::audio_source::AudioChunk>(256);
    audio_subscribers.push(tx);
    Some(rx)
} else {
    None
};
```

- [ ] **Step 4: Spawn ConversationActor**

After the continuous actor spawn block, add:
```rust
let conversation_handle = if let (Some((_conv_tx, conv_rx)), Some(conv_audio_rx)) =
    (conversation_channels, conversation_audio_rx)
{
    let conversation_actor = voicerouter::conversation::ConversationActor::new(
        config.clone(),
        conv_audio_rx,
    );
    let bus_tx_conversation = bus_tx.clone();
    Some(
        std::thread::Builder::new()
            .name("conversation".into())
            .spawn(move || conversation_actor.run(conv_rx, bus_tx_conversation))?,
    )
} else {
    if config.conversation.enabled {
        log::warn!("[conversation] enabled but failed to wire channels");
    } else {
        log::info!("[conversation] disabled");
    }
    None
};
```

- [ ] **Step 5: Add conversation handle to shutdown join list**

In the handles vec:
```rust
if let Some(h) = conversation_handle {
    handles.push(h);
}
```

- [ ] **Step 6: Run cargo build to verify compilation**

Run: `cargo build`
Expected: compiles without errors

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/config.rs
git commit -m "feat(main): wire ConversationActor into daemon with audio and bus"
```

---

### Task 10: Update Default Config

Add conversation section to the default config file.

**Files:**
- Modify: `config.default.toml`

- [ ] **Step 1: Add conversation section**

Append to `config.default.toml`:
```toml
# ── Conversation Mode ────────────────────────────────────────────────
# Multi-turn voice conversation with LLM. Mutually exclusive with [continuous].
[conversation]
enabled = false
timeout_seconds = 30          # inactivity timeout (from last turn)
max_turn_seconds = 30         # max recording duration per turn
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
```

- [ ] **Step 2: Commit**

```bash
git add config.default.toml
git commit -m "docs(config): add conversation mode defaults"
```

---

### Task 11: Integration Test

End-to-end test with mocked audio and HTTP.

**Files:**
- Create: `tests/conversation_integration.rs` (or add to existing integration test file if one exists)

- [ ] **Step 1: Check for existing integration test structure**

Run: `ls tests/` to see if integration tests exist.

- [ ] **Step 2: Write integration test**

Create `tests/conversation_integration.rs`:
```rust
//! Integration tests for conversation mode.
//!
//! These tests verify the ConversationActor state machine using
//! direct channel manipulation (no real audio/LLM).

use crossbeam::channel;
use voicerouter::actor::Message;
use voicerouter::conversation::sentence::split_sentences;
use voicerouter::conversation::session::Session;
use voicerouter::llm::parse_chat_json;

#[test]
fn session_full_lifecycle() {
    let mut s = Session::new(
        "system prompt".into(),
        vec!["结束".into(), "再见".into()],
    );

    // Turn 1
    s.add_user_message("你好");
    s.add_assistant_message("你好！有什么可以帮你的？");
    assert_eq!(s.turn_count(), 1);

    // Turn 2
    s.add_user_message("今天天气怎么样");
    s.add_assistant_message("今天晴天。");
    assert_eq!(s.turn_count(), 2);

    // Full message list
    let msgs = s.messages();
    assert_eq!(msgs.len(), 5); // system + 4 messages
    assert_eq!(msgs[0].role, "system");

    // End phrase
    assert!(s.is_end_phrase("结束"));
    assert!(!s.is_end_phrase("继续聊"));
}

#[test]
fn sentence_splitter_with_llm_output() {
    let reply = "今天天气很好。最高温度25度，适合出行！你还想知道什么？";
    let sentences = split_sentences(reply);
    assert_eq!(sentences.len(), 3);
}

#[test]
fn parse_ollama_json_response() {
    let json = r#"{"reply": "今天晴天，最高25度。", "confidence": 0.85}"#;
    let resp = parse_chat_json(json).unwrap();
    assert_eq!(resp.reply, "今天晴天，最高25度。");
    assert!((resp.confidence - 0.85).abs() < f64::EPSILON);
}

#[test]
fn parse_malformed_ollama_response() {
    // Missing closing brace
    assert!(parse_chat_json(r#"{"reply": "hi""#).is_err());
}

#[test]
fn conversation_messages_roundtrip() {
    // Verify StartConversation routes through bus correctly.
    let (tx, rx) = channel::bounded(8);
    let mut bus = voicerouter::actor::Bus::new();
    bus.subscribe("StartConversation", tx);
    bus.publish(Message::StartConversation {
        wakeword: Some("小助手".into()),
    });
    let msg = rx.try_recv().unwrap();
    assert!(matches!(
        msg,
        Message::StartConversation { wakeword: Some(w) } if w == "小助手"
    ));
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test conversation_integration -- --nocapture`
Expected: all tests PASS

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add tests/conversation_integration.rs
git commit -m "test(conversation): add integration tests"
```

---

### Task 12: Release Build and Manual Verification

Build release binary and verify with real Ollama.

**Files:** None (manual testing)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release`
Expected: compiles without warnings

- [ ] **Step 2: Verify Ollama is running**

Run: `curl -s http://localhost:11434/v1/models | head -20`
Expected: JSON with model list including qwen3.5:4b

- [ ] **Step 3: Update user's config.toml**

Enable conversation mode in `~/.config/voicerouter/config.toml`:
```toml
[conversation]
enabled = true

[conversation.llm]
endpoint = "http://localhost:11434/v1"
model = "qwen3.5:4b"

[wakeword]
enabled = true
action = "start_conversation"
```

Ensure `[continuous]` has `enabled = false`.

- [ ] **Step 4: Run daemon and test**

Run: `./target/release/voicerouter -v`
Test: Say wake word, then ask a question. Verify:
1. Wake word triggers conversation mode (log: "starting session")
2. VAD detects speech (log: "VAD segment")
3. ASR transcribes (log: "transcript")
4. Ollama responds (log: "LLM reply")
5. TTS plays response
6. System returns to listening for next turn
7. Saying "结束" ends the session

- [ ] **Step 5: Verify all changes are committed**

All code changes should already be committed from previous tasks. Run `git status` to confirm no outstanding changes. If there are config changes from manual testing, commit them specifically:
```bash
git status
```
