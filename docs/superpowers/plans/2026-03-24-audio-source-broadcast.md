# AudioSource Broadcast Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract audio capture into a shared AudioSource actor that broadcasts PCM chunks to CoreActor and WakewordActor via crossbeam channels, enabling simultaneous hotkey-triggered recording and continuous wake word detection on a single cpal input stream.

**Architecture:** AudioSource owns the single cpal input stream and broadcasts 10ms chunks (160 samples @ 16kHz) to subscribers via `Vec<Sender<Arc<[f32]>>>`. CoreActor switches between drain-and-discard (Idle) and drain-and-collect (Recording). WakewordActor collects a sliding window and runs ASR every `stride_seconds`. MuteInput/UnmuteInput messages coordinate mutual exclusion.

**Tech Stack:** Rust 2021, crossbeam (channels), cpal (audio capture), sherpa-onnx (ASR)

**Spec:** Design agreed in conversation — no separate spec doc.

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `src/audio_source.rs` | `AudioSourceActor` — owns cpal stream, broadcasts chunks, calibrates silence |

### Modified files

| File | Change |
|------|--------|
| `src/core_actor.rs` | Remove AudioPipeline ownership. Receive audio chunks from channel. Collect into buffer when Recording, discard when Idle/Muted. |
| `src/wakeword/mod.rs` | Receive audio chunks from channel. Sliding window ASR. Detect wake phrases. Emit StartListening or PipelineInput. |
| `src/main.rs` | Create AudioSource, pass audio receivers to CoreActor and WakewordActor constructors. |
| `src/lib.rs` | Add `pub mod audio_source;` |
| `src/audio/mod.rs` | `calibrate_silence` signature change — accept `&Receiver<Arc<[f32]>>` instead of `&mut AudioPipeline`. Keep `peak_rms`, `NoiseTracker`, `compute_rms` unchanged. |

### Unchanged files

| File | Reason |
|------|--------|
| `src/audio/recorder.rs` | AudioSource will reuse the same cpal patterns but inline them (recorder.rs stays for potential future use or can be cleaned up later) |
| `src/audio/denoise.rs` | Still called by CoreActor on collected samples |
| `src/hotkey/mod.rs` | No change — sends StartListening/StopListening via Bus as before |
| `src/actor.rs` | No change — AudioSource uses raw channels, not Actor trait (no inbox needed) |

---

## Task 1: Create AudioSourceActor

**Files:**
- Create: `src/audio_source.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_chunk_is_arc_slice() {
        let chunk: AudioChunk = Arc::from(vec![0.0f32; 160].into_boxed_slice());
        assert_eq!(chunk.len(), 160);
        // Arc clone is cheap — same underlying data.
        let clone = Arc::clone(&chunk);
        assert_eq!(clone.len(), 160);
    }
}
```

- [ ] **Step 2: Implement AudioSourceActor**

```rust
//! AudioSource — owns the cpal input stream and broadcasts PCM chunks.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam::channel::Sender;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::config::AudioConfig;

/// A chunk of mono f32 PCM samples shared via Arc (zero-copy fan-out).
pub type AudioChunk = Arc<[f32]>;

/// Starts the cpal input stream and broadcasts chunks to all subscribers.
///
/// This function blocks the calling thread until `stop` is signalled.
/// It is meant to be spawned on a dedicated thread.
pub fn run_audio_source(
    config: &AudioConfig,
    subscribers: Vec<Sender<AudioChunk>>,
    stop: crossbeam::channel::Receiver<()>,
) -> Result<()> {
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
    let subs = subscribers;

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            let chunk: AudioChunk = Arc::from(mono.into_boxed_slice());

            // Broadcast to all subscribers. Remove none — subscribers
            // are expected to live for the lifetime of the source.
            for tx in &subs {
                // Non-blocking: if a subscriber is full, drop the chunk.
                let _ = tx.try_send(Arc::clone(&chunk));
            }
        },
        |err| log::error!("[audio_source] stream error: {err}"),
        None,
    )
    .context("failed to build input stream")?;

    stream.play().context("failed to start input stream")?;
    log::info!("[audio_source] streaming");

    // Block until stop signal.
    let _ = stop.recv();
    // Stream is dropped here, stopping capture.
    log::info!("[audio_source] stopped");
    Ok(())
}
```

- [ ] **Step 3: Add `pub mod audio_source;` to `src/lib.rs`**

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/audio_source.rs src/lib.rs
git commit -m "feat(audio): add AudioSource broadcast for shared mic access"
```

---

## Task 2: Update calibrate_silence to use AudioChunk receiver

**Files:**
- Modify: `src/audio/mod.rs`

- [ ] **Step 1: Add channel-based calibrate function**

Add a new function alongside the existing one (keep old for backward compat with `--test-audio`):

```rust
/// Calibrate silence threshold from an audio broadcast channel.
pub fn calibrate_silence_from_channel(
    rx: &crossbeam::channel::Receiver<crate::audio_source::AudioChunk>,
    sample_rate: u32,
    floor: f32,
) -> f32 {
    log::info!("calibrating silence threshold (1s ambient sample)…");

    let target_samples = sample_rate as usize; // 1 second
    let mut collected = Vec::with_capacity(target_samples);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);

    while collected.len() < target_samples {
        if std::time::Instant::now() > deadline {
            break;
        }
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(chunk) => collected.extend_from_slice(&chunk),
            Err(_) => continue,
        }
    }

    if collected.is_empty() {
        log::warn!("calibration got no samples — using floor {floor}");
        return floor;
    }

    let window_size = (sample_rate as usize) / 20; // 50ms
    let mut window_rms: Vec<f32> = collected
        .chunks(window_size)
        .filter(|w| w.len() == window_size)
        .map(|w| compute_rms(w))
        .collect();

    if window_rms.is_empty() {
        log::warn!("calibration too short for windowing — using floor {floor}");
        return floor;
    }

    window_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = window_rms[window_rms.len() / 2];
    let ceiling = 0.05_f32;
    let threshold = (median * 2.0).clamp(floor, ceiling);

    log::info!(
        "noise floor (median RMS): {median:.4}, threshold: {threshold:.4} \
         (floor: {floor}, ceiling: {ceiling})"
    );
    threshold
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add src/audio/mod.rs
git commit -m "feat(audio): add calibrate_silence_from_channel for broadcast source"
```

---

## Task 3: Rewrite CoreActor to receive audio from channel

**Files:**
- Modify: `src/core_actor.rs`

- [ ] **Step 1: Update CoreActor constructor**

Change `CoreActor::new` to accept an audio receiver:

```rust
pub struct CoreActor {
    config: Config,
    preload: bool,
    audio_rx: crossbeam::channel::Receiver<crate::audio_source::AudioChunk>,
}

impl CoreActor {
    #[must_use]
    pub fn new(
        config: Config,
        preload: bool,
        audio_rx: crossbeam::channel::Receiver<crate::audio_source::AudioChunk>,
    ) -> Self {
        Self { config, preload, audio_rx }
    }
}
```

- [ ] **Step 2: Rewrite the run() method**

Replace AudioPipeline with channel-based audio collection:

- Remove `AudioPipeline::new()` and `audio.start_recording()` / `audio.stop_recording()` calls
- Use `calibrate_silence_from_channel(&self.audio_rx, ...)` for initial calibration
- In Idle state: drain `audio_rx` with `try_recv` and discard, blocking-wait on `inbox` for StartListening
- In Recording state: use `crossbeam::select!` on both `inbox` and `audio_rx`:
  - `audio_rx` → append to `recording_buffer: Vec<f32>`
  - `inbox` → handle StopListening (finalize), CancelRecording (clear buffer + restart), etc.
- `finalize_recording` takes `recording_buffer` directly instead of calling `audio.stop_recording()`
- Apply denoise to collected buffer if `config.audio.denoise` is true
- Keep the same ASR, punctuation, postprocess logic

Key change in the main loop:

```rust
CoreState::Idle => {
    // Drain audio channel to prevent backpressure.
    while self.audio_rx.try_recv().is_ok() {}

    match inbox.recv() {
        Ok(Message::StartListening) => {
            beep_if(&self.config, sound::beep_start);
            recording_buffer.clear();
            recording_start = Some(Instant::now());
            state = CoreState::Recording;
        }
        Ok(Message::Shutdown) => break,
        _ => {}
    }
}
CoreState::Recording => {
    crossbeam::select! {
        recv(self.audio_rx) -> chunk => {
            if let Ok(chunk) = chunk {
                recording_buffer.extend_from_slice(&chunk);
            }
            // Check timeout.
            if let Some(start) = recording_start {
                if start.elapsed() >= max_record {
                    // ... force-stop logic ...
                }
            }
        }
        recv(inbox) -> msg => {
            match msg {
                Ok(Message::StopListening) => {
                    // finalize with recording_buffer
                }
                Ok(Message::CancelRecording) => {
                    recording_buffer.clear();
                    recording_start = Some(Instant::now());
                }
                // ... other messages ...
            }
        }
    }
}
```

- [ ] **Step 3: Update finalize_recording signature**

Change to accept `&[f32]` directly:

```rust
fn finalize_recording(
    samples: &[f32],
    denoise_enabled: bool,
    asr_engine: &mut Option<AsrEngine>,
    punctuator: &mut Option<sherpa_onnx::OfflinePunctuation>,
    config: &Config,
    outbox: &Sender<Message>,
    elapsed: f32,
    noise_tracker: &mut NoiseTracker,
)
```

Apply denoise inside if `denoise_enabled`. No AudioPipeline needed.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Run tests**

Run: `cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/core_actor.rs
git commit -m "refactor(core): receive audio from broadcast channel instead of AudioPipeline"
```

---

## Task 4: Wire WakewordActor to audio channel

**Files:**
- Modify: `src/wakeword/mod.rs`

- [ ] **Step 1: Update WakewordActor constructor**

```rust
pub struct WakewordActor {
    config: Config,
    audio_rx: crossbeam::channel::Receiver<crate::audio_source::AudioChunk>,
}

impl WakewordActor {
    #[must_use]
    pub fn new(
        config: Config,
        audio_rx: crossbeam::channel::Receiver<crate::audio_source::AudioChunk>,
    ) -> Self {
        Self { config, audio_rx }
    }
}
```

- [ ] **Step 2: Implement sliding window ASR detection loop**

Replace the sleep-based skeleton with real audio processing:

```rust
fn run(self, inbox: Receiver<Message>, outbox: Sender<Message>) {
    if !self.config.wakeword.enabled {
        log::info!("[wakeword] disabled, actor idle");
        // Still drain audio_rx to prevent backpressure.
        loop {
            crossbeam::select! {
                recv(inbox) -> msg => {
                    if matches!(msg, Ok(Message::Shutdown)) { break; }
                }
                recv(self.audio_rx) -> _ => {} // discard
            }
        }
        return;
    }

    let detector = WakewordDetector::new(self.config.wakeword.phrases.clone());
    let window_samples = (self.config.wakeword.window_seconds
        * self.config.audio.sample_rate as f64) as usize;
    let stride_samples = (self.config.wakeword.stride_seconds
        * self.config.audio.sample_rate as f64) as usize;

    // Init ASR engine.
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

    let mut window: Vec<f32> = Vec::with_capacity(window_samples);
    let mut muted = false;
    let mut samples_since_last_asr: usize = 0;

    loop {
        // Check control messages (non-blocking).
        while let Ok(msg) = inbox.try_recv() {
            match msg {
                Message::Shutdown => return,
                Message::MuteInput => { muted = true; window.clear(); }
                Message::UnmuteInput => { muted = false; }
                _ => {}
            }
        }

        // Read audio chunk.
        match self.audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                if muted {
                    continue;
                }
                window.extend_from_slice(&chunk);
                samples_since_last_asr += chunk.len();

                // Trim window to max size (sliding).
                if window.len() > window_samples {
                    let excess = window.len() - window_samples;
                    window.drain(..excess);
                }

                // Run ASR every stride_samples.
                if samples_since_last_asr >= stride_samples && window.len() >= window_samples {
                    samples_since_last_asr = 0;

                    match asr.transcribe(&window, self.config.audio.sample_rate) {
                        Ok(text) if !text.is_empty() => {
                            if let Some((phrase, remainder)) = detector.check(&text) {
                                log::info!("[wakeword] detected '{phrase}', remainder: {remainder:?}");

                                match self.config.wakeword.action {
                                    crate::config::WakewordAction::StartRecording => {
                                        outbox.send(Message::StartListening).ok();
                                    }
                                    crate::config::WakewordAction::PipelinePassthrough => {
                                        if !remainder.is_empty() {
                                            outbox.send(Message::PipelineInput {
                                                text: remainder.to_string(),
                                                metadata: crate::actor::Metadata {
                                                    source: "wakeword".to_string(),
                                                    timestamp: std::time::Instant::now(),
                                                },
                                            }).ok();
                                        }
                                    }
                                }
                                // Clear window after detection to avoid re-triggering.
                                window.clear();
                            }
                        }
                        Ok(_) => {} // empty transcript
                        Err(e) => log::debug!("[wakeword] ASR error: {e}"),
                    }
                }
            }
            Err(_) => {} // timeout, loop back to check inbox
        }
    }
}
```

- [ ] **Step 3: Update test**

Update the `wakeword_actor_name` test to pass a dummy receiver:

```rust
#[test]
fn wakeword_actor_name() {
    let (_tx, rx) = crossbeam::channel::bounded(1);
    let actor = WakewordActor::new(Config::default(), rx);
    assert_eq!(Actor::name(&actor), "wakeword");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Run tests**

Run: `cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/wakeword/mod.rs
git commit -m "feat(wakeword): implement sliding window ASR detection from audio broadcast"
```

---

## Task 5: Wire AudioSource into run_daemon()

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Create audio broadcast channels and spawn AudioSource**

In `run_daemon()`, before spawning actors:

```rust
// Audio broadcast channels.
let (core_audio_tx, core_audio_rx) =
    crossbeam::channel::bounded::<voicerouter::audio_source::AudioChunk>(256);
let (wakeword_audio_tx, wakeword_audio_rx) =
    crossbeam::channel::bounded::<voicerouter::audio_source::AudioChunk>(256);
let (audio_stop_tx, audio_stop_rx) = crossbeam::channel::bounded::<()>(1);

let audio_config = config.audio.clone();
let audio_subscribers = vec![core_audio_tx, wakeword_audio_tx];
let audio_handle = std::thread::Builder::new()
    .name("audio_source".into())
    .spawn(move || {
        if let Err(e) = voicerouter::audio_source::run_audio_source(
            &audio_config, audio_subscribers, audio_stop_rx,
        ) {
            log::error!("[audio_source] failed: {e:#}");
        }
    })?;
```

- [ ] **Step 2: Update CoreActor and WakewordActor construction**

```rust
let core_actor = CoreActor::new(config.clone(), preload, core_audio_rx);
let wakeword_actor = WakewordActor::new(config.clone(), wakeword_audio_rx);
```

- [ ] **Step 3: Send stop signal to AudioSource on shutdown**

In the Ctrl+C handler or after the join loop, send stop:

```rust
// After bus sends Shutdown:
audio_stop_tx.send(()).ok();
```

Add `audio_handle` to the join handles vec.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Run all tests**

Run: `cargo test`

- [ ] **Step 6: Build release and smoke test**

Run: `cargo build --release`
Start: `target/release/voicerouter --preload --verbose`
Expected: All actors start, audio_source streaming, hotkey recording works.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire AudioSource broadcast into run_daemon for shared mic access"
```

---

## Task 6: Cleanup and integration test

**Files:**
- Modify: `src/main.rs` (test-audio mode still uses AudioPipeline — keep it)

- [ ] **Step 1: Verify --test-audio still works**

`--test-audio` uses `AudioPipeline` directly (not actors). Confirm it still compiles and runs.

Run: `cargo build --release && target/release/voicerouter --test-audio`

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Manual smoke test with wakeword disabled**

Start daemon, press hotkey, speak, verify text injection works as before.

- [ ] **Step 4: Manual smoke test with wakeword enabled**

Enable wakeword in config, start daemon, say wake phrase, verify detection in logs.

- [ ] **Step 5: Commit any fixes**

- [ ] **Step 6: Update TODO.md**

Mark "Wakeword audio source" as done.

```bash
git add docs/plans/TODO.md
git commit -m "docs: mark wakeword audio source integration as done"
```
