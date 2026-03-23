# Voice Interaction Framework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Evolve voicerouter from a voice input tool into a local, offline-first voice interaction framework with actor-based architecture, composable handler pipeline, IPC, TTS, wake word detection, and DAG workflow orchestration.

**Architecture:** Actor model with central bus (`crossbeam::channel`). Each component (hotkey, core audio/ASR, pipeline, IPC, TTS, wakeword) runs as an independent actor on its own thread. A lightweight `Bus` routes typed `Message` enums between actors via topic-based 1:N subscriptions. No async runtime.

**Tech Stack:** Rust 2021, crossbeam (channels + select), serde_json (IPC), sherpa-onnx (ASR + TTS), cpal (audio), ureq (HTTP handler Phase 4)

**Spec:** `docs/superpowers/specs/2026-03-23-voice-interaction-framework-design.md`

---

## File Structure

### New files (Phase 1)

| File | Responsibility |
|------|---------------|
| `src/actor.rs` | `Message` enum, `Actor` trait, `Bus` struct, `Metadata`, `SpeakSource` |
| `src/pipeline/mod.rs` | `PipelineActor` — receives `Transcript`/`PipelineInput`, runs stage chain |
| `src/pipeline/stage.rs` | `Stage`, `Condition`, `StageContext`, `HandlerResult` types |
| `src/pipeline/handler.rs` | `Handler` trait (new signature with `StageContext`) |
| `src/pipeline/handlers/mod.rs` | Handler registry: maps handler name string → `Box<dyn Handler>` |
| `src/pipeline/handlers/inject.rs` | `InjectHandler` — wraps `inject::inject_text()` |
| `src/pipeline/handlers/shell.rs` | `ShellHandler` — migrated from `router/handlers/shell.rs` |
| `src/ipc.rs` | `IpcActor` — Unix socket listener, JSON-RPC protocol, event subscriptions |

### New files (Phase 2)

| File | Responsibility |
|------|---------------|
| `src/tts/mod.rs` | `TtsActor`, `TtsEngine` trait, `TtsConfig` |
| `src/tts/sherpa.rs` | `SherpaTts` — sherpa-onnx VITS implementation |

### New files (Phase 3)

| File | Responsibility |
|------|---------------|
| `src/wakeword/mod.rs` | `WakewordActor` |
| `src/wakeword/detector.rs` | `WakewordDetector` — sliding window + ASR prefix match |

### New files (Phase 4)

| File | Responsibility |
|------|---------------|
| `src/pipeline/dag.rs` | `DagExecutor` — topological sort, parallel execution, error propagation |
| `src/pipeline/handlers/pipe.rs` | `PipeHandler` — stdin/stdout subprocess pipe |
| `src/pipeline/handlers/http.rs` | `HttpHandler` — sync HTTP via ureq |
| `src/pipeline/handlers/transform.rs` | `TransformHandler` — regex replace, template |

### Modified files

| File | Change |
|------|--------|
| `Cargo.toml` | Add `crossbeam`, `serde_json`; later `ureq` (Phase 4) |
| `src/config.rs` | Add `PipelineConfig`, `IpcConfig`, `TtsConfig`, `WakewordConfig`; router→pipeline migration |
| `src/lib.rs` | Add `pub mod actor`, `pub mod pipeline`, `pub mod ipc`, `pub mod tts`, `pub mod wakeword` |
| `src/main.rs` | Rewrite `run_daemon()` to spawn actors + bus; keep CLI/test modes unchanged |
| `src/hotkey/evdev.rs` | Add `HotkeyActor` wrapper that reads inbox for `StopListening`/`Shutdown` |

### Deleted files (after migration)

| File | Reason |
|------|--------|
| `src/router/mod.rs` | Replaced by `src/pipeline/mod.rs` |
| `src/router/handler.rs` | Replaced by `src/pipeline/handler.rs` |
| `src/router/handlers/mod.rs` | Replaced by `src/pipeline/handlers/mod.rs` |
| `src/router/handlers/inject.rs` | Replaced by `src/pipeline/handlers/inject.rs` |
| `src/router/handlers/shell.rs` | Replaced by `src/pipeline/handlers/shell.rs` |

---

## Phase 1: Actor Infrastructure + Pipeline + IPC

### Task 1: Add crossbeam and serde_json dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add dependencies to Cargo.toml**

Add under `[dependencies]`:
```toml
crossbeam = "0.8"
serde_json = "1"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add crossbeam and serde_json dependencies"
```

---

### Task 2: Message enum and Actor trait

**Files:**
- Create: `src/actor.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for Message**

Create `src/actor.rs` with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_clone_preserves_data() {
        let msg = Message::Transcript {
            text: "hello".into(),
            raw: "hello".into(),
        };
        let cloned = msg.clone();
        assert!(matches!(cloned, Message::Transcript { text, .. } if text == "hello"));
    }

    #[test]
    fn message_topic_returns_variant_name() {
        assert_eq!(Message::StartListening.topic(), "StartListening");
        assert_eq!(Message::Shutdown.topic(), "Shutdown");
        let msg = Message::Transcript { text: "x".into(), raw: "x".into() };
        assert_eq!(msg.topic(), "Transcript");
    }

    #[test]
    fn bus_routes_to_subscribers() {
        let (tx, rx) = crossbeam::channel::bounded(8);
        let mut bus = Bus::new();
        bus.subscribe("StartListening", tx);
        bus.publish(Message::StartListening);
        let received = rx.try_recv().unwrap();
        assert!(matches!(received, Message::StartListening));
    }

    #[test]
    fn bus_fan_out_to_multiple_subscribers() {
        let (tx1, rx1) = crossbeam::channel::bounded(8);
        let (tx2, rx2) = crossbeam::channel::bounded(8);
        let mut bus = Bus::new();
        bus.subscribe("Shutdown", tx1);
        bus.subscribe("Shutdown", tx2);
        bus.publish(Message::Shutdown);
        assert!(matches!(rx1.try_recv().unwrap(), Message::Shutdown));
        assert!(matches!(rx2.try_recv().unwrap(), Message::Shutdown));
    }

    #[test]
    fn bus_no_subscriber_is_silent() {
        let bus = Bus::new();
        bus.publish(Message::StartListening); // should not panic
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter actor::tests -- --nocapture`
Expected: FAIL — `Message`, `Bus` not defined yet.

- [ ] **Step 3: Implement Message, Metadata, SpeakSource, Actor trait, Bus**

```rust
//! Actor infrastructure: Message types, Actor trait, and Bus.

use std::collections::HashMap;
use std::time::Instant;

use crossbeam::channel::Sender;

// ---- Message ----

#[derive(Clone, Debug)]
pub enum Message {
    Transcript { text: String, raw: String },
    PipelineInput { text: String, metadata: Metadata },
    PipelineOutput { text: String, stage: String },
    SpeakRequest { text: String, source: SpeakSource },
    SpeakDone,
    MuteInput,
    UnmuteInput,
    StartListening,
    StopListening,
    /// Cancel active recording without transcribing (discard audio).
    /// Used by Auto-mode CancelAndToggle to discard tentative recording.
    CancelRecording,
    Shutdown,
}

impl Message {
    /// Return the topic string for bus routing.
    #[must_use]
    pub fn topic(&self) -> &'static str {
        match self {
            Self::Transcript { .. } => "Transcript",
            Self::PipelineInput { .. } => "PipelineInput",
            Self::PipelineOutput { .. } => "PipelineOutput",
            Self::SpeakRequest { .. } => "SpeakRequest",
            Self::SpeakDone => "SpeakDone",
            Self::MuteInput => "MuteInput",
            Self::UnmuteInput => "UnmuteInput",
            Self::StartListening => "StartListening",
            Self::StopListening => "StopListening",
            Self::CancelRecording => "CancelRecording",
            Self::Shutdown => "Shutdown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub source: String,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub enum SpeakSource {
    LlmReply,
    SystemFeedback,
}

// ---- Actor trait ----

pub trait Actor: Send + 'static {
    fn name(&self) -> &str;
    fn run(self, inbox: crossbeam::channel::Receiver<Message>, outbox: Sender<Message>);
}

// ---- Bus ----

pub struct Bus {
    subscriptions: HashMap<&'static str, Vec<Sender<Message>>>,
}

impl Bus {
    #[must_use]
    pub fn new() -> Self {
        Self { subscriptions: HashMap::new() }
    }

    pub fn subscribe(&mut self, topic: &'static str, sender: Sender<Message>) {
        self.subscriptions.entry(topic).or_default().push(sender);
    }

    pub fn publish(&self, msg: Message) {
        let topic = msg.topic();
        if let Some(subs) = self.subscriptions.get(topic) {
            for sender in subs {
                if let Err(e) = sender.send(msg.clone()) {
                    if matches!(msg, Message::Shutdown) {
                        log::warn!("failed to deliver Shutdown: {e}");
                    }
                }
            }
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod actor;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter actor::tests -- --nocapture`
Expected: All 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/actor.rs src/lib.rs
git commit -m "feat(actor): add Message enum, Actor trait, and Bus"
```

---

### Task 3: Pipeline stage types and Handler trait

**Files:**
- Create: `src/pipeline/handler.rs`
- Create: `src/pipeline/stage.rs`
- Create: `src/pipeline/mod.rs` (minimal, just module declarations)
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for Condition and StageContext**

In `src/pipeline/stage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_starts_with_matches() {
        let cond = Condition::StartsWith("搜索".into());
        assert!(cond.matches_text("搜索什么东西"));
        assert!(!cond.matches_text("其他内容"));
    }

    #[test]
    fn condition_starts_with_strip_prefix() {
        let cond = Condition::StartsWith("搜索".into());
        assert_eq!(cond.strip_prefix("搜索什么东西"), Some("什么东西"));
        assert_eq!(cond.strip_prefix("其他内容"), None);
    }

    #[test]
    fn condition_always_matches_everything() {
        let cond = Condition::Always;
        assert!(cond.matches_text("anything"));
        assert_eq!(cond.strip_prefix("anything"), None);
    }

    #[test]
    fn stage_context_from_params() {
        let mut params = HashMap::new();
        params.insert("command".into(), "echo {text}".into());
        let ctx = StageContext { stage_name: "test".into(), params };
        assert_eq!(ctx.get("command"), Some("echo {text}"));
        assert_eq!(ctx.get("missing"), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter pipeline::stage::tests -- --nocapture`
Expected: FAIL — types not defined.

- [ ] **Step 3: Implement stage types**

`src/pipeline/handler.rs`:

```rust
//! Handler trait for pipeline stages.

use anyhow::Result;

use super::stage::StageContext;
use crate::actor::Message;

/// Result of a handler execution.
#[derive(Debug)]
pub enum HandlerResult {
    /// Pass transformed text to the next stage.
    Forward(String),
    /// Emit a message to the bus. Pipeline continues with same text.
    Emit(Message),
    /// Forward text to next stage AND emit a message to the bus.
    ForwardAndEmit(String, Message),
    /// Stop pipeline execution.
    Done,
}

/// A pipeline stage handler.
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult>;
}
```

`src/pipeline/stage.rs`:

```rust
//! Pipeline stage types: Stage, Condition, StageContext.

use std::collections::HashMap;
use std::time::Duration;

use super::handler::{Handler, HandlerResult};

/// A single pipeline stage.
pub struct Stage {
    pub name: String,
    pub handler: Box<dyn Handler>,
    pub condition: Option<Condition>,
    pub after: Option<String>,        // DAG dependency (Phase 4); None = root stage
    pub params: HashMap<String, String>,
    pub timeout: Duration,
}

impl Stage {
    pub fn to_context(&self) -> StageContext {
        StageContext {
            stage_name: self.name.clone(),
            params: self.params.clone(),
        }
    }
}

/// Condition guard for stage execution.
#[derive(Debug, Clone)]
pub enum Condition {
    Always,
    StartsWith(String),
    OutputEq(String),
    OutputContains(String),
}

impl Condition {
    /// Phase 1: evaluate against current text only.
    #[must_use]
    pub fn matches_text(&self, text: &str) -> bool {
        match self {
            Self::Always => true,
            Self::StartsWith(prefix) => text.starts_with(prefix.as_str()),
            // OutputEq / OutputContains need results map — always false in Phase 1
            Self::OutputEq(_) | Self::OutputContains(_) => false,
        }
    }

    /// Phase 4: evaluate against current text AND upstream stage results.
    #[must_use]
    pub fn matches_with_results(
        &self,
        text: &str,
        results: &HashMap<String, String>,
    ) -> bool {
        match self {
            Self::Always => true,
            Self::StartsWith(prefix) => text.starts_with(prefix.as_str()),
            Self::OutputEq(expected) => {
                // Check if any upstream result equals expected
                results.values().any(|v| v.trim() == expected.as_str())
            }
            Self::OutputContains(substring) => {
                results.values().any(|v| v.contains(substring.as_str()))
            }
        }
    }

    /// Strip the prefix matched by StartsWith; returns remaining text trimmed.
    #[must_use]
    pub fn strip_prefix<'a>(&self, text: &'a str) -> Option<&'a str> {
        match self {
            Self::StartsWith(prefix) => {
                text.strip_prefix(prefix.as_str()).map(|s| s.trim_start())
            }
            _ => None,
        }
    }
}

/// Read-only context passed to handler at execution time.
#[derive(Debug, Clone)]
pub struct StageContext {
    pub stage_name: String,
    pub params: HashMap<String, String>,
}

impl StageContext {
    /// Get a parameter value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}
```

`src/pipeline/mod.rs` (minimal for now):

```rust
//! Composable handler pipeline.

pub mod handler;
pub mod stage;
pub mod handlers;
```

Add to `src/lib.rs`:
```rust
pub mod pipeline;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter pipeline::stage::tests -- --nocapture`
Expected: All 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/ src/lib.rs
git commit -m "feat(pipeline): add Handler trait, Stage, Condition, StageContext types"
```

---

### Task 4: Migrate inject and shell handlers to pipeline

**Files:**
- Create: `src/pipeline/handlers/mod.rs`
- Create: `src/pipeline/handlers/inject.rs`
- Create: `src/pipeline/handlers/shell.rs`

- [ ] **Step 1: Write tests for new handlers**

In `src/pipeline/handlers/inject.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    #[test]
    fn inject_handler_name() {
        let handler = InjectHandler::new(InjectMethod::Auto);
        assert_eq!(handler.name(), "inject");
    }

    // Note: actual injection requires a display server; only test the trait contract.
}
```

In `src/pipeline/handlers/shell.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::handler::HandlerResult;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx_with_command(cmd: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert("command".into(), cmd.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn shell_handler_name() {
        let handler = ShellHandler;
        assert_eq!(handler.name(), "shell");
    }

    #[test]
    fn shell_executes_command_template() {
        let handler = ShellHandler;
        let ctx = ctx_with_command("echo {text}");
        let result = handler.handle("hello", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_executes_raw_text_without_template() {
        let handler = ShellHandler;
        let ctx = StageContext {
            stage_name: "test".into(),
            params: HashMap::new(),
        };
        let result = handler.handle("echo raw", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_url_encodes_text_in_template() {
        let handler = ShellHandler;
        let ctx = ctx_with_command("echo '{text}'");
        // Should not fail even with special chars — they get URL-encoded.
        let result = handler.handle("hello world", &ctx).unwrap();
        assert!(matches!(result, HandlerResult::Done));
    }

    #[test]
    fn shell_empty_text_with_no_template_errors() {
        let handler = ShellHandler;
        let ctx = StageContext {
            stage_name: "test".into(),
            params: HashMap::new(),
        };
        let result = handler.handle("", &ctx);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter pipeline::handlers -- --nocapture`
Expected: FAIL — handler structs not defined.

- [ ] **Step 3: Implement handlers**

`src/pipeline/handlers/mod.rs`:

```rust
//! Concrete handler implementations for the pipeline.

pub mod inject;
pub mod shell;

use crate::config::Config;
use super::handler::Handler;
use inject::InjectHandler;
use shell::ShellHandler;

/// Build a handler by name from config.
pub fn build_handler(name: &str, config: &Config) -> Box<dyn Handler> {
    match name {
        "inject" => Box::new(InjectHandler::new(config.inject.method)),
        "shell" => Box::new(ShellHandler),
        other => {
            log::warn!("[pipeline] unknown handler {other:?}, falling back to inject");
            Box::new(InjectHandler::new(config.inject.method))
        }
    }
}
```

`src/pipeline/handlers/inject.rs`:

```rust
//! Inject handler — forwards text to the focused window.

use anyhow::Result;

use crate::config::InjectMethod;
use crate::inject::inject_text;
use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct InjectHandler {
    method: InjectMethod,
}

impl InjectHandler {
    #[must_use]
    pub fn new(method: InjectMethod) -> Self {
        Self { method }
    }
}

impl Handler for InjectHandler {
    fn name(&self) -> &str {
        "inject"
    }

    fn handle(&self, text: &str, _ctx: &StageContext) -> Result<HandlerResult> {
        inject_text(text, self.method)?;
        Ok(HandlerResult::Done)
    }
}
```

`src/pipeline/handlers/shell.rs`:

```rust
//! Shell handler — execute text as a shell command or apply command template.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct ShellHandler;

impl Handler for ShellHandler {
    fn name(&self) -> &str {
        "shell"
    }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let cmd = match ctx.get("command") {
            Some(tpl) => {
                let encoded = url_encode(text);
                tpl.replace("{text}", &encoded)
            }
            None => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    bail!("shell handler received empty command");
                }
                trimmed.to_string()
            }
        };

        log::info!("[shell] executing: {:?}", cmd);

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .context("failed to spawn shell process")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            log::info!("[shell] stdout: {}", stdout.trim_end());
        }
        if !stderr.is_empty() {
            log::warn!("[shell] stderr: {}", stderr.trim_end());
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            bail!("shell command exited with status {code}: {cmd:?}");
        }

        Ok(HandlerResult::Done)
    }
}

/// Percent-encode for safe use in shell command templates.
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push('%');
                result.push_str(&format!("{b:02X}"));
            }
        }
    }
    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter pipeline::handlers -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/handlers/
git commit -m "feat(pipeline): migrate inject and shell handlers with new Handler trait"
```

---

### Task 5: Config — add pipeline, IPC, TTS, wakeword sections + router migration

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write tests for new config sections and migration**

Add to `src/config.rs` tests module:

```rust
#[test]
fn pipeline_config_defaults() {
    let config = Config::default();
    assert!(config.pipeline.stages.is_empty());
    assert_eq!(config.pipeline.error_policy, ErrorPolicy::FailFast);
}

#[test]
fn ipc_config_defaults() {
    let config = Config::default();
    assert!(config.ipc.enabled);
    assert_eq!(config.ipc.max_connections, 8);
}

#[test]
fn pipeline_stage_deserializes() {
    let toml = r#"
[[pipeline.stages]]
name = "default"
handler = "inject"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert_eq!(config.pipeline.stages.len(), 1);
    assert_eq!(config.pipeline.stages[0].name, "default");
}

#[test]
fn pipeline_stage_with_condition() {
    let toml = r#"
[[pipeline.stages]]
name = "search"
handler = "shell"
command = "firefox {text}"
condition = "starts_with:搜索"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    assert_eq!(config.pipeline.stages[0].condition.as_deref(), Some("starts_with:搜索"));
}

#[test]
fn router_rules_migrate_to_pipeline() {
    let toml = r#"
[[router.rules]]
trigger = "搜索"
handler = "shell"
command = "firefox https://google.com/search?q={text}"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    let stages = config.effective_pipeline_stages();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].name, "router_rule_0");
    assert_eq!(stages[0].handler, "shell");
    assert_eq!(stages[0].condition.as_deref(), Some("starts_with:搜索"));
}

#[test]
fn pipeline_stages_take_precedence_over_router() {
    let toml = r#"
[[router.rules]]
trigger = "old"
handler = "shell"

[[pipeline.stages]]
name = "new"
handler = "inject"
"#;
    let config: Config = toml::from_str(toml).expect("parse failed");
    let stages = config.effective_pipeline_stages();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].name, "new");
}

#[test]
fn tts_config_defaults() {
    let config = Config::default();
    assert!(!config.tts.enabled);
    assert_eq!(config.tts.engine, "sherpa-onnx");
}

#[test]
fn wakeword_config_defaults() {
    let config = Config::default();
    assert!(!config.wakeword.enabled);
    assert!(config.wakeword.phrases.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter config::tests -- --nocapture`
Expected: FAIL — new config types not defined.

- [ ] **Step 3: Implement config additions**

Add to `src/config.rs` — new enums and structs:

```rust
/// Pipeline error handling policy.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorPolicy {
    #[default]
    FailFast,
    BestEffort,
}

/// Wakeword action on detection.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WakewordAction {
    #[default]
    StartRecording,
    PipelinePassthrough,
}

/// A single pipeline stage definition (config-level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    pub name: String,
    pub handler: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default = "default_stage_timeout")]
    pub timeout: u64,
}

fn default_stage_timeout() -> u64 { 10 }

/// Pipeline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    pub stages: Vec<StageConfig>,
    pub error_policy: ErrorPolicy,
    pub max_parallel_stages: usize,
    pub max_concurrent_executions: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            error_policy: ErrorPolicy::default(),
            max_parallel_stages: 4,
            max_concurrent_executions: 2,
        }
    }
}

/// IPC configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IpcConfig {
    pub enabled: bool,
    pub socket_path: String,
    pub max_connections: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            socket_path: String::new(),
            max_connections: 8,
        }
    }
}

/// TTS configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub enabled: bool,
    pub engine: String,
    pub model: String,
    pub model_dir: String,
    pub speed: f64,
    pub mute_mic_during_playback: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: "sherpa-onnx".to_owned(),
            model: "vits-zh".to_owned(),
            model_dir: "~/.cache/voicerouter/models".to_owned(),
            speed: 1.0,
            mute_mic_during_playback: true,
        }
    }
}

/// Wakeword configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WakewordConfig {
    pub enabled: bool,
    pub phrases: Vec<String>,
    pub window_seconds: f64,
    pub stride_seconds: f64,
    pub action: WakewordAction,
    pub model: String,
}

impl Default for WakewordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            phrases: Vec::new(),
            window_seconds: 2.0,
            stride_seconds: 1.0,
            action: WakewordAction::default(),
            model: String::new(),
        }
    }
}
```

Add fields to `Config`:
```rust
pub struct Config {
    // ... existing fields ...
    pub pipeline: PipelineConfig,
    pub ipc: IpcConfig,
    pub tts: TtsConfig,
    pub wakeword: WakewordConfig,
}
```

Add migration method:
```rust
impl Config {
    /// Return effective pipeline stages: pipeline.stages if present,
    /// else convert router.rules with deprecation warning.
    pub fn effective_pipeline_stages(&self) -> Vec<StageConfig> {
        if !self.pipeline.stages.is_empty() {
            if !self.router.rules.is_empty() {
                log::warn!(
                    "[config] both [router] and [pipeline] defined; \
                     [router] is deprecated and will be ignored"
                );
            }
            return self.pipeline.stages.clone();
        }

        self.router
            .rules
            .iter()
            .enumerate()
            .map(|(i, rule)| StageConfig {
                name: format!("router_rule_{i}"),
                handler: rule.handler.clone(),
                command: rule.command.clone(),
                condition: Some(format!("starts_with:{}", rule.trigger)),
                after: None,
                url: None,
                method: None,
                body: None,
                timeout: default_stage_timeout(),
            })
            .collect()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter config::tests -- --nocapture`
Expected: All tests PASS (existing + new).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add pipeline, ipc, tts, wakeword config sections + router migration"
```

---

### Task 6: PipelineActor — linear chain execution

**Files:**
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Write tests for pipeline execution**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::Message;
    use crate::pipeline::handler::{Handler, HandlerResult};
    use crate::pipeline::stage::{Condition, Stage, StageContext};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Test handler that records received text.
    struct RecordingHandler {
        received: Arc<Mutex<Vec<String>>>,
    }

    impl Handler for RecordingHandler {
        fn name(&self) -> &str { "recording" }
        fn handle(&self, text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
            self.received.lock().unwrap().push(text.to_string());
            Ok(HandlerResult::Forward(text.to_string()))
        }
    }

    /// Test handler that transforms text.
    struct UpperHandler;

    impl Handler for UpperHandler {
        fn name(&self) -> &str { "upper" }
        fn handle(&self, text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
            Ok(HandlerResult::Forward(text.to_uppercase()))
        }
    }

    fn make_stage(name: &str, handler: Box<dyn Handler>, cond: Option<Condition>) -> Stage {
        Stage {
            name: name.into(),
            handler,
            condition: cond,
            after: None,
            params: HashMap::new(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn pipeline_single_stage() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![make_stage("s1", Box::new(RecordingHandler {
            received: Arc::clone(&received),
        }), None)];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["hello"]);
    }

    #[test]
    fn pipeline_chain_transforms_text() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("upper", Box::new(UpperHandler), None),
            make_stage("record", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), None),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["HELLO"]);
    }

    #[test]
    fn pipeline_condition_skips_non_matching() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("conditional", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), Some(Condition::StartsWith("搜索".into()))),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "其他内容", &tx);
        assert!(received.lock().unwrap().is_empty());
    }

    #[test]
    fn pipeline_condition_strips_prefix() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let stages = vec![
            make_stage("conditional", Box::new(RecordingHandler {
                received: Arc::clone(&received),
            }), Some(Condition::StartsWith("搜索".into()))),
        ];
        let (tx, _rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "搜索什么东西", &tx);
        assert_eq!(*received.lock().unwrap(), vec!["什么东西"]);
    }

    #[test]
    fn pipeline_emit_sends_to_outbox() {
        struct EmitHandler;
        impl Handler for EmitHandler {
            fn name(&self) -> &str { "emit" }
            fn handle(&self, _text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
                Ok(HandlerResult::Emit(Message::SpeakRequest {
                    text: "spoken".into(),
                    source: crate::actor::SpeakSource::SystemFeedback,
                }))
            }
        }
        let stages = vec![make_stage("emit", Box::new(EmitHandler), None)];
        let (tx, rx) = crossbeam::channel::bounded(8);
        execute_pipeline(&stages, "hello", &tx);
        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, Message::SpeakRequest { text, .. } if text == "spoken"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter pipeline::tests -- --nocapture`
Expected: FAIL — `execute_pipeline` not defined.

- [ ] **Step 3: Implement execute_pipeline**

Add to `src/pipeline/mod.rs`:

```rust
//! Composable handler pipeline.

pub mod handler;
pub mod handlers;
pub mod stage;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use stage::Stage;

// ---- PipelineActor ----

/// Actor that receives transcripts and runs the stage pipeline.
pub struct PipelineActor {
    stages: Vec<Stage>,
}

impl PipelineActor {
    #[must_use]
    pub fn new(stages: Vec<Stage>) -> Self {
        Self { stages }
    }
}

impl Actor for PipelineActor {
    fn name(&self) -> &str {
        "pipeline"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        log::info!("[pipeline] ready with {} stages", self.stages.len());

        for msg in inbox {
            match msg {
                Message::Shutdown => break,
                Message::Transcript { ref text, .. } => {
                    execute_pipeline(&self.stages, text, &outbox);
                }
                Message::PipelineInput { ref text, .. } => {
                    execute_pipeline(&self.stages, text, &outbox);
                }
                _ => {}
            }
        }

        log::info!("[pipeline] stopped");
    }
}

// ---- Pipeline execution ----

/// Execute a linear pipeline of stages on the given text.
///
/// Stages run in order. Each stage may transform the text (Forward),
/// emit a bus message (Emit), do both (ForwardAndEmit), or terminate (Done).
pub fn execute_pipeline(
    stages: &[Stage],
    text: &str,
    outbox: &Sender<Message>,
) {
    let mut current_text = text.to_string();

    for stage in stages {
        if let Some(ref cond) = stage.condition {
            if !cond.matches_text(&current_text) {
                continue;
            }
        }

        let payload = stage.condition.as_ref()
            .and_then(|c| c.strip_prefix(&current_text))
            .unwrap_or(&current_text);

        let ctx = stage.to_context();
        match stage.handler.handle(payload, &ctx) {
            Ok(handler::HandlerResult::Forward(text)) => current_text = text,
            Ok(handler::HandlerResult::Emit(msg)) => {
                outbox.send(msg).ok();
            }
            Ok(handler::HandlerResult::ForwardAndEmit(text, msg)) => {
                current_text = text;
                outbox.send(msg).ok();
            }
            Ok(handler::HandlerResult::Done) => break,
            Err(e) => {
                log::error!("[pipeline] stage '{}' failed: {e:#}", stage.name);
                break;
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter pipeline::tests -- --nocapture`
Expected: All 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "feat(pipeline): implement linear chain execution with condition matching"
```

---

### Task 7: IpcActor — Unix socket + JSON-RPC

**Files:**
- Create: `src/ipc.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for JSON-RPC parsing**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pipeline_send() {
        let json = r#"{"method":"pipeline.send","params":{"text":"hello"}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "pipeline.send");
        assert_eq!(req.params["text"].as_str().unwrap(), "hello");
    }

    #[test]
    fn parse_recording_start() {
        let json = r#"{"method":"recording.start"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "recording.start");
    }

    #[test]
    fn parse_events_subscribe() {
        let json = r#"{"method":"events.subscribe","params":{"types":["transcript"]}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        let types = req.params["types"].as_array().unwrap();
        assert_eq!(types[0].as_str().unwrap(), "transcript");
    }

    #[test]
    fn format_error_response() {
        let resp = JsonRpcResponse::error(-32700, "Parse error");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32700"));
        assert!(json.contains("Parse error"));
    }

    #[test]
    fn format_event_notification() {
        let event = json_event("transcript", "你好世界", Some("你好世界"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("transcript"));
        assert!(json.contains("你好世界"));
    }

    #[test]
    fn default_socket_path_uses_xdg() {
        let path = resolve_socket_path("");
        assert!(path.ends_with("voicerouter.sock"));
    }

    #[test]
    fn custom_socket_path_is_used() {
        let path = resolve_socket_path("/tmp/custom.sock");
        assert_eq!(path, "/tmp/custom.sock");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter ipc::tests -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement IPC module**

`src/ipc.rs`:

```rust
//! IPC actor — Unix socket server with JSON-RPC 2.0 protocol.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::actor::{Actor, Message, Metadata};
use crate::config::IpcConfig;

// ---- JSON-RPC types ----

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn ok(result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", result: Some(result), error: None }
    }

    pub fn error(code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError { code, message: message.into() }),
        }
    }
}

/// Format a push event notification.
pub fn json_event(
    event_type: &str,
    text: &str,
    raw: Option<&str>,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();
    params.insert("type".into(), event_type.into());
    params.insert("text".into(), text.into());
    if let Some(r) = raw {
        params.insert("raw".into(), r.into());
    }
    serde_json::json!({ "method": "event", "params": params })
}

/// Resolve socket path: use custom if non-empty, else XDG_RUNTIME_DIR.
pub fn resolve_socket_path(configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_string();
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{runtime_dir}/voicerouter.sock")
    } else {
        "/tmp/voicerouter.sock".to_string()
    }
}

// ---- IpcActor ----

pub struct IpcActor {
    config: IpcConfig,
}

impl IpcActor {
    #[must_use]
    pub fn new(config: IpcConfig) -> Self {
        Self { config }
    }
}

impl Actor for IpcActor {
    fn name(&self) -> &str {
        "ipc"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let socket_path = resolve_socket_path(&self.config.socket_path);

        // Remove stale socket file.
        let _ = std::fs::remove_file(&socket_path);

        let listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("[ipc] failed to bind {socket_path}: {e}");
                return;
            }
        };

        // Set socket permissions to 0600.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &socket_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        listener.set_nonblocking(true).ok();
        log::info!("[ipc] listening on {socket_path}");

        let clients: Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let max_conn = self.config.max_connections;

        loop {
            // Check for shutdown or bus events.
            if let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::Shutdown => {
                        log::info!("[ipc] shutting down");
                        let _ = std::fs::remove_file(&socket_path);
                        break;
                    }
                    Message::Transcript { ref text, ref raw } => {
                        let event = json_event("transcript", text, Some(raw));
                        push_to_clients(&clients, &event);
                    }
                    Message::PipelineOutput { ref text, ref stage } => {
                        let event = json_event("pipeline_output", text, Some(stage));
                        push_to_clients(&clients, &event);
                    }
                    _ => {}
                }
            }

            // Accept new connections.
            if let Ok((stream, _addr)) = listener.accept() {
                let mut client_list = clients.lock().unwrap();
                if client_list.len() >= max_conn {
                    log::warn!("[ipc] max connections reached, rejecting");
                    let mut s = stream;
                    let resp = JsonRpcResponse::error(-32000, "max connections reached");
                    let _ = writeln!(s, "{}", serde_json::to_string(&resp).unwrap());
                    drop(s);
                } else {
                    let client = Arc::new(Mutex::new(stream));
                    client_list.push(Arc::clone(&client));
                    let outbox_clone = outbox.clone();
                    std::thread::spawn(move || {
                        handle_client(client, outbox_clone);
                    });
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

fn handle_client(stream: Arc<Mutex<UnixStream>>, outbox: Sender<Message>) {
    let reader_stream = stream.lock().unwrap().try_clone().unwrap();
    let reader = BufReader::new(reader_stream);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.len() > 65536 {
            let resp = JsonRpcResponse::error(-32000, "message too large");
            if let Ok(mut s) = stream.lock() {
                let _ = writeln!(s, "{}", serde_json::to_string(&resp).unwrap());
            }
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                let resp = JsonRpcResponse::error(-32700, "Parse error");
                if let Ok(mut s) = stream.lock() {
                    let _ = writeln!(s, "{}", serde_json::to_string(&resp).unwrap());
                }
                continue;
            }
        };

        let resp = handle_request(&req, &outbox);
        if let Ok(mut s) = stream.lock() {
            let _ = writeln!(s, "{}", serde_json::to_string(&resp).unwrap());
        }
    }
}

fn handle_request(req: &JsonRpcRequest, outbox: &Sender<Message>) -> JsonRpcResponse {
    match req.method.as_str() {
        "pipeline.send" => {
            let text = req.params.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return JsonRpcResponse::error(-32602, "missing 'text' parameter");
            }
            let msg = Message::PipelineInput {
                text: text.to_string(),
                metadata: Metadata {
                    source: "ipc".to_string(),
                    timestamp: Instant::now(),
                },
            };
            outbox.send(msg).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "recording.start" => {
            outbox.send(Message::StartListening).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "recording.stop" => {
            outbox.send(Message::StopListening).ok();
            JsonRpcResponse::ok(serde_json::json!({"status": "ok"}))
        }
        "status" => {
            JsonRpcResponse::ok(serde_json::json!({"status": "running"}))
        }
        "events.subscribe" => {
            // Subscription is implicit — all connected clients receive events.
            JsonRpcResponse::ok(serde_json::json!({"status": "subscribed"}))
        }
        _ => {
            JsonRpcResponse::error(-32601, &format!("unknown method: {}", req.method))
        }
    }
}

fn push_to_clients(
    clients: &Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>>,
    event: &serde_json::Value,
) {
    let json = serde_json::to_string(event).unwrap();
    let mut client_list = clients.lock().unwrap();
    client_list.retain(|client| {
        if let Ok(mut s) = client.lock() {
            writeln!(s, "{json}").is_ok()
        } else {
            false
        }
    });
}
```

Add to `src/lib.rs`:
```rust
pub mod ipc;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter ipc::tests -- --nocapture`
Expected: All 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ipc.rs src/lib.rs
git commit -m "feat(ipc): add IpcActor with Unix socket and JSON-RPC protocol"
```

---

### Task 8: HotkeyActor wrapper

**Files:**
- Modify: `src/hotkey/evdev.rs`
- Modify: `src/hotkey/mod.rs`

- [ ] **Step 1: Read current evdev.rs to understand HotkeyMonitor**

Read `src/hotkey/evdev.rs` to understand the `poll()` method and how to wrap it.

- [ ] **Step 2: Add HotkeyActor to `src/hotkey/mod.rs`**

```rust
use crate::actor::{Actor, Message};
use crossbeam::channel::{Receiver, Sender};

/// Actor wrapper around HotkeyMonitor.
pub struct HotkeyActor {
    config: crate::config::HotkeyConfig,
}

impl HotkeyActor {
    #[must_use]
    pub fn new(config: crate::config::HotkeyConfig) -> Self {
        Self { config }
    }
}

impl Actor for HotkeyActor {
    fn name(&self) -> &str {
        "hotkey"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let mut monitor = match HotkeyMonitor::new(&self.config) {
            Ok(m) => m,
            Err(e) => {
                log::error!("[hotkey] failed to init: {e:#}");
                return;
            }
        };

        log::info!("[hotkey] listening for '{}'", self.config.key);

        loop {
            // Check for StopListening (forced stop from CoreActor) or Shutdown.
            if let Ok(msg) = inbox.try_recv() {
                match msg {
                    Message::StopListening => {
                        log::debug!("[hotkey] received StopListening, resetting state");
                        monitor.reset_state();
                    }
                    Message::Shutdown => {
                        log::info!("[hotkey] shutting down");
                        break;
                    }
                    _ => {}
                }
            }

            if let Some(event) = monitor.poll() {
                match event {
                    HotkeyEvent::StartRecording => {
                        outbox.send(Message::StartListening).ok();
                    }
                    HotkeyEvent::StopRecording => {
                        outbox.send(Message::StopListening).ok();
                    }
                    HotkeyEvent::CancelAndToggle => {
                        // Discard tentative recording (no transcription), then restart.
                        outbox.send(Message::CancelRecording).ok();
                        outbox.send(Message::StartListening).ok();
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add src/hotkey/mod.rs
git commit -m "feat(hotkey): add HotkeyActor wrapper for actor-based event loop"
```

---

### Task 9: CoreActor — audio + ASR + postprocess

**Files:**
- Create: `src/core_actor.rs` (separate from audio/ to avoid bloating audio module)
- Modify: `src/lib.rs`

- [ ] **Step 1: Implement CoreActor**

`src/core_actor.rs`:

```rust
//! CoreActor — owns audio pipeline, ASR engine, noise tracker, and postprocessing.

use std::time::{Duration, Instant};

use anyhow::Context;
use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::asr::AsrEngine;
use crate::audio::{self, AudioPipeline, NoiseTracker};
use crate::config::Config;
use crate::postprocess::postprocess;
use crate::sound;

const MIN_RECORDING_SECS: f32 = 0.3;

enum CoreState {
    Idle,
    Recording,
    Muted,
}

pub struct CoreActor {
    config: Config,
    preload: bool,
}

impl CoreActor {
    #[must_use]
    pub fn new(config: Config, preload: bool) -> Self {
        Self { config, preload }
    }
}

impl Actor for CoreActor {
    fn name(&self) -> &str {
        "core"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let mut audio = match AudioPipeline::new(&self.config.audio) {
            Ok(a) => a,
            Err(e) => {
                log::error!("[core] failed to open audio: {e:#}");
                return;
            }
        };

        let initial_floor = audio::calibrate_silence(
            &mut audio,
            self.config.audio.sample_rate,
            self.config.audio.silence_threshold as f32,
        );
        let mut noise_tracker = NoiseTracker::new(
            initial_floor,
            self.config.audio.silence_threshold as f32,
            self.config.audio.sample_rate,
        );

        let mut asr_engine: Option<AsrEngine> = if self.preload {
            log::info!("[core] preloading ASR model '{}'", self.config.asr.model);
            match AsrEngine::new(&self.config.asr) {
                Ok(e) => Some(e),
                Err(e) => {
                    log::error!("[core] preload failed: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        let mut punctuator: Option<sherpa_onnx::OfflinePunctuation> = None;
        let mut recording_start: Option<Instant> = None;
        let mut state = CoreState::Idle;
        let max_record = Duration::from_secs(u64::from(self.config.audio.max_record_seconds));

        log::info!("[core] ready");

        loop {
            match state {
                CoreState::Idle => {
                    match inbox.recv() {
                        Ok(Message::StartListening) => {
                            log::info!("[core] recording started");
                            beep_if(&self.config, sound::beep_start);
                            if let Err(e) = audio.start_recording() {
                                log::error!("[core] start_recording failed: {e:#}");
                                beep_if(&self.config, sound::beep_error);
                                continue;
                            }
                            recording_start = Some(Instant::now());
                            state = CoreState::Recording;
                        }
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
                CoreState::Recording => {
                    crossbeam::select! {
                        recv(inbox) -> msg => match msg {
                            Ok(Message::StopListening) => {
                                finalize_recording(
                                    &mut audio, &mut asr_engine, &mut punctuator,
                                    &self.config, &outbox, &mut recording_start,
                                    &mut noise_tracker,
                                );
                                state = CoreState::Idle;
                            }
                            Ok(Message::CancelRecording) => {
                                // Discard audio without transcribing.
                                audio.stop_recording();
                                recording_start = None;
                                log::info!("[core] recording cancelled, audio discarded");
                                state = CoreState::Idle;
                            }
                            Ok(Message::MuteInput) => {
                                audio.stop_recording();
                                recording_start = None;
                                state = CoreState::Muted;
                            }
                            Ok(Message::Shutdown) => break,
                            _ => {}
                        },
                        default(Duration::from_millis(10)) => {
                            if let Some(start) = recording_start {
                                if start.elapsed() >= max_record {
                                    log::warn!(
                                        "[core] recording exceeded {}s limit, force-stopping",
                                        self.config.audio.max_record_seconds
                                    );
                                    finalize_recording(
                                        &mut audio, &mut asr_engine, &mut punctuator,
                                        &self.config, &outbox, &mut recording_start,
                                        &mut noise_tracker,
                                    );
                                    // Notify hotkey actor to reset state.
                                    outbox.send(Message::StopListening).ok();
                                    state = CoreState::Idle;
                                }
                            }
                        }
                    }
                }
                CoreState::Muted => {
                    match inbox.recv() {
                        Ok(Message::UnmuteInput) => {
                            state = CoreState::Idle;
                        }
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
            }
        }

        log::info!("[core] stopped");
    }
}

fn finalize_recording(
    audio: &mut AudioPipeline,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
    outbox: &Sender<Message>,
    recording_start: &mut Option<Instant>,
    noise_tracker: &mut NoiseTracker,
) {
    let elapsed = recording_start
        .take()
        .map_or(0.0, |t| t.elapsed().as_secs_f32());
    log::info!("[core] recording stopped ({elapsed:.1}s)");
    beep_if(config, sound::beep_done);

    let samples = match audio.stop_recording() {
        Some(s) => s,
        None => return,
    };

    if elapsed < MIN_RECORDING_SECS {
        log::info!("[core] too short ({elapsed:.1}s), discarding");
        return;
    }

    let threshold = noise_tracker.threshold();
    let peak = audio::peak_rms(&samples, config.audio.sample_rate);
    if peak < threshold {
        log::info!("[core] silence (peak {peak:.4} < {threshold:.4}), discarding");
        return;
    }

    // Lazy-init ASR engine.
    if asr_engine.is_none() {
        log::info!("[core] initialising ASR engine (lazy)");
        match AsrEngine::new(&config.asr) {
            Ok(e) => *asr_engine = Some(e),
            Err(e) => {
                log::error!("[core] ASR init failed: {e:#}");
                beep_if(config, sound::beep_error);
                return;
            }
        }
    }

    let raw = match asr_engine.as_mut().unwrap().transcribe(&samples, config.audio.sample_rate) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            log::info!("[core] transcribed: (empty)");
            return;
        }
        Err(e) => {
            log::error!("[core] transcription failed: {e:#}");
            beep_if(config, sound::beep_error);
            return;
        }
    };

    // Punctuation restoration.
    let with_punct = if config.postprocess.restore_punctuation {
        add_punctuation(&raw, punctuator, config)
    } else {
        raw.clone()
    };

    let text = postprocess(&with_punct, &config.postprocess);
    log::info!("[core] transcribed: {text:?}");

    outbox.send(Message::Transcript { text, raw }).ok();
}

fn add_punctuation(
    text: &str,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
) -> String {
    if punctuator.is_none() {
        let model_path = match crate::asr::models::expand_tilde(&config.postprocess.punctuation_model) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[core] punctuation model path error: {e}");
                return text.to_owned();
            }
        };
        let model_file = model_path.join("model.int8.onnx");
        if !model_file.exists() {
            log::warn!("[core] punctuation model not found at {}", model_file.display());
            return text.to_owned();
        }
        log::info!("[core] loading punctuation model from {}", model_file.display());
        let cfg = sherpa_onnx::OfflinePunctuationConfig {
            model: sherpa_onnx::OfflinePunctuationModelConfig {
                ct_transformer: Some(model_file.to_string_lossy().into_owned()),
                ..Default::default()
            },
        };
        *punctuator = sherpa_onnx::OfflinePunctuation::create(&cfg);
    }

    match punctuator.as_ref() {
        Some(punc) => punc.add_punctuation(text).unwrap_or_else(|| text.to_owned()),
        None => text.to_owned(),
    }
}

fn beep_if(config: &Config, f: fn() -> anyhow::Result<()>) {
    if config.sound.feedback {
        if let Err(e) = f() {
            log::debug!("[core] beep failed: {e}");
        }
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod core_actor;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add src/core_actor.rs src/lib.rs
git commit -m "feat(core): add CoreActor with audio/ASR/postprocess lifecycle"
```

---

### Task 10: Rewrite run_daemon() to use actors

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rewrite run_daemon()**

Replace the `run_daemon` function and its helper functions (`handle_event`, `on_start_recording`, `on_stop_recording`, `validate_recording`, `transcribe_and_process`, `transcribe`, `add_punctuation`, `init_punctuator`, `beep_if`) with the actor-based version:

```rust
fn run_daemon(config: Config, preload: bool) -> Result<()> {
    log::info!("voicerouter starting up (actor mode)");

    use voicerouter::actor::{Actor, Bus, Message};
    use voicerouter::core_actor::CoreActor;
    use voicerouter::hotkey::HotkeyActor;
    use voicerouter::ipc::IpcActor;
    use voicerouter::pipeline::{self, execute_pipeline, handler::Handler, stage::Stage};

    // Build pipeline stages from config.
    let stage_configs = config.effective_pipeline_stages();
    let stages: Vec<Stage> = stage_configs.iter().map(|sc| {
        let handler = pipeline::handlers::build_handler(&sc.handler, &config);
        let condition = sc.condition.as_ref().map(|c| parse_condition(c));
        let mut params = std::collections::HashMap::new();
        if let Some(ref cmd) = sc.command {
            params.insert("command".into(), cmd.clone());
        }
        if let Some(ref url) = sc.url {
            params.insert("url".into(), url.clone());
        }
        if let Some(ref method) = sc.method {
            params.insert("method".into(), method.clone());
        }
        if let Some(ref body) = sc.body {
            params.insert("body".into(), body.clone());
        }
        Stage {
            name: sc.name.clone(),
            handler,
            condition,
            after: sc.after.clone(),
            params,
            timeout: std::time::Duration::from_secs(sc.timeout),
        }
    }).collect();

    // Create channels for each actor.
    let (hotkey_tx, hotkey_rx) = crossbeam::channel::bounded::<Message>(32);
    let (core_tx, core_rx) = crossbeam::channel::bounded::<Message>(32);
    let (pipeline_tx, pipeline_rx) = crossbeam::channel::bounded::<Message>(32);
    let (ipc_tx, ipc_rx) = crossbeam::channel::bounded::<Message>(32);
    let (bus_tx, bus_rx) = crossbeam::channel::bounded::<Message>(128);

    // Set up bus subscriptions.
    let mut bus = Bus::new();
    bus.subscribe("StartListening", core_tx.clone());
    bus.subscribe("StopListening", core_tx.clone());
    bus.subscribe("StopListening", hotkey_tx.clone());
    bus.subscribe("CancelRecording", core_tx.clone());
    bus.subscribe("MuteInput", core_tx.clone());
    bus.subscribe("UnmuteInput", core_tx.clone());
    bus.subscribe("SpeakDone", core_tx.clone());
    bus.subscribe("Transcript", pipeline_tx.clone());
    bus.subscribe("Transcript", ipc_tx.clone());
    bus.subscribe("PipelineInput", pipeline_tx.clone());
    bus.subscribe("PipelineOutput", ipc_tx.clone());
    bus.subscribe("Shutdown", hotkey_tx.clone());
    bus.subscribe("Shutdown", core_tx.clone());
    bus.subscribe("Shutdown", pipeline_tx.clone());
    bus.subscribe("Shutdown", ipc_tx.clone());

    // Spawn bus router thread.
    let bus_handle = std::thread::Builder::new()
        .name("bus".into())
        .spawn(move || {
            for msg in bus_rx {
                if matches!(msg, Message::Shutdown) {
                    bus.publish(msg);
                    break;
                }
                bus.publish(msg);
            }
        })?;

    // Spawn actors.
    let hotkey_actor = HotkeyActor::new(config.hotkey.clone());
    let core_actor = CoreActor::new(config.clone(), preload);
    let ipc_actor = IpcActor::new(config.ipc.clone());

    let bus_tx_hotkey = bus_tx.clone();
    let bus_tx_core = bus_tx.clone();
    let bus_tx_pipeline = bus_tx.clone();
    let bus_tx_ipc = bus_tx.clone();

    let hotkey_handle = std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || hotkey_actor.run(hotkey_rx, bus_tx_hotkey))?;

    let core_handle = std::thread::Builder::new()
        .name("core".into())
        .spawn(move || core_actor.run(core_rx, bus_tx_core))?;

    let pipeline_actor = voicerouter::pipeline::PipelineActor::new(stages);
    let pipeline_handle = std::thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || pipeline_actor.run(pipeline_rx, bus_tx_pipeline))?;

    let ipc_handle = std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || ipc_actor.run(ipc_rx, bus_tx_ipc))?;

    // Set up Ctrl+C to send Shutdown.
    let bus_tx_ctrlc = bus_tx.clone();
    ctrlc::set_handler(move || {
        log::info!("received Ctrl+C — shutting down");
        bus_tx_ctrlc.send(Message::Shutdown).ok();
    })
    .context("failed to set Ctrl+C handler")?;

    log::info!("voicerouter ready — all actors running");

    // Wait for actors to finish with 5s global timeout.
    // park_timeout unblocks after deadline even if threads haven't joined.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for handle in [hotkey_handle, core_handle, pipeline_handle, ipc_handle, bus_handle] {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            log::warn!("shutdown timeout exceeded, force-exiting");
            break;
        }
        // join() blocks but actors should exit within timeout on Shutdown.
        let _ = handle.join();
    }

    log::info!("voicerouter stopped");
    Ok(())
}

fn parse_condition(s: &str) -> voicerouter::pipeline::stage::Condition {
    use voicerouter::pipeline::stage::Condition;
    if let Some(prefix) = s.strip_prefix("starts_with:") {
        Condition::StartsWith(prefix.to_string())
    } else if let Some(val) = s.strip_prefix("output_eq:") {
        Condition::OutputEq(val.to_string())
    } else if let Some(val) = s.strip_prefix("output_contains:") {
        Condition::OutputContains(val.to_string())
    } else {
        Condition::Always
    }
}
```

Also update imports at the top of `main.rs` — remove old `Router` import, add actor imports. Remove old `handle_event`, `on_start_recording`, `on_stop_recording`, `validate_recording`, `transcribe_and_process`, `transcribe`, `add_punctuation`, `init_punctuator`, `beep_if` functions.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles. Fix any issues.

- [ ] **Step 3: Run all existing tests**

Run: `cargo test`
Expected: All non-router tests pass. Router tests may fail — that's expected since router is being replaced.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: rewrite run_daemon() to use actor-based architecture"
```

---

### Task 11: Delete old router module

**Files:**
- Delete: `src/router/` (entire directory)
- Modify: `src/lib.rs`

- [ ] **Step 1: Remove `pub mod router` from lib.rs**

- [ ] **Step 2: Delete `src/router/` directory**

```bash
rm -r src/router/
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass. No references to old router remain.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: remove old router module, replaced by pipeline"
```

---

### Task 12: Integration smoke test — end-to-end actor flow

**Files:**
- Add integration test or manual test

- [ ] **Step 1: Write integration test for bus + pipeline**

Add to `src/pipeline/mod.rs` tests:

```rust
#[test]
fn integration_bus_routes_transcript_to_pipeline() {
    use crate::actor::{Bus, Message};

    let received = Arc::new(Mutex::new(Vec::new()));
    let stages = vec![make_stage("record", Box::new(RecordingHandler {
        received: Arc::clone(&received),
    }), None)];

    let (pipeline_tx, pipeline_rx) = crossbeam::channel::bounded(8);
    let (bus_out_tx, _bus_out_rx) = crossbeam::channel::bounded(8);

    let mut bus = Bus::new();
    bus.subscribe("Transcript", pipeline_tx);

    // Simulate: CoreActor publishes Transcript → Bus → PipelineActor
    bus.publish(Message::Transcript {
        text: "测试消息".into(),
        raw: "测试消息".into(),
    });

    let msg = pipeline_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    if let Message::Transcript { text, .. } = msg {
        execute_pipeline(&stages, &text, &bus_out_tx);
    }

    assert_eq!(*received.lock().unwrap(), vec!["测试消息"]);
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 3: Manual smoke test**

Run: `cargo build --release && target/release/voicerouter --verbose`
Expected: Starts up, shows "all actors running", responds to hotkey.

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "test: add integration test for bus→pipeline flow"
```

---

## Phase 2: TTS Module

### Task 13: TTS engine trait and sherpa-onnx implementation

**Files:**
- Create: `src/tts/mod.rs`
- Create: `src/tts/sherpa.rs`
- Modify: `src/lib.rs`
- Modify: `src/config.rs` (TtsConfig already added in Task 5)

- [ ] **Step 1: Write tests for TTS engine trait**

In `src/tts/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_actor_name() {
        let actor = TtsActor::new(crate::config::TtsConfig::default());
        assert_eq!(Actor::name(&actor), "tts");
    }
}
```

- [ ] **Step 2: Implement TtsEngine trait and TtsActor**

`src/tts/mod.rs`:

```rust
//! TTS actor and engine abstraction.

pub mod sherpa;

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message};
use crate::config::TtsConfig;

/// Abstract TTS engine.
pub trait TtsEngine: Send {
    fn synthesize(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    fn sample_rate(&self) -> u32;
}

pub struct TtsActor {
    config: TtsConfig,
}

impl TtsActor {
    #[must_use]
    pub fn new(config: TtsConfig) -> Self {
        Self { config }
    }
}

impl Actor for TtsActor {
    fn name(&self) -> &str {
        "tts"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.enabled {
            log::info!("[tts] disabled, actor idle");
            for msg in inbox {
                if matches!(msg, Message::Shutdown) { break; }
            }
            return;
        }

        // Lazy-init engine on first SpeakRequest.
        let mut engine: Option<Box<dyn TtsEngine>> = None;

        for msg in inbox {
            match msg {
                Message::SpeakRequest { text, source } => {
                    if engine.is_none() {
                        match self.config.engine.as_str() {
                            "sherpa-onnx" => {
                                match sherpa::SherpaTts::new(&self.config) {
                                    Ok(e) => engine = Some(Box::new(e)),
                                    Err(e) => {
                                        log::error!("[tts] engine init failed: {e:#}");
                                        continue;
                                    }
                                }
                            }
                            other => {
                                log::error!("[tts] unknown engine: {other}");
                                continue;
                            }
                        }
                    }

                    if self.config.mute_mic_during_playback {
                        outbox.send(Message::MuteInput).ok();
                    }

                    log::info!("[tts] speaking: {text:?}");
                    if let Some(ref eng) = engine {
                        match eng.synthesize(&text) {
                            Ok(samples) => {
                                if let Err(e) = play_audio(&samples, eng.sample_rate()) {
                                    log::error!("[tts] playback failed: {e:#}");
                                }
                            }
                            Err(e) => log::error!("[tts] synthesis failed: {e:#}"),
                        }
                    }

                    if self.config.mute_mic_during_playback {
                        outbox.send(Message::UnmuteInput).ok();
                    }
                    outbox.send(Message::SpeakDone).ok();
                }
                Message::Shutdown => break,
                _ => {}
            }
        }

        log::info!("[tts] stopped");
    }
}

fn play_audio(samples: &[f32], sample_rate: u32) -> anyhow::Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no output device"))?;

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples = samples.to_vec();
    let pos = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let pos_clone = std::sync::Arc::clone(&pos);
    let len = samples.len();

    let (done_tx, done_rx) = crossbeam::channel::bounded(1);

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                let idx = pos_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *sample = if idx < len { samples[idx] } else { 0.0 };
            }
            if pos_clone.load(std::sync::atomic::Ordering::Relaxed) >= len {
                let _ = done_tx.try_send(());
            }
        },
        |err| log::error!("[tts] stream error: {err}"),
        None,
    )?;

    stream.play()?;
    let _ = done_rx.recv_timeout(std::time::Duration::from_secs(30));

    Ok(())
}
```

`src/tts/sherpa.rs`:

```rust
//! sherpa-onnx TTS engine implementation.

use anyhow::{Context, Result};

use super::TtsEngine;
use crate::asr::models::expand_tilde;
use crate::config::TtsConfig;

pub struct SherpaTts {
    // sherpa-onnx TTS handle will go here once sherpa-onnx TTS API is available.
    // For now, this is a placeholder that logs a warning.
    sample_rate: u32,
}

impl SherpaTts {
    pub fn new(config: &TtsConfig) -> Result<Self> {
        let model_dir = expand_tilde(&config.model_dir)
            .context("TTS model dir path error")?;
        log::info!("[tts/sherpa] model dir: {}", model_dir.display());
        // TODO: Initialize sherpa-onnx TTS when model is available.
        // For now, this is a stub that will be completed when TTS models
        // are downloaded and tested.
        Ok(Self { sample_rate: 22050 })
    }
}

impl TtsEngine for SherpaTts {
    fn synthesize(&self, text: &str) -> Result<Vec<f32>> {
        log::warn!("[tts/sherpa] TTS synthesis not yet implemented, skipping: {text:?}");
        // Return silence for now — will be replaced with actual synthesis.
        Ok(vec![0.0; self.sample_rate as usize]) // 1 second of silence
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod tts;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p voicerouter tts -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Wire TtsActor into run_daemon()**

In `src/main.rs`, add TTS actor channels and bus subscriptions:

```rust
// Add TTS channels.
let (tts_tx, tts_rx) = crossbeam::channel::bounded::<Message>(32);
bus.subscribe("SpeakRequest", tts_tx.clone());
bus.subscribe("Shutdown", tts_tx.clone());
// MuteInput/UnmuteInput from TTS → CoreActor already subscribed.
// SpeakDone → CoreActor already subscribed (add if needed).

let tts_actor = voicerouter::tts::TtsActor::new(config.tts.clone());
let bus_tx_tts = bus_tx.clone();
let tts_handle = std::thread::Builder::new()
    .name("tts".into())
    .spawn(move || tts_actor.run(tts_rx, bus_tx_tts))?;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: Compiles.

- [ ] **Step 6: Commit**

```bash
git add src/tts/ src/lib.rs src/main.rs
git commit -m "feat(tts): add TtsActor with sherpa-onnx engine stub and cpal playback"
```

---

## Phase 3: Wake Word Detection

### Task 14: WakewordDetector — sliding window + prefix match

**Files:**
- Create: `src/wakeword/detector.rs`
- Create: `src/wakeword/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for WakewordDetector**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_matches_phrase() {
        let detector = WakewordDetector::new(
            vec!["小助手".into(), "hey router".into()],
        );
        assert_eq!(detector.check("小助手帮我搜索"), Some(("小助手", "帮我搜索")));
        assert_eq!(detector.check("hey router do something"), Some(("hey router", "do something")));
        assert_eq!(detector.check("random text"), None);
    }

    #[test]
    fn detector_returns_empty_remainder() {
        let detector = WakewordDetector::new(vec!["小助手".into()]);
        assert_eq!(detector.check("小助手"), Some(("小助手", "")));
    }

    #[test]
    fn detector_empty_phrases_never_matches() {
        let detector = WakewordDetector::new(Vec::new());
        assert_eq!(detector.check("anything"), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p voicerouter wakeword -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement WakewordDetector**

`src/wakeword/detector.rs`:

```rust
//! Wake word detection via ASR output prefix matching.

/// Detects configured wake phrases in ASR transcript text.
pub struct WakewordDetector {
    phrases: Vec<String>,
}

impl WakewordDetector {
    #[must_use]
    pub fn new(phrases: Vec<String>) -> Self {
        Self { phrases }
    }

    /// Check if text starts with any wake phrase.
    /// Returns (matched_phrase, remainder) or None.
    pub fn check<'a>(&self, text: &'a str) -> Option<(&str, &'a str)> {
        for phrase in &self.phrases {
            if text.starts_with(phrase.as_str()) {
                let remainder = text[phrase.len()..].trim_start();
                return Some((phrase.as_str(), remainder));
            }
        }
        None
    }
}
```

`src/wakeword/mod.rs`:

```rust
//! Wakeword actor — continuous ASR-based wake word detection.

pub mod detector;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, Sender};

use crate::actor::{Actor, Message, Metadata};
use crate::asr::AsrEngine;
use crate::config::{Config, WakewordAction};
use detector::WakewordDetector;

pub struct WakewordActor {
    config: Config,
}

impl WakewordActor {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Actor for WakewordActor {
    fn name(&self) -> &str {
        "wakeword"
    }

    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        if !self.config.wakeword.enabled {
            log::info!("[wakeword] disabled, actor idle");
            for msg in inbox {
                if matches!(msg, Message::Shutdown) { break; }
            }
            return;
        }

        let detector = WakewordDetector::new(self.config.wakeword.phrases.clone());
        let window_samples = (self.config.wakeword.window_seconds
            * self.config.audio.sample_rate as f64) as usize;
        let stride = Duration::from_secs_f64(self.config.wakeword.stride_seconds);

        // Init separate ASR engine for wakeword detection.
        let asr_model = if self.config.wakeword.model.is_empty() {
            self.config.asr.model.clone()
        } else {
            self.config.wakeword.model.clone()
        };
        let asr_config = crate::config::AsrConfig {
            model: asr_model,
            model_dir: self.config.asr.model_dir.clone(),
        };
        let mut asr = match AsrEngine::new(&asr_config) {
            Ok(e) => e,
            Err(e) => {
                log::error!("[wakeword] ASR init failed: {e:#}");
                return;
            }
        };

        log::info!("[wakeword] ready, phrases: {:?}", self.config.wakeword.phrases);

        // Main detection loop — this is a skeleton that will be connected
        // to AudioSource in the mic-sharing implementation.
        // For now, it processes messages and waits for audio integration.
        let mut muted = false;
        loop {
            match inbox.try_recv() {
                Ok(Message::Shutdown) => break,
                Ok(Message::MuteInput) => { muted = true; }
                Ok(Message::UnmuteInput) => { muted = false; }
                _ => {}
            }

            if muted {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            // TODO: Read audio samples from AudioSource channel.
            // For now, sleep for stride interval.
            // When AudioSource is implemented, this will:
            // 1. Collect window_samples from audio channel
            // 2. Run ASR on the window
            // 3. Check detector.check() on ASR output
            // 4. If matched, emit StartListening or PipelineInput

            std::thread::sleep(stride);
        }

        log::info!("[wakeword] stopped");
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod wakeword;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p voicerouter wakeword -- --nocapture`
Expected: All 3 tests PASS.

- [ ] **Step 5: Wire WakewordActor into run_daemon()**

Add channels, subscriptions, and spawn in `src/main.rs` (similar to TTS actor).

- [ ] **Step 6: Commit**

```bash
git add src/wakeword/ src/lib.rs src/main.rs
git commit -m "feat(wakeword): add WakewordActor with ASR-based phrase detection"
```

---

## Phase 4: DAG Workflow Orchestration

### Task 15: Add ureq dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add ureq**

```toml
ureq = { version = "2", default-features = false }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add ureq dependency for HTTP handler"
```

---

### Task 16: DAG executor — topological sort + parallel execution

**Files:**
- Create: `src/pipeline/dag.rs`

- [ ] **Step 1: Write tests for topological sort and DAG execution**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topo_sort_linear_chain() {
        let stages = vec!["a", "b", "c"];
        let deps = vec![None, Some("a"), Some("b")];
        let order = topo_sort(&stages, &deps).unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn topo_sort_fan_out() {
        // a → b, a → c (b and c are independent)
        let stages = vec!["a", "b", "c"];
        let deps = vec![None, Some("a"), Some("a")];
        let order = topo_sort(&stages, &deps).unwrap();
        assert_eq!(order[0], 0); // a first
        // b and c in any order
    }

    #[test]
    fn topo_sort_detects_cycle() {
        let stages = vec!["a", "b"];
        let deps = vec![Some("b"), Some("a")];
        assert!(topo_sort(&stages, &deps).is_err());
    }

    #[test]
    fn topo_sort_detects_missing_dependency() {
        let stages = vec!["a"];
        let deps = vec![Some("nonexistent")];
        assert!(topo_sort(&stages, &deps).is_err());
    }
}
```

- [ ] **Step 2: Run tests, verify fail**

- [ ] **Step 3: Implement topological sort**

```rust
//! DAG pipeline execution: topological sort and parallel stage execution.

use std::collections::HashMap;

use anyhow::{bail, Result};

/// Topological sort of stages. Returns execution order as indices.
pub fn topo_sort(
    stage_names: &[&str],
    dependencies: &[Option<&str>],
) -> Result<Vec<usize>> {
    let name_to_idx: HashMap<&str, usize> = stage_names.iter()
        .enumerate()
        .map(|(i, n)| (*n, i))
        .collect();

    let n = stage_names.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, dep) in dependencies.iter().enumerate() {
        if let Some(dep_name) = dep {
            let dep_idx = name_to_idx.get(dep_name)
                .ok_or_else(|| anyhow::anyhow!(
                    "stage '{}' depends on unknown stage '{dep_name}'",
                    stage_names[i]
                ))?;
            adj[*dep_idx].push(i);
            in_degree[i] += 1;
        }
    }

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(n);

    while let Some(node) = queue.pop() {
        order.push(node);
        for &next in &adj[node] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push(next);
            }
        }
    }

    if order.len() != n {
        bail!("cycle detected in pipeline DAG");
    }

    Ok(order)
}
```

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/dag.rs
git commit -m "feat(pipeline): add DAG topological sort"
```

---

### Task 17: Pipe handler

**Files:**
- Create: `src/pipeline/handlers/pipe.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx(cmd: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert("command".into(), cmd.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn pipe_captures_stdout() {
        let handler = PipeHandler;
        let ctx = ctx("cat");
        let result = handler.handle("hello", &ctx).unwrap();
        match result {
            HandlerResult::Forward(text) => assert_eq!(text.trim(), "hello"),
            _ => panic!("expected Forward"),
        }
    }

    #[test]
    fn pipe_with_template() {
        let handler = PipeHandler;
        let ctx = ctx("echo prefix-{text}");
        let result = handler.handle("world", &ctx).unwrap();
        match result {
            HandlerResult::Forward(text) => assert_eq!(text.trim(), "prefix-world"),
            _ => panic!("expected Forward"),
        }
    }
}
```

- [ ] **Step 2: Implement PipeHandler**

```rust
//! Pipe handler — write text to subprocess stdin, read stdout.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct PipeHandler;

impl Handler for PipeHandler {
    fn name(&self) -> &str { "pipe" }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let cmd = ctx.get("command")
            .ok_or_else(|| anyhow::anyhow!("pipe handler requires 'command' param"))?;

        let effective_cmd = if cmd.contains("{text}") {
            cmd.replace("{text}", text)
        } else {
            cmd.to_string()
        };

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&effective_cmd)
            .stdin(if !cmd.contains("{text}") { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn pipe process")?;

        // Write text to stdin if no {text} template was used.
        if !cmd.contains("{text}") {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes()).ok();
            }
        }

        let output = child.wait_with_output().context("pipe process failed")?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("[pipe] stderr: {}", stderr.trim());
            bail!("pipe command failed: {effective_cmd}");
        }

        Ok(HandlerResult::Forward(stdout))
    }
}
```

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Register in handlers/mod.rs**

Add `pub mod pipe;` and update `build_handler` to include `"pipe"`.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/handlers/pipe.rs src/pipeline/handlers/mod.rs
git commit -m "feat(pipeline): add pipe handler for stdin/stdout subprocess"
```

---

### Task 18: HTTP handler

**Files:**
- Create: `src/pipeline/handlers/http.rs`

- [ ] **Step 1: Write tests (mock with local echo)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    #[test]
    fn http_handler_name() {
        assert_eq!(HttpHandler.name(), "http");
    }

    #[test]
    fn http_requires_url_param() {
        let ctx = StageContext {
            stage_name: "test".into(),
            params: HashMap::new(),
        };
        assert!(HttpHandler.handle("hello", &ctx).is_err());
    }
}
```

- [ ] **Step 2: Implement HttpHandler**

```rust
//! HTTP handler — sync HTTP requests via ureq.

use anyhow::{bail, Result};

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct HttpHandler;

impl Handler for HttpHandler {
    fn name(&self) -> &str { "http" }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        let url = ctx.get("url")
            .ok_or_else(|| anyhow::anyhow!("http handler requires 'url' param"))?;
        let method = ctx.get("method").unwrap_or("POST");

        let url = url.replace("{text}", text);

        let response = match method.to_uppercase().as_str() {
            "GET" => ureq::get(&url).call(),
            "POST" => {
                let body = ctx.get("body")
                    .map(|b| b.replace("{text}", text))
                    .unwrap_or_else(|| text.to_string());
                ureq::post(&url)
                    .set("Content-Type", "application/json")
                    .send_string(&body)
            }
            other => bail!("unsupported HTTP method: {other}"),
        };

        match response {
            Ok(resp) => {
                let body = resp.into_string()?;
                Ok(HandlerResult::Forward(body))
            }
            Err(e) => bail!("HTTP request failed: {e}"),
        }
    }
}
```

- [ ] **Step 3: Register in handlers/mod.rs**

Add `pub mod http;` and update `build_handler`.

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/handlers/http.rs src/pipeline/handlers/mod.rs
git commit -m "feat(pipeline): add HTTP handler using ureq"
```

---

### Task 19: Transform handler

**Files:**
- Create: `src/pipeline/handlers/transform.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::StageContext;
    use std::collections::HashMap;

    fn ctx_with(key: &str, val: &str) -> StageContext {
        let mut params = HashMap::new();
        params.insert(key.into(), val.into());
        StageContext { stage_name: "test".into(), params }
    }

    #[test]
    fn template_replaces_text() {
        let ctx = ctx_with("template", "prefix: {text} :suffix");
        let result = TransformHandler.handle("hello", &ctx).unwrap();
        match result {
            HandlerResult::Forward(t) => assert_eq!(t, "prefix: hello :suffix"),
            _ => panic!("expected Forward"),
        }
    }

    #[test]
    fn regex_replaces_pattern() {
        let mut params = HashMap::new();
        params.insert("regex".into(), r"\d+".into());
        params.insert("replacement".into(), "NUM".into());
        let ctx = StageContext { stage_name: "test".into(), params };
        let result = TransformHandler.handle("abc 123 def", &ctx).unwrap();
        match result {
            HandlerResult::Forward(t) => assert_eq!(t, "abc NUM def"),
            _ => panic!("expected Forward"),
        }
    }
}
```

- [ ] **Step 2: Implement TransformHandler**

```rust
//! Transform handler — built-in text transformations.

use anyhow::Result;

use crate::pipeline::handler::{Handler, HandlerResult};
use crate::pipeline::stage::StageContext;

pub struct TransformHandler;

impl Handler for TransformHandler {
    fn name(&self) -> &str { "transform" }

    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult> {
        // Template mode: replace {text} in template string.
        if let Some(template) = ctx.get("template") {
            return Ok(HandlerResult::Forward(template.replace("{text}", text)));
        }

        // Regex mode: replace pattern with replacement.
        if let Some(pattern) = ctx.get("regex") {
            let replacement = ctx.get("replacement").unwrap_or("");
            let re = regex_lite::Regex::new(pattern)?;
            let result = re.replace_all(text, replacement).to_string();
            return Ok(HandlerResult::Forward(result));
        }

        // No transform specified — pass through.
        Ok(HandlerResult::Forward(text.to_string()))
    }
}
```

Note: This uses `regex-lite` (zero-dependency regex) instead of full `regex` crate. Add to `Cargo.toml`:
```toml
regex-lite = "0.1"
```

- [ ] **Step 3: Register in handlers/mod.rs**

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/pipeline/handlers/transform.rs src/pipeline/handlers/mod.rs
git commit -m "feat(pipeline): add transform handler with template and regex modes"
```

---

### Task 20: Wire DAG execution into PipelineActor

**Files:**
- Modify: `src/pipeline/mod.rs`
- Modify: `src/pipeline/dag.rs`
- Modify: `src/config.rs`

- [ ] **Step 1: Write test for DAG pipeline execution**

```rust
#[test]
fn dag_pipeline_fan_out() {
    // classify → inject (output_eq:note)
    // classify → shell (output_eq:command)
    // Classifier always returns "note"
    struct ClassifyHandler;
    impl Handler for ClassifyHandler {
        fn name(&self) -> &str { "classify" }
        fn handle(&self, _text: &str, _ctx: &StageContext) -> anyhow::Result<HandlerResult> {
            Ok(HandlerResult::Forward("note".into()))
        }
    }

    let received = Arc::new(Mutex::new(Vec::new()));
    let stages = vec![
        Stage {
            name: "classify".into(),
            handler: Box::new(ClassifyHandler),
            condition: None,
            params: HashMap::new(),
            timeout: Duration::from_secs(10),
            after: None,
        },
        Stage {
            name: "note_handler".into(),
            handler: Box::new(RecordingHandler { received: Arc::clone(&received) }),
            condition: Some(Condition::OutputEq("note".into())),
            params: HashMap::new(),
            timeout: Duration::from_secs(10),
            after: Some("classify".into()),
        },
    ];
    let (tx, _rx) = crossbeam::channel::bounded(8);
    execute_dag(&stages, "test input", &tx);
    let texts = received.lock().unwrap();
    assert_eq!(texts.len(), 1);
    // note_handler should receive the original input text since
    // classify's output "note" matched OutputEq("note").
    assert_eq!(texts[0], "test input");
}
```

- [ ] **Step 2: Add `after` field to Stage struct**

Extend `Stage` in `src/pipeline/stage.rs`:
```rust
pub struct Stage {
    // ... existing fields ...
    pub after: Option<String>,
}
```

- [ ] **Step 3: Implement `execute_dag` function**

In `src/pipeline/dag.rs`, add DAG execution that uses topological sort, evaluates conditions with results map, and supports parallel siblings via `crossbeam::scope`.

- [ ] **Step 4: Update PipelineActor to choose linear vs DAG mode**

If any stage has `after` set → use DAG execution. Otherwise → use linear chain.

- [ ] **Step 5: Add ErrorPolicy support**

Read `config.pipeline.error_policy` and apply `FailFast` or `BestEffort` behavior.

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/pipeline/
git commit -m "feat(pipeline): add DAG execution with parallel stages and error policies"
```

---

### Task 21: Final integration — all phases working together

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Build release binary**

Run: `cargo build --release`
Expected: Builds without warnings.

- [ ] **Step 3: Manual smoke test**

1. Start daemon: `target/release/voicerouter --verbose`
2. Verify hotkey triggers recording → ASR → pipeline → inject
3. Test IPC: `echo '{"method":"status"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/voicerouter.sock`
4. Test pipeline.send via IPC

- [ ] **Step 4: Commit any final fixes**

- [ ] **Step 5: Update TODO.md**

Mark completed items, add new Phase 2-4 items that need real-world testing.

```bash
git add docs/plans/TODO.md
git commit -m "docs: update TODO with framework implementation status"
```
