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
enum Message {
    // Audio domain
    AudioSamples { samples: Vec<f32>, sample_rate: u32 },
    SilenceDetected,

    // ASR domain
    Transcript { text: String, raw: String, confidence: f32 },

    // Pipeline domain
    PipelineInput { text: String, metadata: Metadata },
    PipelineOutput { text: String, next: Option<String> },

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
```

### Actor Trait

```rust
trait Actor: Send + 'static {
    fn name(&self) -> &str;
    fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>);
}
```

Each actor owns its thread, communicates via `crossbeam::channel`. Internal
logic remains synchronous.

### Bus

Lightweight central message router, not a broadcast system:

```rust
struct Bus {
    routes: HashMap<&'static str, Sender<Message>>,
}
```

Routes are registered statically at startup. Each message type has a known
destination actor.

---

## Phase 1: Handler Pipeline + IPC

### Actor Breakdown

| Actor | Thread | Responsibility | Input | Output |
|-------|--------|----------------|-------|--------|
| `HotkeyActor` | 1 | evdev listen + state machine | — (self-driven) | `StartListening`, `StopListening` |
| `CoreActor` | 1 | Audio capture, silence detection, ASR, postprocess | `StartListening`, `StopListening`, `MuteInput`, `UnmuteInput` | `Transcript` |
| `PipelineActor` | 1 | Handler chain execution | `Transcript`, `PipelineInput` | `PipelineOutput`, `SpeakRequest` |
| `IpcActor` | 1 | Unix socket, JSON-RPC | External connections | `PipelineInput`; pushes `Transcript` events to subscribers |

### Handler Trait (revised)

```rust
trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, input: Message) -> Result<HandlerResult>;
}

enum HandlerResult {
    Forward(Message),   // pass to next handler in chain
    Emit(Message),      // send to bus (e.g. SpeakRequest)
    Done,               // terminate pipeline
}
```

### Pipeline Configuration

```toml
[[pipeline.stages]]
name = "default"
handler = "inject"
```

Stages execute in declaration order. Each stage receives the output of the
previous stage. This is a linear chain in Phase 1, upgraded to DAG in Phase 4.

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

### Migration from Router

Existing `[[router.rules]]` config is auto-converted to `[[pipeline.stages]]`
at load time. When no pipeline config exists, behavior is identical to current
(single inject handler). The `[router]` section is deprecated but still parsed.

### Code Changes

- `main.rs`: `run_daemon()` → create 4 actors + bus, spawn threads, await shutdown
- `hotkey/`: Internal logic unchanged, wrapped in `HotkeyActor::run()` loop
- `audio/`, `asr/`, `postprocess/`: No changes, called internally by `CoreActor`
- `router/`: Rewritten as `pipeline/`, Handler trait signature changed
- New: `actor.rs`, `bus.rs`, `ipc.rs`, `pipeline/`

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
pauses audio capture during TTS output to prevent ASR from transcribing its own
speech.

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
- Separate `AsrEngine` instance or shared with mutex

### Configuration

```toml
[wakeword]
enabled = false              # opt-in
phrases = ["小助手", "hey router"]
window_seconds = 2.0
stride_seconds = 1.0
action = "start_recording"   # start_recording | pipeline_passthrough
```

`pipeline_passthrough`: wake phrase + command in one utterance
("小助手帮我搜索XXX" → "帮我搜索XXX" enters pipeline directly).

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
    timeout: Duration,
}

enum Condition {
    Always,
    StartsWith(String),
    OutputEq(String),
    OutputContains(String),
}

struct PipelineExecution {
    stages: Vec<Stage>,
    results: HashMap<String, Message>,
}
```

1. Topological sort on stages
2. Execute stages with no dependencies first
3. On stage completion, evaluate downstream conditions, execute if satisfied
4. Sibling stages at same depth can run in parallel (`crossbeam::scope`)
5. Stage timeout → skip with warning log

### New Handler Types

| Handler | Purpose |
|---------|---------|
| `pipe` | stdin/stdout pipe: write text to subprocess stdin, read stdout as output |
| `http` | HTTP POST/GET, body contains text, response body as output |
| `transform` | Built-in text transforms: regex replace, jq extract, template |

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
| `crossbeam` | Actor channels (bounded MPMC) | 1 | De facto standard for sync channels, zero-cost over std when idle |
| `serde_json` | IPC JSON-RPC serialization | 1 | Already transitively depended via serde ecosystem |

No new dependencies for Phase 2–4. TTS uses sherpa-onnx (existing). Wake word
reuses ASR. HTTP handler uses `std::net` / minimal HTTP (no reqwest needed for
localhost calls).

## File Structure (Final)

```
src/
├── actor.rs              # Actor trait + Bus                  (Phase 1)
├── ipc.rs                # IpcActor + JSON-RPC                (Phase 1)
├── pipeline/
│   ├── mod.rs            # PipelineActor                      (Phase 1)
│   ├── stage.rs          # Stage + Condition + HandlerResult   (Phase 1, extended Phase 4)
│   ├── dag.rs            # DAG topo-sort + parallel execution  (Phase 4)
│   └── handlers/
│       ├── mod.rs
│       ├── inject.rs     # migrated from router/handlers/
│       ├── shell.rs      # migrated from router/handlers/
│       ├── pipe.rs       # stdin/stdout pipe                   (Phase 4)
│       ├── http.rs       # HTTP handler                        (Phase 4)
│       └── transform.rs  # text transforms                     (Phase 4)
├── tts/
│   ├── mod.rs            # TtsActor + engine trait             (Phase 2)
│   └── sherpa.rs         # sherpa-onnx TTS implementation
├── wakeword/
│   ├── mod.rs            # WakewordActor                       (Phase 3)
│   └── detector.rs       # sliding window + ASR match
├── hotkey/               # unchanged, wrapped in Actor
├── audio/                # unchanged
├── asr/                  # unchanged
├── postprocess/          # unchanged
├── inject/               # unchanged
├── config.rs             # extended with new sections
├── sound.rs              # unchanged
├── lib.rs                # extended with new modules
└── main.rs               # run_daemon → spawn actors + bus
```

## Backward Compatibility

- Existing `[router]` config auto-migrates to `[[pipeline.stages]]` at load time
- No pipeline config → defaults to single inject handler (current behavior)
- `[router]` section deprecated but parsed without error for one major version
- All new features (`[tts]`, `[wakeword]`, `[ipc]`) default to disabled or
  sensible defaults — zero-config first run still works

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ASR-based wake word CPU cost too high on weaker hardware | Medium | Users disable feature | Make it opt-in (default off), document CPU requirements |
| sherpa-onnx TTS voice quality insufficient | Medium | Poor UX | Engine trait allows swapping to piper later |
| DAG pipeline complexity vs. actual usage | Low | Over-engineering | Phase 4 is last; validate need with Phase 1-3 usage first |
| Actor message ordering edge cases | Low | Subtle bugs | Keep bus routing deterministic (no broadcast), extensive tests |
