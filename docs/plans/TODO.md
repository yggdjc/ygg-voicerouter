# voicerouter TODO

Features from Python ygg-voiceim not yet ported to Rust voicerouter.

## Pending — Continuous Listening (Jarvis mode)

Spec: `docs/superpowers/specs/2026-03-24-continuous-listening-design.md`
Plan: `docs/superpowers/plans/2026-03-24-continuous-listening.md`

- [ ] **VAD Actor** — Silero VAD speech segment detection from AudioSource broadcast
- [ ] **Speaker verification** — sherpa-onnx embedding + cosine similarity, `voicerouter enroll` CLI
- [ ] **Local intent filter** — rule-based Command/Ambient/Uncertain classification
- [ ] **LLM judge** — OpenAI-compatible API fallback for uncertain segments
- [ ] **Risk-graded execution** — low-risk silent, high-risk beep + hotkey confirm
- [ ] **ContinuousActor** — orchestrate VAD→Speaker→ASR→Intent→Execute pipeline
- [ ] **Model download** — silero-vad and 3dspeaker model download support

## Pending — Other

- [ ] **Interactive setup wizard** — select mic, hotkey, model interactively (Python setup is more detailed)
- [ ] **CT-Transformer disfluency detection** — replace rule-based filler removal with ct-transformer's built-in disfluency output (model already downloaded for punctuation restoration)
- [ ] **的/得/地 同音修正** — 中文 ASR 最常见的同音字错误，需上下文语法分析
- [ ] **IPC client cleanup** — disconnected clients accumulate until next bus event triggers retain()
- [ ] **DAG parallel execution** — topo sort done, execution is sequential; add crossbeam::scope for parallel siblings
- [ ] **parse_condition to lib** — move from main.rs to pipeline::stage for testability

## Done

- [x] Clipboard restore after paste injection (74b8151)
- [x] Model auto-download — `voicerouter download [model]` (3c275ed)
- [x] Voice Interaction Framework Phase 1 — actor infrastructure, pipeline, IPC
- [x] Voice Interaction Framework Phase 2 — TTS actor with cpal playback
- [x] Voice Interaction Framework Phase 3 — wakeword actor with prefix detection
- [x] Voice Interaction Framework Phase 4 — DAG orchestration, pipe/http/transform handlers
- [x] TTS sherpa-onnx integration — Kokoro v1.1 Chinese TTS via OfflineTts API
- [x] Wakeword audio source — AudioSource broadcast, CoreActor + WakewordActor share single cpal stream
- [x] Mode-dependent recording stop — wakeword uses silence auto-stop, hotkey uses 60s timeout only
- [x] Number formatting — comma separators for large numbers (1000000 → 1,000,000)
- [x] Wakeword prefix stripping — only for wakeword-triggered recordings
- [x] Post-inject cooldown — 2s cooldown prevents wakeword retrigger during inject
- [x] Silent window skip — wakeword ASR skips silent windows to prevent hallucination false wakes
