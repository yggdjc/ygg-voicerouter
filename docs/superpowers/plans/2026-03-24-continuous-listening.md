# Continuous Listening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add continuous listening mode with VAD, speaker verification, local intent filtering, and remote LLM fallback — coexisting with hotkey and wakeword modes.

**Architecture:** New ContinuousActor orchestrates the VAD→Speaker→ASR→Intent→Execute pipeline. Speaker verification and intent filtering are standalone modules callable from the actor. LLM judge uses OpenAI-compatible HTTP API via reqwest.

**Tech Stack:** silero-vad ONNX, sherpa-onnx speaker embedding, reqwest (existing), serde_json (existing)

**Spec:** `docs/superpowers/specs/2026-03-24-continuous-listening-design.md`

---

## File Structure

```
src/
├── continuous/
│   ├── mod.rs              # ContinuousActor — orchestrates VAD→Speaker→ASR→Intent→Execute
│   ├── vad.rs              # Silero VAD wrapper — detect speech segments from audio stream
│   ├── speaker.rs          # Speaker enrollment + verification via sherpa-onnx embeddings
│   └── intent.rs           # Local rule-based intent filter (Command/Ambient/Uncertain)
├── llm/
│   ├── mod.rs              # LLM judge — OpenAI-compatible API intent classification
│   └── client.rs           # HTTP client wrapper for OpenAI-compatible endpoints
├── config.rs               # Add ContinuousConfig, LlmConfig
├── actor.rs                # Add SpeechSegment, IntentResult, ConfirmAction messages
├── main.rs                 # Wire ContinuousActor into daemon, add `enroll` CLI subcommand
└── pipeline/
    └── handler.rs          # Add risk_level() method to Handler trait
tests/
├── intent_test.rs          # IntentFilter unit tests
├── speaker_test.rs         # Speaker embedding cosine similarity tests
└── vad_test.rs             # VAD segment detection tests
```

---

### Task 1: Configuration Types

**Files:**
- Modify: `src/config.rs`
- Test: `tests/config_test.rs`

- [ ] **Step 1: Write failing test for ContinuousConfig deserialization**

```rust
#[test]
fn continuous_config_deserializes() {
    let toml = r#"
[continuous]
enabled = true
speaker_verify = true
speaker_threshold = 0.7
speaker_model = "3dspeaker"
vad_model = "silero"

[continuous.llm]
endpoint = "http://localhost:8080/v1"
model = "claude-haiku"
api_key_env = "TEST_KEY"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert!(config.continuous.enabled);
    assert_eq!(config.continuous.speaker_threshold, 0.7);
    assert_eq!(config.continuous.llm.model, "claude-haiku");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_test continuous_config_deserializes`
Expected: FAIL — no field `continuous` on Config

- [ ] **Step 3: Add ContinuousConfig and LlmConfig structs to config.rs**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key_env: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            model: "claude-haiku".to_owned(),
            api_key_env: "VOICEROUTER_LLM_KEY".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContinuousConfig {
    pub enabled: bool,
    pub speaker_verify: bool,
    pub speaker_threshold: f64,
    pub speaker_model: String,
    pub vad_model: String,
    pub llm: LlmConfig,
}

impl Default for ContinuousConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            speaker_verify: true,
            speaker_threshold: 0.6,
            speaker_model: "3dspeaker".to_owned(),
            vad_model: "silero".to_owned(),
            llm: LlmConfig::default(),
        }
    }
}
```

Add `pub continuous: ContinuousConfig` to `Config` struct.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test config_test continuous_config_deserializes`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs tests/config_test.rs
git commit -m "feat(config): add ContinuousConfig and LlmConfig types"
```

---

### Task 2: New Message Types

**Files:**
- Modify: `src/actor.rs`

- [ ] **Step 1: Add new message variants to Message enum**

```rust
pub enum Message {
    // ... existing variants ...

    /// A speech segment detected by VAD (continuous mode).
    SpeechSegment { samples: Vec<f32>, duration: f32 },

    /// Request user confirmation for high-risk action (continuous mode).
    ConfirmAction { text: String, stage: String },

    /// User confirmed a pending high-risk action.
    ActionConfirmed,

    /// User rejected or timeout on a pending high-risk action.
    ActionRejected,
}
```

- [ ] **Step 2: Add topic() match arms and update existing tests**

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add src/actor.rs
git commit -m "feat(actor): add SpeechSegment and ConfirmAction messages"
```

---

### Task 3: VAD Module

**Files:**
- Create: `src/continuous/mod.rs`
- Create: `src/continuous/vad.rs`
- Test: `tests/vad_test.rs`
- Modify: `src/lib.rs` — add `pub mod continuous;`

- [ ] **Step 1: Write failing test for VAD segment detection**

```rust
// tests/vad_test.rs
use voicerouter::continuous::vad::VadDetector;

#[test]
fn vad_detects_speech_in_loud_signal() {
    let mut vad = VadDetector::new_mock(); // threshold-based mock for unit test
    // Simulate: silence, speech, silence
    let silence = vec![0.0f32; 1600]; // 100ms @ 16kHz
    let speech = vec![0.3f32; 8000];   // 500ms loud signal
    let mut segments = Vec::new();
    vad.feed(&silence, |_seg| {});
    vad.feed(&speech, |_seg| {});
    vad.feed(&silence, |seg| segments.push(seg));
    vad.feed(&silence, |seg| segments.push(seg)); // ensure flush
    assert!(!segments.is_empty(), "should detect at least one segment");
}

#[test]
fn vad_ignores_pure_silence() {
    let mut vad = VadDetector::new_mock();
    let silence = vec![0.0f32; 16000]; // 1s silence
    let mut segments = Vec::new();
    vad.feed(&silence, |seg| segments.push(seg));
    assert!(segments.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement VadDetector**

Two implementations behind a trait:
- `SileroVad` — loads silero-vad ONNX model, real VAD inference
- `MockVad` — simple RMS threshold for unit testing without model files

VadDetector accumulates audio, calls model on windows, tracks speech onset/offset, emits complete segments via callback.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
git add src/continuous/ src/lib.rs tests/vad_test.rs
git commit -m "feat(continuous): add VAD module with Silero support"
```

---

### Task 4: Speaker Verification Module

**Files:**
- Create: `src/continuous/speaker.rs`
- Test: `tests/speaker_test.rs`

- [ ] **Step 1: Write failing test for cosine similarity**

```rust
use voicerouter::continuous::speaker::cosine_similarity;

#[test]
fn cosine_similarity_identical_vectors() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    let sim = cosine_similarity(&a, &b);
    assert!((sim - 1.0).abs() < 1e-6);
}

#[test]
fn cosine_similarity_orthogonal_vectors() {
    let a = vec![1.0, 0.0];
    let b = vec![0.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-6);
}

#[test]
fn speaker_verify_accepts_above_threshold() {
    let enrollment = vec![0.5, 0.5, 0.5];
    let sample = vec![0.49, 0.51, 0.5]; // very close
    let verifier = SpeakerVerifier::from_enrollment(enrollment, 0.6);
    assert!(verifier.verify(&sample));
}

#[test]
fn speaker_verify_rejects_below_threshold() {
    let enrollment = vec![1.0, 0.0, 0.0];
    let sample = vec![0.0, 1.0, 0.0]; // orthogonal
    let verifier = SpeakerVerifier::from_enrollment(enrollment, 0.6);
    assert!(!verifier.verify(&sample));
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement cosine_similarity and SpeakerVerifier**

```rust
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    dot / (norm_a * norm_b)
}

pub struct SpeakerVerifier {
    enrollment: Vec<f32>,
    threshold: f32,
}

impl SpeakerVerifier {
    pub fn from_enrollment(embedding: Vec<f32>, threshold: f32) -> Self {
        Self { enrollment: embedding, threshold }
    }

    pub fn verify(&self, sample_embedding: &[f32]) -> bool {
        cosine_similarity(&self.enrollment, sample_embedding) >= self.threshold
    }
}
```

Sherpa-onnx embedding extraction (SpeakerEmbedder) wraps the speaker model for runtime use. Enrollment CLI stores mean embedding to `~/.config/voicerouter/speaker.bin`.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
git add src/continuous/speaker.rs tests/speaker_test.rs
git commit -m "feat(continuous): add speaker verification module"
```

---

### Task 5: Intent Filter Module

**Files:**
- Create: `src/continuous/intent.rs`
- Test: `tests/intent_test.rs`

- [ ] **Step 1: Write failing tests for intent classification**

```rust
use voicerouter::continuous::intent::{IntentFilter, Intent};

#[test]
fn short_text_is_ambient() {
    let filter = IntentFilter::new(&["搜索", "打开"]);
    assert_eq!(filter.classify("嗯"), Intent::Ambient);
}

#[test]
fn filler_words_are_ambient() {
    let filter = IntentFilter::new(&["搜索"]);
    assert_eq!(filter.classify("啊呃嗯"), Intent::Ambient);
}

#[test]
fn trigger_prefix_is_command() {
    let filter = IntentFilter::new(&["搜索", "echo "]);
    assert_eq!(filter.classify("搜索Rust VAD"), Intent::Command);
}

#[test]
fn imperative_verb_is_command() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("帮我打开浏览器"), Intent::Command);
}

#[test]
fn declarative_is_uncertain() {
    let filter = IntentFilter::new(&[]);
    assert_eq!(filter.classify("今天天气不错啊"), Intent::Uncertain);
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement IntentFilter**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Command,
    Ambient,
    Uncertain,
}

pub struct IntentFilter {
    triggers: Vec<String>,
    imperative_prefixes: Vec<&'static str>,
}

impl IntentFilter {
    pub fn new(triggers: &[&str]) -> Self { ... }
    pub fn classify(&self, text: &str) -> Intent { ... }
}
```

Rules per spec: length < 2 → Ambient, filler → Ambient, trigger match → Command, imperative prefix → Command, otherwise → Uncertain.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
git add src/continuous/intent.rs tests/intent_test.rs
git commit -m "feat(continuous): add local intent filter"
```

---

### Task 6: LLM Client Module

**Files:**
- Create: `src/llm/mod.rs`
- Create: `src/llm/client.rs`
- Modify: `src/lib.rs` — add `pub mod llm;`

- [ ] **Step 1: Write failing test for LLM response parsing**

```rust
use voicerouter::llm::LlmResponse;

#[test]
fn parse_command_response() {
    let json = r#"{"intent":"command","action":"搜索","text":"Rust VAD"}"#;
    let resp: LlmResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.intent, "command");
    assert_eq!(resp.action, "搜索");
}

#[test]
fn parse_ambient_response() {
    let json = r#"{"intent":"ambient","action":"","text":""}"#;
    let resp: LlmResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.intent, "ambient");
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement LlmClient and LlmResponse**

```rust
#[derive(Debug, Deserialize)]
pub struct LlmResponse {
    pub intent: String,
    pub action: String,
    pub text: String,
}

pub struct LlmClient {
    endpoint: String,
    model: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl LlmClient {
    pub fn new(config: &LlmConfig) -> Result<Self> { ... }
    pub fn classify(&self, transcript: &str, available_actions: &[String]) -> Result<LlmResponse> { ... }
}
```

Uses OpenAI-compatible `/v1/chat/completions` endpoint. System prompt lists available pipeline actions. 5s timeout.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
git add src/llm/ src/lib.rs tests/
git commit -m "feat(llm): add OpenAI-compatible LLM client for intent classification"
```

---

### Task 7: Handler Risk Levels

**Files:**
- Modify: `src/pipeline/handler.rs`
- Modify: `src/pipeline/handlers/*.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn inject_handler_is_low_risk() {
    let handler = InjectHandler::new(InjectMethod::Auto);
    assert_eq!(handler.risk_level(), RiskLevel::Low);
}

#[test]
fn shell_handler_is_high_risk() {
    let handler = ShellHandler;
    assert_eq!(handler.risk_level(), RiskLevel::High);
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Add RiskLevel enum and risk_level() to Handler trait**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,  // inject, speak, transform
    High, // shell, http, pipe
}

pub trait Handler: Send + Sync {
    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult>;
    fn risk_level(&self) -> RiskLevel { RiskLevel::Low } // default low
}
```

Override `risk_level()` in ShellHandler, HttpHandler, PipeHandler to return `High`.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/
git commit -m "feat(pipeline): add risk levels to handlers"
```

---

### Task 8: Speaker Enrollment CLI

**Files:**
- Modify: `src/main.rs` — add `enroll` subcommand
- Uses: `src/continuous/speaker.rs`

- [ ] **Step 1: Add `enroll` subcommand to CLI**

Records 3-5 utterances (3s each), extracts embeddings, computes mean, saves to `~/.config/voicerouter/speaker.bin`.

- [ ] **Step 2: Test manually**

Run: `voicerouter enroll`
Expected: prompts user to speak 5 times, saves enrollment file

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add speaker enrollment subcommand"
```

---

### Task 9: ContinuousActor

**Files:**
- Modify: `src/continuous/mod.rs` — add ContinuousActor
- Modify: `src/main.rs` — wire into daemon

- [ ] **Step 1: Implement ContinuousActor**

```rust
pub struct ContinuousActor {
    config: Config,
    audio_rx: Receiver<AudioChunk>,
}

impl Actor for ContinuousActor {
    fn name(&self) -> &str { "continuous" }
    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        // 1. Init VAD, SpeakerVerifier (if enabled), ASR, IntentFilter, LlmClient
        // 2. Loop: receive audio → VAD → speaker verify → ASR → intent → dispatch
    }
}
```

Orchestration loop:
1. Feed audio to VAD
2. On SpeechSegment → speaker verify (pass/fail)
3. On pass → ASR transcribe
4. IntentFilter classify
5. Command → check risk → low: send PipelineInput, high: send ConfirmAction
6. Uncertain → LlmClient.classify → same risk check
7. Ambient → discard

- [ ] **Step 2: Wire into main.rs daemon startup**

Add AudioSource subscriber for continuous actor. Subscribe to ConfirmAction/ActionConfirmed in Bus.

- [ ] **Step 3: Integration test — run daemon with continuous enabled**

Run: `VOICEROUTER_LLM_KEY=test voicerouter --preload` with `[continuous] enabled = true`
Expected: logs show `[continuous] ready`, VAD processes audio

- [ ] **Step 4: Commit**

```bash
git add src/continuous/mod.rs src/main.rs
git commit -m "feat(continuous): add ContinuousActor with full pipeline"
```

---

### Task 10: High-Risk Confirmation Flow

**Files:**
- Modify: `src/hotkey/mod.rs` — listen for ConfirmAction, accept hotkey as confirmation
- Modify: `src/continuous/mod.rs` — wait for ActionConfirmed/ActionRejected

- [ ] **Step 1: Implement confirmation flow**

When ContinuousActor detects a high-risk command:
1. Send `ConfirmAction { text, stage }` to bus
2. Play confirmation beep (distinct from recording beep)
3. Wait up to 3s for `ActionConfirmed` (hotkey press) or timeout → `ActionRejected`
4. On confirmed → send `PipelineInput`
5. On rejected → discard + log

HotkeyActor: when in Idle state and ConfirmAction is pending, treat next key press as ActionConfirmed.

- [ ] **Step 2: Test manually**

Say a shell command with continuous mode enabled. Verify beep plays and waits for confirmation.

- [ ] **Step 3: Commit**

```bash
git add src/continuous/mod.rs src/hotkey/mod.rs
git commit -m "feat(continuous): add high-risk action confirmation flow"
```

---

### Task 11: Model Download Support

**Files:**
- Modify: `src/main.rs` or model download logic

- [ ] **Step 1: Add silero-vad and speaker model to download command**

`voicerouter download silero-vad` — download Silero VAD ONNX model
`voicerouter download 3dspeaker` — download 3D-Speaker embedding model

- [ ] **Step 2: Test download**

Run: `voicerouter download silero-vad`
Expected: model downloaded to `~/.cache/voicerouter/models/silero-vad/`

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add silero-vad and speaker model download"
```

---

### Task 12: Documentation and Config Update

**Files:**
- Modify: `README.md`, `README_zh.md`
- Modify: `config.default.toml`
- Modify: `docs/plans/TODO.md`

- [ ] **Step 1: Add continuous listening section to READMEs**
- [ ] **Step 2: Add `[continuous]` section to config.default.toml**
- [ ] **Step 3: Update TODO.md**
- [ ] **Step 4: Commit**

```bash
git add README.md README_zh.md config.default.toml docs/
git commit -m "docs: add continuous listening documentation"
```
