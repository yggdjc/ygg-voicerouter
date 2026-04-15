# Lazy Audio Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Release the microphone device when no actor needs audio, so the system mic indicator turns off between recordings.

**Architecture:** Add an `AudioControl` channel to `audio_source`. When no continuous-listening actor (wakeword, continuous) is enabled, audio_source starts in idle state with the device closed. Core and Conversation actors send `Open`/`Close` commands to acquire/release the mic on demand. A reference count tracks concurrent openers so overlapping requests work correctly. When all continuous listeners are enabled, audio_source falls back to always-on mode (current behavior, zero regression).

**Tech Stack:** Rust, crossbeam channels, cpal

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/audio_source.rs` | Modify | Add `AudioControl` enum, refactor to support lazy mode with open/close lifecycle |
| `src/core_actor.rs` | Modify | Accept `Option<Sender<AudioControl>>`, send Open/Close around recording |
| `src/conversation/mod.rs` | Modify | Accept `Option<Sender<AudioControl>>`, send Open/Close around conversation sessions |
| `src/main.rs` | Modify | Determine lazy vs always-on, create control channel, wire to actors |
| `src/config.rs` | Modify | Remove `KwsConfig` struct and `[kws]` section (phantom feature cleanup) |
| `~/.config/voicerouter/config.toml` | Modify | Remove `[kws]` section |
| `config.default.toml` | Modify | Remove `[kws]` section |

---

### Task 1: Add `AudioControl` enum and refactor `audio_source` to support lazy mode

**Files:**
- Modify: `src/audio_source.rs`

- [ ] **Step 1: Write tests for AudioControl and lazy behavior**

Add to the existing `#[cfg(test)] mod tests` in `audio_source.rs`:

```rust
#[test]
fn audio_control_debug_display() {
    // Verify enum variants exist and derive Debug.
    let open = AudioControl::Open;
    let close = AudioControl::Close;
    assert_eq!(format!("{open:?}"), "Open");
    assert_eq!(format!("{close:?}"), "Close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib audio_source::tests::audio_control_debug_display`
Expected: FAIL — `AudioControl` not defined.

- [ ] **Step 3: Implement AudioControl enum and refactor run_audio_source**

Rewrite `src/audio_source.rs` with the following changes:

1. Add the `AudioControl` enum at the top:

```rust
/// Commands for lazy audio device lifecycle.
#[derive(Debug, Clone, Copy)]
pub enum AudioControl {
    /// Request the device to open (ref-counted: first Open opens the device).
    Open,
    /// Release the device (ref-counted: last Close closes the device).
    Close,
}
```

2. Extract device config resolution into a helper (called once):

```rust
struct ResolvedDevice {
    device: cpal::Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
}

fn resolve_device(config: &AudioConfig) -> Result<ResolvedDevice> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device")?;

    let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());

    let mut configs: Vec<cpal::SupportedStreamConfigRange> = device
        .supported_input_configs()
        .context("failed to query input configs")?
        .collect();

    if configs.is_empty() {
        anyhow::bail!("device '{}' has no input configs", device_name);
    }

    configs.sort_by_key(|c| {
        let fmt = if c.sample_format() == cpal::SampleFormat::F32 { 0i32 } else { 1 };
        (fmt, -(c.max_sample_rate().0 as i32))
    });

    let best = &configs[0];
    let target = cpal::SampleRate(config.sample_rate);
    let actual = if target >= best.min_sample_rate() && target <= best.max_sample_rate() {
        target
    } else {
        cpal::SampleRate(target.0.clamp(best.min_sample_rate().0, best.max_sample_rate().0))
    };

    let supported = (*best).with_sample_rate(actual);
    let stream_config = cpal::StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    log::info!(
        "[audio_source] device '{}': {} ch @ {} Hz",
        device_name,
        stream_config.channels,
        stream_config.sample_rate.0,
    );

    let channels = stream_config.channels as usize;
    Ok(ResolvedDevice { device, stream_config, channels })
}
```

3. Extract stream building into a helper that takes `Arc<[Sender<AudioChunk>]>`:

```rust
fn build_stream(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
) -> Result<cpal::Stream> {
    let channels = resolved.channels;
    let subs = Arc::clone(subs);

    let stream = resolved.device.build_input_stream(
        &resolved.stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            let chunk: AudioChunk = Arc::from(mono.into_boxed_slice());

            for tx in subs.iter() {
                let _ = tx.try_send(Arc::clone(&chunk));
            }
        },
        |err| log::error!("[audio_source] stream error: {err}"),
        None,
    )
    .context("failed to build input stream")?;

    stream.play().context("failed to start input stream")?;
    Ok(stream)
}
```

4. Refactor `run_audio_source` to accept an optional control channel:

```rust
pub fn run_audio_source(
    config: &AudioConfig,
    subscribers: Vec<Sender<AudioChunk>>,
    stop: crossbeam::channel::Receiver<()>,
    control: Option<crossbeam::channel::Receiver<AudioControl>>,
) -> Result<()> {
    let resolved = resolve_device(config)?;
    let subs: Arc<[Sender<AudioChunk>]> = Arc::from(subscribers.into_boxed_slice());

    match control {
        None => run_always_on(&resolved, &subs, &stop),
        Some(ctrl) => run_lazy(&resolved, &subs, &stop, &ctrl),
    }
}

fn run_always_on(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
    stop: &crossbeam::channel::Receiver<()>,
) -> Result<()> {
    let _stream = build_stream(resolved, subs)?;
    log::info!("[audio_source] streaming");
    let _ = stop.recv();
    log::info!("[audio_source] stopped");
    Ok(())
}

fn run_lazy(
    resolved: &ResolvedDevice,
    subs: &Arc<[Sender<AudioChunk>]>,
    stop: &crossbeam::channel::Receiver<()>,
    ctrl: &crossbeam::channel::Receiver<AudioControl>,
) -> Result<()> {
    log::info!("[audio_source] lazy mode, device closed");
    let mut stream: Option<cpal::Stream> = None;
    let mut open_count: u32 = 0;

    loop {
        crossbeam::select! {
            recv(ctrl) -> msg => {
                match msg {
                    Ok(AudioControl::Open) => {
                        open_count += 1;
                        if open_count == 1 {
                            match build_stream(resolved, subs) {
                                Ok(s) => {
                                    log::info!("[audio_source] device opened");
                                    stream = Some(s);
                                }
                                Err(e) => {
                                    log::error!("[audio_source] failed to open: {e:#}");
                                    open_count = 0;
                                }
                            }
                        }
                    }
                    Ok(AudioControl::Close) => {
                        open_count = open_count.saturating_sub(1);
                        if open_count == 0 && stream.is_some() {
                            stream = None;
                            log::info!("[audio_source] device closed");
                        }
                    }
                    Err(_) => break,
                }
            }
            recv(stop) -> _ => break,
        }
    }

    log::info!("[audio_source] stopped");
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib audio_source`
Expected: PASS (both the new test and the existing `audio_chunk_is_arc_slice` test).

- [ ] **Step 5: Commit**

```bash
git add src/audio_source.rs
git commit -m "feat(audio_source): add lazy mode with AudioControl open/close lifecycle"
```

---

### Task 2: Wire lazy audio control into `main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add control channel creation and wiring**

In `run_daemon()`, after the audio broadcast channel setup (around line 274), add:

```rust
// Determine whether audio must be always-on (continuous listeners need it).
let needs_always_on = config.wakeword.enabled || config.continuous.enabled;
let (audio_ctl_tx, audio_ctl_rx) = if needs_always_on {
    (None, None)
} else {
    let (tx, rx) = crossbeam::channel::bounded::<voicerouter::audio_source::AudioControl>(4);
    (Some(tx), Some(rx))
};
```

- [ ] **Step 2: Pass control channel to `run_audio_source`**

Update the audio source thread spawn (around line 295-305):

```rust
let audio_handle = std::thread::Builder::new()
    .name("audio_source".into())
    .spawn(move || {
        if let Err(e) = voicerouter::audio_source::run_audio_source(
            &audio_config,
            audio_subscribers,
            audio_stop_rx,
            audio_ctl_rx,
        ) {
            log::error!("[audio_source] failed: {e:#}");
        }
    })?;
```

- [ ] **Step 3: Pass control sender clone to CoreActor**

Update CoreActor construction (around line 309):

```rust
let core_actor = CoreActor::new(config.clone(), preload, core_audio_rx, audio_ctl_tx.clone());
```

- [ ] **Step 4: Pass control sender clone to ConversationActor**

Update ConversationActor construction (around line 360):

```rust
let conversation_actor = ConversationActor::new(config.clone(), conv_audio_rx, audio_ctl_tx.clone());
```

- [ ] **Step 5: Build and verify compilation**

Run: `cargo build 2>&1`
Expected: Compiler errors for `CoreActor::new` and `ConversationActor::new` signatures — this is expected, will be fixed in Tasks 3 and 4.

- [ ] **Step 6: Commit (WIP, won't compile until Tasks 3-4)**

Do NOT commit yet. Proceed to Task 3.

---

### Task 3: Add audio control to `CoreActor`

**Files:**
- Modify: `src/core_actor.rs`

- [ ] **Step 1: Add `audio_ctl` field and update constructor**

Add the field to `CoreActor` struct:

```rust
pub struct CoreActor {
    config: Config,
    preload: bool,
    audio_rx: Receiver<AudioChunk>,
    audio_ctl: Option<Sender<crate::audio_source::AudioControl>>,
}
```

Update `new()`:

```rust
pub fn new(
    config: Config,
    preload: bool,
    audio_rx: Receiver<AudioChunk>,
    audio_ctl: Option<Sender<crate::audio_source::AudioControl>>,
) -> Self {
    Self { config, preload, audio_rx, audio_ctl }
}
```

- [ ] **Step 2: Add helper method for sending audio control**

Add a private helper inside the `impl Actor for CoreActor` block, before `run()`:

```rust
fn send_audio_ctl(ctl: &Option<Sender<crate::audio_source::AudioControl>>, cmd: crate::audio_source::AudioControl) {
    if let Some(ref tx) = ctl {
        tx.send(cmd).ok();
    }
}
```

(This is a free function, not a method, to avoid borrowing issues with `self`.)

- [ ] **Step 3: Send Open when recording starts**

In the `CoreState::Idle` arm, right before `log::info!("[core] recording started")` (inside the `StartListening` handler):

```rust
send_audio_ctl(&self.audio_ctl, crate::audio_source::AudioControl::Open);
```

- [ ] **Step 4: Send Close when returning to Idle**

There are three transitions back to Idle in the Recording state. Add `Close` before each `state = CoreState::Idle;`:

1. After auto-stop finalize (around the `outbox.send(Message::StopListening)` block):

```rust
send_audio_ctl(&self.audio_ctl, crate::audio_source::AudioControl::Close);
state = CoreState::Idle;
```

2. After `StopListening` message finalize:

```rust
send_audio_ctl(&self.audio_ctl, crate::audio_source::AudioControl::Close);
state = CoreState::Idle;
```

3. After `MuteInput` (transitions to Muted, not Idle — no Close here; device stays open while muted so unmute can resume quickly). When `UnmuteInput` transitions Muted → Idle, send Close:

```rust
Ok(Message::UnmuteInput) => {
    send_audio_ctl(&self.audio_ctl, crate::audio_source::AudioControl::Close);
    state = CoreState::Idle;
}
```

- [ ] **Step 5: Build and verify compilation**

Run: `cargo build 2>&1`
Expected: Compiler error for `ConversationActor::new` only — fixed in Task 4.

- [ ] **Step 6: Do NOT commit yet**

Proceed to Task 4.

---

### Task 4: Add audio control to `ConversationActor`

**Files:**
- Modify: `src/conversation/mod.rs`

- [ ] **Step 1: Add `audio_ctl` field and update constructor**

Update struct:

```rust
pub struct ConversationActor {
    config: Config,
    audio_rx: Receiver<AudioChunk>,
    audio_ctl: Option<Sender<crate::audio_source::AudioControl>>,
}
```

Update `new()`:

```rust
pub fn new(
    config: Config,
    audio_rx: Receiver<AudioChunk>,
    audio_ctl: Option<Sender<crate::audio_source::AudioControl>>,
) -> Self {
    Self { config, audio_rx, audio_ctl }
}
```

- [ ] **Step 2: Send Open when conversation starts**

In `start_session()`, add the `audio_ctl` parameter and send Open at the top of the function. Update the function signature:

```rust
fn start_session(
    state: &mut State,
    session: &mut Option<Session>,
    vad: &mut Option<VadDetector>,
    config: &Config,
    outbox: &Sender<Message>,
    overlay: &mut OverlayClient,
    audio_ctl: &Option<Sender<crate::audio_source::AudioControl>>,
) {
    if let Some(ref tx) = audio_ctl {
        tx.send(crate::audio_source::AudioControl::Open).ok();
    }
    // ... rest unchanged
}
```

Update the call site in `drain_control()` to pass `audio_ctl`.

- [ ] **Step 3: Send Close when conversation ends**

In `end_conversation()`, add the `audio_ctl` parameter and send Close:

```rust
fn end_conversation(
    outbox: &Sender<Message>,
    feedback: bool,
    state: &mut State,
    session: &mut Option<Session>,
    vad: &mut Option<VadDetector>,
    overlay: &mut OverlayClient,
    audio_ctl: &Option<Sender<crate::audio_source::AudioControl>>,
) {
    end_session(outbox, feedback, overlay);
    if let Some(ref tx) = audio_ctl {
        tx.send(crate::audio_source::AudioControl::Close).ok();
    }
    *state = State::Idle;
    *session = None;
    *vad = None;
}
```

Update all call sites of `end_conversation()` in `drain_control()` and `handle_thinking()` to pass `audio_ctl`.

- [ ] **Step 4: Thread `audio_ctl` through `drain_control`**

Add `audio_ctl: &Option<Sender<crate::audio_source::AudioControl>>` parameter to `drain_control()`. Update the function signature and pass it to `start_session()` and `end_conversation()` calls within.

Update the call site in the `run()` method to pass `&self.audio_ctl`.

- [ ] **Step 5: Update `finalize_transcript` end-conversation path**

In `finalize_transcript()`, the `end_conversation` call also needs `audio_ctl`. Add the parameter to `finalize_transcript` and thread it through. Update its call site in `handle_transcribing`.

- [ ] **Step 6: Update `handle_thinking` end-conversation path**

In `handle_thinking()`, the `end_conversation` call also needs `audio_ctl`. Add the parameter and update the call site in `run()`.

- [ ] **Step 7: Update the existing test**

The existing test `conversation_actor_name` constructs a `ConversationActor`. Update it:

```rust
#[test]
fn conversation_actor_name() {
    let (_tx, rx) = crossbeam::channel::bounded(1);
    let actor = ConversationActor::new(Config::default(), rx, None);
    assert_eq!(Actor::name(&actor), "conversation");
}
```

- [ ] **Step 8: Build and run all tests**

Run: `cargo build 2>&1 && cargo test 2>&1`
Expected: Compiles. All 347+ tests pass.

- [ ] **Step 9: Commit Tasks 2-4 together**

```bash
git add src/audio_source.rs src/main.rs src/core_actor.rs src/conversation/mod.rs
git commit -m "feat(audio): wire lazy audio control through core and conversation actors

CoreActor sends Open on StartListening, Close on Idle/UnmuteInput.
ConversationActor sends Open on StartConversation, Close on EndConversation.
main.rs creates the control channel when wakeword and continuous are both disabled."
```

---

### Task 5: Remove phantom `[kws]` config

**Files:**
- Modify: `src/config.rs`
- Modify: `config.default.toml`
- Modify: `~/.config/voicerouter/config.toml`

- [ ] **Step 1: Check if KwsConfig exists in config.rs**

Search for `Kws` or `kws` in `src/config.rs`. If a `KwsConfig` struct or `kws` field exists on `Config`, remove it. If it doesn't exist (TOML parser ignores unknown keys), skip to step 3.

- [ ] **Step 2: Remove KwsConfig from config.rs (if found)**

Remove the struct definition and the field from `Config`. Run `cargo build` to verify no code references it.

- [ ] **Step 3: Remove `[kws]` section from config.default.toml**

Remove these lines from `config.default.toml`:

```toml
[kws]
enabled = false
model_dir = "~/.cache/voicerouter/models/kws"
keyword = "一二一二"
handler = "inject"
punct_mode = "keep"
```

- [ ] **Step 4: Remove `[kws]` section from user config**

Remove these lines from `~/.config/voicerouter/config.toml`:

```toml
[kws]
enabled = false
model_dir = "~/.cache/voicerouter/models/kws"
keyword = "一二一二"
handler = "inject"
punct_mode = "keep"
```

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs config.default.toml
git commit -m "chore(config): remove phantom [kws] section (no code implements it)"
```

---

### Task 6: Integration test — build, restart service, verify logs

- [ ] **Step 1: Release build**

Run: `cargo build --release 2>&1`
Expected: Compiles with no warnings relevant to changed code.

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass (347+).

- [ ] **Step 3: Restart service and verify lazy mode in logs**

```bash
systemctl --user restart voicerouter
sleep 2
journalctl --user -u voicerouter --since "30 sec ago" --no-pager | head -20
```

Expected log lines:
- `[audio_source] device '...': 1 ch @ 16000 Hz` (device config resolved)
- `[audio_source] lazy mode, device closed` (lazy mode active, mic NOT open)
- `[wakeword] disabled, actor idle`
- `voicerouter ready — all actors running`
- No `[audio_source] streaming` line (that only appears in always-on mode)

- [ ] **Step 4: Test hotkey recording works**

Press the configured hotkey (Right Alt), speak briefly, release.

Expected log lines:
- `[audio_source] device opened`
- `[core] recording started`
- `[core] recording stopped`
- `[core] transcribed: "..."`
- `[audio_source] device closed`

Verify the system mic indicator turns ON during recording and OFF after.

- [ ] **Step 5: Commit (if any fixes needed)**

If Task 6 revealed issues, fix and commit. Otherwise, done.
