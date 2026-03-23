# voicerouter TODO

Features from Python ygg-voiceim not yet ported to Rust voicerouter.

## Pending

- [ ] **Interactive setup wizard** — select mic, hotkey, model interactively (Python setup is more detailed)
- [ ] **CT-Transformer disfluency detection** — replace rule-based filler removal with ct-transformer's built-in disfluency output (model already downloaded for punctuation restoration)
- [ ] **Model auto-download** — CLI command to download models instead of manual curl

## Done

- [x] Clipboard restore after paste injection (74b8151)
