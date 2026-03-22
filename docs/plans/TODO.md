# voicerouter TODO

Features from Python ygg-voiceim not yet ported to Rust voicerouter.

## Pending

- [ ] **LLM polish** — optional OpenAI-compatible API to fix ASR homophones/grammar
- [ ] **Interactive setup wizard** — select mic, hotkey, model interactively (Python setup is more detailed)
- [ ] **Fine-grained ASR config** — language selection, beam_size for Whisper models
- [ ] **Model auto-download** — CLI command to download models instead of manual curl

## Done

- [x] Clipboard restore after paste injection (74b8151)
