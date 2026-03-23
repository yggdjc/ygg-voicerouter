# voicerouter TODO

Features from Python ygg-voiceim not yet ported to Rust voicerouter.

## Pending

- [ ] **Interactive setup wizard** — select mic, hotkey, model interactively (Python setup is more detailed)
- [ ] **CT-Transformer disfluency detection** — replace rule-based filler removal with ct-transformer's built-in disfluency output (model already downloaded for punctuation restoration)
- [ ] **的/得/地 同音修正** — 中文 ASR 最常见的同音字错误，需上下文语法分析

## Voice Interaction Framework — needs real-world testing

- [x] **TTS sherpa-onnx integration** — Kokoro v1.1 Chinese TTS integrated via OfflineTts API
- [x] **Wakeword audio source** — AudioSource broadcast, CoreActor + WakewordActor share single cpal stream
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
