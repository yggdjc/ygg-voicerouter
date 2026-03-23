# Voice Interaction Framework — Design Spec

> Evolve voicerouter from a voice input tool into a local, offline-first voice
> interaction framework: STT + TTS + wake word + composable handler pipeline.

## Status

- **Date**: 2026-03-23
- **Phase**: Design approved, pending implementation

## Goals

1. Transform voicerouter into a general-purpose **voice infrastructure layer** for
   Linux, supporting local AI assistants, desktop automation, and productivity
   pipelines.
2. Maintain the project's core values: offline-first, single binary, low resource
   footprint, zero cloud dependency.
3. Deliver in four incremental phases, each independently useful.

## Non-Goals

- Cloud/SaaS ASR or TTS backends
- Mobile or macOS/Windows support
- GUI or web dashboard
- Streaming/incremental ASR (current batch mode is sufficient)

---

## Architecture Overview

### Actor Model with Central Bus

All components run as independent actors (own thread + `crossbeam::channel`).
A lightweight `Bus` routes typed messages between actors. No async runtime
required.

```
┌─────────────┐  ┌──────────────┐
│ HotkeyActor │  │WakewordActor │ (Phase 3)
└──────┬──────┘  └──────┬───────┘
       │                │
   StartListening  StartListening / PipelineInput
       │                │
       └───────┬────────┘
               ↓
          ┌─────────┐        ┌──────────┐
          │   Bus   │←──────→│ IpcActor │←──→ external processes (Unix socket)
          └────┬────┘        └──────────┘
               ↓
         ┌───────────┐
         │ CoreActor │  audio + denoise + ASR + postprocess
         └─────┬─────┘
               │ Transcript
               ↓
       ┌────────────────┐
       │ PipelineActor  │  DAG orchestration (Phase 4)
       │                │
       │  stage₁ → stage₂ ──→ stage₃  │
       │       ↘ stage₄               │
       └───┬────────┬────────┬────────┘
           ↓        ↓        ↓
       InjectH   ShellH   TtsActor (Phase 2)
                  PipeH    HttpH    (Phase 4)
```

### Message Types

```rust
#[derive(Clone, Debug)]
enum Message {
    // ASR domain
    Transcript { text: String, raw: String },

    // Pipeline domain
    PipelineInput { text: String, metadata: Metadata },
    PipelineOutput { text: String, stage: String },

    // TTS domain (Phase 2)
    SpeakRequest { text: String, source: SpeakSource },
    SpeakDone,

    // Mic control (TTS echo prevention)
    MuteInput,
    UnmuteInput,

    // Control
    StartListening,
    StopListening,
    Shutdown,
}

enum SpeakSource {
    LlmReply,
    SystemFeedback,
}

/// Context carried through the pipeline for condition evaluation.
struct Metadata {
    source: String,      // "hotkey", "wakeword", "ipc"
    timestamp: Instant,
}
```

Removed from original draft: `AudioSamples`, `SilenceDetected` (internal to
CoreActor, never cross the bus), and `confidence: f32` (no consumer uses it).

`Message` derives `Clone` because `Bus::publish()` clones messages for 1:N
fan-out. All fields are `String`/`Copy` types — clone cost is negligible at
the system's message throughput (~1-2 messages/second).

### Actor Trait

```rust
trait Actor: Send + 'static {
    fn name(&self) -> &str;
    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>);
}
```

Each actor owns its thread, communicates via `crossbeam::channel`. Internal
logic remains synchronous.

### Bus Routing Model

The bus uses **topic-based 1:N routing**, not 1:1. Each message variant maps to
a static list of subscriber actors:

```rust
struct Bus {
    /// message_variant_name → list of subscriber senders
    subscriptions: HashMap<&'static str, Vec<Sender<Message>>>,
}

impl Bus {
    fn publish(&self, msg: Message) {
        let topic = msg.topic();  // e.g. "MuteInput", "Transcript"
        if let Some(subs) = self.subscriptions.get(topic) {
            for sender in subs {
                if let Err(e) = sender.send(msg.clone()) {
                    // Warn on Shutdown delivery failure — actor may miss exit signal.
                    if matches!(msg, Message::Shutdown) {
                        log::warn!("failed to deliver Shutdown: {e}");
                    }
                }
            }
        }
    }
}
```

Subscription table (registered at startup, immutable):

| Message | Subscribers |
|---------|------------|
| `StartListening` | CoreActor |
| `StopListening` | CoreActor, HotkeyActor (reset state machine on forced stop) |
| `Transcript` | PipelineActor, IpcActor (push to subscribers) |
| `PipelineInput` | PipelineActor |
| `PipelineOutput` | IpcActor (push to subscribers) |
| `SpeakRequest` | TtsActor (Phase 2) |
| `SpeakDone` | CoreActor, WakewordActor (Phase 3) |
| `MuteInput` | CoreActor, WakewordActor (Phase 3) |
| `UnmuteInput` | CoreActor, WakewordActor (Phase 3) |
| `Shutdown` | ALL actors |

This resolves the fan-out requirement: `MuteInput` reaches both CoreActor and
WakewordActor without broadcast overhead.

---

## Phase 1: Handler Pipeline + IPC

### Actor Breakdown

| Actor | Thread | Responsibility | Input | Output |
|-------|--------|----------------|-------|--------|
| `HotkeyActor` | 1 | evdev listen + state machine | `StopListening` (reset on forced stop) | `StartListening`, `StopListening` |
| `CoreActor` | 1 | Audio capture, silence detection, ASR, postprocess | `StartListening`, `StopListening`, `MuteInput`, `UnmuteInput` | `Transcript` |
| `PipelineActor` | 1 | Handler chain execution | `Transcript`, `PipelineInput` | `PipelineOutput`, `SpeakRequest` |
| `IpcActor` | 1 | Unix socket, JSON-RPC | External connections | `PipelineInput`; pushes `Transcript` events to subscribers |

### CoreActor Message Processing Loop

CoreActor must simultaneously: (a) block on inbox for control messages, and
(b) monitor audio levels for silence detection during recording.

Solution: `crossbeam::select!` with a timeout channel.

```rust
impl Actor for CoreActor {
    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
        let mut state = CoreState::Idle;

        loop {
            match state {
                CoreState::Idle => {
                    // Block on inbox — no audio processing needed
                    match inbox.recv() {
                        Ok(Message::StartListening) => {
                            self.audio.start_recording();
                            self.recording_start = Some(Instant::now());
                            state = CoreState::Recording;
                        }
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
                CoreState::Recording => {
                    // Poll inbox with 10ms timeout for silence monitoring
                    crossbeam::select! {
                        recv(inbox) -> msg => match msg {
                            Ok(Message::StopListening) => {
                                state = self.finalize_recording(&outbox);
                            }
                            Ok(Message::MuteInput) => {
                                state = CoreState::Muted;
                            }
                            Ok(Message::Shutdown) => break,
                            _ => {}
                        },
                        default(Duration::from_millis(10)) => {
                            // Check recording timeout
                            if self.exceeded_max_record() {
                                state = self.finalize_recording(&outbox);
                                outbox.send(Message::StopListening).ok();
                            }
                            // Silence detection continues via audio pipeline
                        }
                    }
                }
                CoreState::Muted => {
                    // Paused — wait for UnmuteInput or Shutdown
                    match inbox.recv() {
                        Ok(Message::UnmuteInput) => state = CoreState::Idle,
                        Ok(Message::Shutdown) => break,
                        _ => {}
                    }
                }
            }
        }
    }
}
```

`finalize_recording()` calls `audio.stop_recording()` → `validate_recording()`
→ `transcribe()` → `postprocess()` → publishes `Transcript` to outbox. This is
the same logic as current `on_stop_recording()` in `main.rs`, just moved into
CoreActor.

### Handler Trait (revised)

```rust
trait Handler: Send + Sync {
    fn name(&self) -> &str;

    /// Process input text, returning what to do next.
    ///
    /// `ctx` provides access to stage configuration (command template, URL, etc.)
    /// so handlers don't need to carry config internally.
    fn handle(&self, text: &str, ctx: &StageContext) -> Result<HandlerResult>;
}

/// Read-only view of the stage's configuration, passed to handler at execution.
/// Uses a flat key-value map so new handler types don't require struct changes.
struct StageContext {
    stage_name: String,
    params: HashMap<String, String>,  // "command", "url", "method", "body", etc.
}

enum HandlerResult {
    /// Pass transformed text to the next stage in the chain.
    Forward(String),
    /// Send a message to the bus (e.g. SpeakRequest). Pipeline continues.
    Emit(Message),
    /// Both forward to next stage AND emit to bus.
    ForwardAndEmit(String, Message),
    /// Terminate pipeline — no further stages execute.
    Done,
}
```

Design decisions:
- Handler receives `&str` (not `Message`) — handlers are text processors, not
  message routers. PipelineActor extracts text before calling handlers:
  `Transcript` → uses `text` field (postprocessed), `PipelineInput` → uses
  `text` field. The `raw` field is only forwarded to IPC event subscribers.
- `StageContext` provides stage config to handlers at execution time, resolving
  the Phase 1 / Phase 4 compatibility issue. Handlers like `shell` and `http`
  read their command/URL templates from context rather than construction time.
- `ForwardAndEmit` handles the common case: forward text to next stage AND
  emit a side-effect (e.g., TTS speak while also logging).

### PipelineActor Orchestration (Phase 1)

PipelineActor receives `Transcript` or `PipelineInput`, extracts text, and
runs the stage chain:

```rust
fn execute_pipeline(&self, text: &str, outbox: &Sender<Message>) {
    let mut current_text = text.to_string();
    // Phase 4 adds: let mut results: HashMap<String, String> for DAG condition eval.
    // Phase 1 only uses Condition::StartsWith which needs no prior results.

    for stage in &self.stages {
        // Phase 1: conditions are limited to StartsWith (for router compat)
        if let Some(ref cond) = stage.condition {
            if !cond.matches_text(&current_text) {
                continue;  // skip this stage
            }
        }

        // Strip trigger prefix if condition was a StartsWith match
        let payload = stage.condition.as_ref()
            .and_then(|c| c.strip_prefix(&current_text))
            .unwrap_or(&current_text);

        let ctx = stage.to_context();
        match stage.handler.handle(&payload, &ctx) {
            Ok(HandlerResult::Forward(text)) => current_text = text,
            Ok(HandlerResult::Emit(msg)) => { outbox.send(msg).ok(); }
            Ok(HandlerResult::ForwardAndEmit(text, msg)) => {
                current_text = text;
                outbox.send(msg).ok();
            }
            Ok(HandlerResult::Done) => break,
            Err(e) => {
                log::error!("[pipeline] stage '{}' failed: {e:#}", stage.name);
                break;  // Phase 1: fail-fast. Phase 4: configurable.
            }
        }
    }
}
```

`Condition` has two evaluation methods:
- `matches_text(&str)`: Phase 1 — evaluates against current text only
  (`StartsWith`, `Always`).
- `matches_with_results(&str, &HashMap<String, String>)`: Phase 4 — also
  checks upstream stage outputs (`OutputEq`, `OutputContains`).

This preserves the current router's prefix-matching + trigger-stripping logic
inside PipelineActor, using `Condition::StartsWith` for migrated router rules.

### Router → Pipeline Migration Logic

At config load time, `[[router.rules]]` entries are converted:

```rust
// router rule:
//   trigger = "搜索"
//   handler = "shell"
//   command = "firefox https://google.com/search?q={text}"
//
// becomes pipeline stage:
//   name = "router_rule_0"
//   handler = "shell"
//   command = "firefox https://google.com/search?q={text}"
//   condition = "starts_with:搜索"
```

When `[[pipeline.stages]]` is present, `[[router.rules]]` is ignored with a
deprecation warning.

### IPC Protocol

Unix socket at `$XDG_RUNTIME_DIR/voicerouter.sock`. JSON-RPC 2.0:

```jsonc
// External → voicerouter: inject text into pipeline
{"method": "pipeline.send", "params": {"text": "hello world"}}

// External → voicerouter: subscribe to event stream
{"method": "events.subscribe", "params": {"types": ["transcript"]}}

// voicerouter → external: push event
{"method": "event", "params": {"type": "transcript", "text": "你好世界", "raw": "你好世界"}}

// External → voicerouter: control
{"method": "recording.start"}
{"method": "recording.stop"}
{"method": "status"}
```

### IPC Security and Error Handling

- **Authentication**: Unix socket permissions (0600) — only the owning user
  can connect. No application-level auth needed.
- **Max connections**: 8 concurrent. New connections beyond limit receive
  JSON-RPC error and are closed.
- **Malformed JSON**: Return JSON-RPC parse error (-32700), keep connection open.
- **Message size limit**: 64 KB per message. Oversized messages are rejected.
- **Client disconnect**: Subscription is removed, no effect on other actors.
- **Backpressure**: If a subscriber falls behind (channel full), events are
  dropped for that client with a warning log. Other clients unaffected.

### IPC Configuration

```toml
[ipc]
enabled = true
socket_path = ""              # default: $XDG_RUNTIME_DIR/voicerouter.sock
max_connections = 8
```

### Graceful Shutdown Protocol

On `Shutdown` message (triggered by SIGINT/SIGTERM):

1. **HotkeyActor**: Stop evdev polling, exit immediately.
2. **WakewordActor** (Phase 3): Stop audio capture, exit immediately.
3. **CoreActor**: If recording, discard current audio (do not transcribe). Exit.
4. **PipelineActor**: Drain in-flight pipeline execution (with 3s timeout),
   then exit. Stages that exceed timeout are abandoned.
5. **TtsActor** (Phase 2): Stop current playback immediately. Exit.
6. **IpcActor**: Close all client connections, remove socket file. Exit last.

Bus sends `Shutdown` to all actors simultaneously. Each actor handles it in its
own `run()` loop. Main thread joins all actor threads with a 5s global timeout,
then force-exits.

### Code Changes

- `main.rs`: `run_daemon()` → create 4 actors + bus, spawn threads, await shutdown
- `hotkey/`: Internal logic unchanged, wrapped in `HotkeyActor::run()` loop.
  `CancelAndToggle` event is translated to `StopListening` + `StartListening`
  pair on the bus. HotkeyActor also subscribes to `StopListening` to call
  `reset_state()` when CoreActor forces a stop (e.g., recording timeout).
- `audio/`, `asr/`, `postprocess/`: No changes, called internally by `CoreActor`
- `router/`: Rewritten as `pipeline/`, Handler trait signature changed
- `inject/`: Unchanged. `pipeline/handlers/inject.rs` wraps `inject::inject_text()`
  (delegation, not duplication)
- New: `actor.rs`, `ipc.rs`, `pipeline/`

Existing 140 tests: only router-related tests need adaptation to new Handler
trait. All others unaffected.

---

## Phase 2: TTS Module

### TtsActor

```
PipelineActor ──SpeakRequest──→ Bus ──→ TtsActor ──→ audio output
                                              │
                                         SpeakDone ──→ Bus
```

### Engine Selection

| Option | Pros | Cons | Recommendation |
|--------|------|------|----------------|
| sherpa-onnx TTS | Already a dependency, zero new deps | Limited voice selection | **Default** |
| piper (ONNX VITS) | Higher quality, more voices | New dependency | Future option |

Start with sherpa-onnx. Abstract behind an engine trait so piper can be added
later without touching TtsActor.

### Echo Prevention

TtsActor sends `MuteInput` before playback and `UnmuteInput` after. CoreActor
and WakewordActor both subscribe to these messages and pause accordingly.

### Configuration

```toml
[tts]
enabled = true
engine = "sherpa-onnx"     # sherpa-onnx | piper (future)
model = "vits-zh"
model_dir = "~/.cache/voicerouter/models"
speed = 1.0
mute_mic_during_playback = true
```

### New Files

- `src/tts/mod.rs` — TtsActor + engine trait
- `src/tts/sherpa.rs` — sherpa-onnx TTS implementation

---

## Phase 3: Wake Word Detection

### Approach: ASR-based Wake Word

Instead of a dedicated (low-accuracy) wake word model, reuse the existing ASR
engine with a sliding audio window.

| | Traditional KWS model | ASR-based |
|---|---|---|
| Accuracy | Low (especially Chinese) | High (reuses Paraformer) |
| CPU cost | ~1% | ~12.5% of one core |
| Custom phrases | Requires retraining | Config change only |
| Latency | ~200ms | ~800ms |

CPU cost is acceptable on the target hardware (i7-11700, 8 cores). The ASR
inference (~250ms per 2s window) runs only during idle listening and stops
automatically when recording begins.

### Microphone Sharing Architecture

WakewordActor and CoreActor both need mic access. The solution is a shared
`AudioSource` actor that owns the single cpal stream and multicasts samples:

```
┌──────────────┐
│  AudioSource │  (owns cpal stream, always-on when wakeword enabled)
│  (mic thread) │
└──────┬───────┘
       │ raw samples (ring buffer / crossbeam channel)
       ├──→ WakewordActor (continuous 2s sliding window)
       └──→ CoreActor (on-demand recording buffer)
```

- **AudioSource** is not a full actor — it is a shared component that runs the
  cpal input callback and writes samples to two bounded channels.
- When wakeword is disabled, AudioSource is owned solely by CoreActor (current
  behavior, no change).
- When wakeword is enabled, AudioSource starts on daemon launch and feeds both
  consumers. CoreActor's `start_recording` / `stop_recording` control whether
  it accumulates samples into its buffer, not whether the mic is active.
- WakewordActor stops consuming (drains channel) when it receives `MuteInput`
  or when CoreActor is actively recording (via a shared `AtomicBool` flag).

### ASR Instance Strategy

**Separate AsrEngine instance for WakewordActor** (recommended):

- Two Paraformer instances = ~600 MB total RAM. Acceptable on 32 GB target.
- No mutex contention — CoreActor and WakewordActor never block each other.
- WakewordActor can use a smaller/faster model (e.g., FunASR Nano at ~100 MB)
  since it only needs to detect a few phrases, not full transcription accuracy.

### Always-On Mic: UX Consequences

When wakeword is enabled, the mic is always active. This has visible effects:

| Concern | Mitigation |
|---------|-----------|
| PulseAudio/PipeWire shows "recording" indicator | Document in README; use PipeWire monitor source if available (passive, no indicator) |
| GNOME/KDE privacy indicator (red dot) | Unavoidable with direct mic access; document as expected behavior |
| Power consumption | Measure actual impact; cpal callback is interrupt-driven, CPU cost is minimal when not running ASR |
| Privacy | All processing is local/offline; no data leaves the machine; document clearly |

Wakeword is **opt-in** (default disabled) to avoid surprising users.

### WakewordActor

```
WakewordActor ──continuous 2s window──→ ASR ──→ prefix match ──→ StartListening / PipelineInput
      │
  MuteInput ──→ pause detection
```

- Sliding window: 2s audio, inference every 1s (50% overlap)
- On match: wake phrase is stripped, remaining content goes directly into
  pipeline as `PipelineInput` (single-utterance command without second trigger)
- Pauses on `MuteInput` (TTS playback) and during active recording
- Owns its own `AsrEngine` instance

### Pipeline Passthrough Semantics

When `action = "pipeline_passthrough"`:

- Wake phrase is stripped via prefix match (exact text match after ASR).
- If remaining text is non-empty: sent as `PipelineInput` directly.
- If remaining text is empty (user only said the wake phrase): falls back to
  `start_recording` behavior — CoreActor begins recording for the next
  utterance.
- Matching algorithm: exact prefix match on ASR text output (same as current
  router trigger matching). No fuzzy matching — ASR output is already
  normalized text.

### Configuration

```toml
[wakeword]
enabled = false              # opt-in
phrases = ["小助手", "hey router"]
window_seconds = 2.0
stride_seconds = 1.0
action = "start_recording"   # start_recording | pipeline_passthrough
model = ""                   # optional: override ASR model for wakeword (e.g. funasr-nano)
```

### New Files

- `src/wakeword/mod.rs` — WakewordActor
- `src/wakeword/detector.rs` — sliding window + ASR match logic

---

## Phase 4: Workflow Orchestration (DAG Pipeline)

### Pipeline Upgrade: Linear Chain → DAG

Stages gain `after` (dependency) and `condition` (guard) fields:

```toml
[[pipeline.stages]]
name = "classify"
handler = "pipe"
command = "intent-classifier"

[[pipeline.stages]]
name = "llm"
handler = "http"
url = "http://localhost:11434/api/generate"
method = "POST"
body = '{"prompt":"{text}"}'
after = "classify"
condition = "output_eq:chat"

[[pipeline.stages]]
name = "speak"
handler = "tts"
after = "llm"

[[pipeline.stages]]
name = "inject"
handler = "inject"
after = "classify"
condition = "output_eq:note"

[[pipeline.stages]]
name = "exec"
handler = "shell"
command = "{text}"
after = "classify"
condition = "output_eq:command"
```

### Execution Model

```rust
struct Stage {
    name: String,
    handler: Box<dyn Handler>,
    after: Option<String>,
    condition: Option<Condition>,
    timeout: Duration,            // default: 10s
}

enum Condition {
    Always,
    StartsWith(String),
    OutputEq(String),
    OutputContains(String),
}

struct PipelineExecution {
    stages: Vec<Stage>,
    results: HashMap<String, String>,  // stage name → output text
}
```

1. Topological sort on stages at config load time (fail-fast on cycles)
2. Execute stages with no dependencies first
3. On stage completion, evaluate downstream conditions, execute if satisfied
4. Sibling stages at same depth can run in parallel (`crossbeam::scope`)
5. Stage timeout → skip with warning log, downstream stages that depend on it
   are also skipped

### Error Propagation in DAG

- **Non-leaf stage fails** (e.g., `classify` errors): All downstream stages
  that depend on it are skipped. Other independent branches continue.
- **Leaf stage fails** (e.g., `inject` errors): Logged, no cascade effect.
- **Partial parallel failure**: Each sibling is independent. Failures in one
  branch do not affect others.
- **Pipeline-level error policy**: Configurable per-pipeline (not per-stage):
  - `fail_fast` (default Phase 1): First error stops entire pipeline.
  - `best_effort` (default Phase 4): Continue other branches on error.

```toml
[pipeline]
error_policy = "best_effort"   # fail_fast | best_effort
```

### Concurrency Limits

- **Max parallel stages per execution**: 4 (configurable). Prevents thread
  explosion from wide DAGs.
- **Max concurrent pipeline executions**: 2 (configurable). IPC `pipeline.send`
  calls are queued if limit is reached.

```toml
[pipeline]
max_parallel_stages = 4
max_concurrent_executions = 2
```

### New Handler Types

| Handler | Purpose |
|---------|---------|
| `pipe` | stdin/stdout pipe: write text to subprocess stdin, read stdout as output |
| `http` | HTTP POST/GET, body contains text, response body as output |
| `transform` | Built-in text transforms: regex replace, jq extract, template |

### HTTP Handler: Dependency Decision

Writing correct HTTP/1.1 from `std::net` is not worth the effort for localhost
calls to Ollama/LLM APIs (chunked encoding, timeouts, etc.). Use `ureq` — a
minimal sync HTTP client (~5 transitive deps, no async, no TLS needed for
localhost).

### Fan-out Example

```
"记录今天开会讨论了架构重构"
         │
    ┌────┴────┐────────────┐
    ↓         ↓            ↓
 inject    shell(log)   http(llm) → tts
 (type)    (write log)  (summarize → speak)
```

### New Files

- `src/pipeline/dag.rs` — topological sort + DAG execution
- `src/pipeline/handlers/pipe.rs` — stdin/stdout handler
- `src/pipeline/handlers/http.rs` — HTTP handler
- `src/pipeline/handlers/transform.rs` — text transform handler

---

## Dependencies

| Crate | Purpose | Phase | Justification |
|-------|---------|-------|---------------|
| `crossbeam` | Actor channels (bounded MPMC) + `select!` | 1 | De facto standard for sync channels |
| `serde_json` | IPC JSON-RPC serialization | 1 | Already transitively depended via serde |
| `ureq` | Sync HTTP client for localhost LLM calls | 4 | Minimal deps, no async, no TLS needed |

No new dependencies for Phase 2–3. TTS uses sherpa-onnx (existing). Wake word
reuses ASR.

## File Structure (Final)

```
src/
├── actor.rs              # Actor trait + Bus + shutdown       (Phase 1)
├── ipc.rs                # IpcActor + JSON-RPC                (Phase 1)
├── pipeline/
│   ├── mod.rs            # PipelineActor                      (Phase 1)
│   ├── stage.rs          # Stage + Condition + HandlerResult   (Phase 1, extended Phase 4)
│   ├── dag.rs            # DAG topo-sort + parallel execution  (Phase 4)
│   └── handlers/
│       ├── mod.rs        # Handler trait definition
│       ├── inject.rs     # wraps inject::inject_text()
│       ├── shell.rs      # migrated from router/handlers/
│       ├── pipe.rs       # stdin/stdout pipe                   (Phase 4)
│       ├── http.rs       # HTTP handler (ureq)                 (Phase 4)
│       └── transform.rs  # text transforms                     (Phase 4)
├── tts/
│   ├── mod.rs            # TtsActor + engine trait             (Phase 2)
│   └── sherpa.rs         # sherpa-onnx TTS implementation
├── wakeword/
│   ├── mod.rs            # WakewordActor                       (Phase 3)
│   └── detector.rs       # sliding window + ASR match
├── hotkey/               # unchanged, wrapped in Actor
├── audio/                # unchanged (AudioSource extraction for Phase 3)
├── asr/                  # unchanged
├── postprocess/          # unchanged
├── inject/               # unchanged, called by pipeline/handlers/inject.rs
├── config.rs             # extended with new sections
├── sound.rs              # unchanged
├── lib.rs                # extended with new modules
└── main.rs               # run_daemon → spawn actors + bus
```

## Backward Compatibility

- Existing `[router]` config auto-migrates to `[[pipeline.stages]]` with
  `condition = "starts_with:<trigger>"` at load time
- No pipeline config → defaults to single inject handler (current behavior)
- `[router]` section deprecated but parsed without error for one major version
- All new features (`[tts]`, `[wakeword]`, `[ipc]`) default to disabled or
  sensible defaults — zero-config first run still works

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ASR-based wake word CPU cost too high on weaker hardware | Medium | Users disable feature | Opt-in (default off), allow lighter model override, document CPU requirements |
| sherpa-onnx TTS voice quality insufficient | Medium | Poor UX | Engine trait allows swapping to piper later |
| DAG pipeline complexity vs. actual usage | Low | Over-engineering | Phase 4 is last; validate need with Phase 1-3 usage first |
| Actor message ordering edge cases | Low | Subtle bugs | Topic-based routing with static subscription table, extensive tests |
| Always-on mic privacy concerns | Medium | User trust | Opt-in only, clear documentation, prefer PipeWire passive source |
| WakewordActor + CoreActor dual ASR memory | Low | ~600MB total | Acceptable on 32GB; allow lighter model for wakeword |
| HTTP handler correctness (chunked responses) | Medium | Broken LLM integration | Use ureq instead of raw std::net |
| Thread explosion from wide DAGs + concurrent IPC | Low | Resource exhaustion | Configurable max_parallel_stages and max_concurrent_executions |
