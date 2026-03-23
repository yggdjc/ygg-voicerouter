# voicerouter TODO

Features from Python ygg-voiceim not yet ported to Rust voicerouter.

## Pending

- [ ] **Interactive setup wizard** — select mic, hotkey, model interactively (Python setup is more detailed)
- [ ] **CT-Transformer disfluency detection** — replace rule-based filler removal with ct-transformer's built-in disfluency output (model already downloaded for punctuation restoration)
- [x] **Model auto-download** — `voicerouter download [model]` (3c275ed)
- [ ] **的/得/地 同音修正** — 中文 ASR 最常见的同音字错误，需上下文语法分析

## Done

- [x] Clipboard restore after paste injection (74b8151)
