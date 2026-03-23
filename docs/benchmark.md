# ASR Model Benchmark

Comparison of offline ASR models tested with voicerouter on Linux (Intel i7-11700, 32GB RAM, CPU-only).

## Models Tested

| Model | Size (int8) | Params | Architecture | Languages |
|-------|------------|--------|--------------|-----------|
| Paraformer-zh | 223 MB | 220M | Non-autoregressive | zh, en |
| FunASR Nano | 751 MB | 0.8B | LLM decoder (Qwen3-0.6B) | zh, en, ja + 22 dialects |

## Accuracy (Published CER %)

| Benchmark | Paraformer-zh | FunASR Nano |
|-----------|--------------|-------------|
| AISHELL-1 | **1.68** | 1.80 |
| AISHELL-2 | **2.85** | 2.75 |
| WenetSpeech (net) | 6.74 | — |

## Runtime Performance (i7-11700, CPU)

| Metric | Paraformer-zh | FunASR Nano |
|--------|--------------|-------------|
| RAM (RSS) | ~635 MB | ~1,547 MB |
| Idle CPU | ~0% | ~8.7% |
| Model load time | ~1s | ~3s |
| Inference (short utterance) | <0.5s | ~1s |

## Practical Observations

### Paraformer-zh (current default)

- Reliable for short and medium utterances
- Stable output — no hallucination observed
- English output in lowercase, consistent casing
- Requires ct-punc (76 MB) for punctuation
- No built-in ITN

### FunASR Nano

- Built-in punctuation (partial — commas but inconsistent question marks)
- Supports multilingual (zh/en/ja) — but may misidentify language ("ええじはいへんわ" output for Chinese input)
- **LLM hallucination observed**: input "似有标点但是不尽理想" → output gibberish ("SHER 半夜通宵整夜工作 TSMB...")
- English casing unstable: full uppercase in mixed mode, lowercase in pure English mode
- 2.4x RAM usage vs Paraformer
- Persistent idle CPU usage (~8.7%)

## Recommendation

**Paraformer-zh** for production use. FunASR Nano's LLM-based decoder introduces hallucination risk that is unacceptable for a voice input method.

## Models Not Tested

| Model | Reason |
|-------|--------|
| SenseVoice-Small | Accuracy worse than Paraformer (~2.96 vs 1.68 CER on AISHELL-1) |
| SenseVoice-Large | Not open-source (paper only) |
| Qwen3-ASR-1.7B | No sherpa-onnx/ONNX support |
| FireRedASR2 | 799 MB, likely too slow on CPU for real-time input |
| Whisper-large-v3 | Poor Chinese accuracy (5.14% CER on AISHELL-1) |
