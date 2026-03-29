# ASR Model Benchmark

Comparison of offline ASR models tested with voicerouter on Linux.

**Hardware:** Intel i7-11700 (8 cores), 32 GB RAM, NVIDIA GeForce RTX 3070 8 GB VRAM.

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

## Runtime Performance — CPU vs GPU

### Paraformer-zh (current default)

| Metric | CPU only | GPU (CUDA) |
|--------|---------|------------|
| RAM (RSS) | ~1.2 GB | ~1.5 GB |
| VRAM | 0 | ~630 MiB |
| Idle CPU | ~3% | ~7% (includes CUDA event handler) |
| Model load time | ~1s | ~1.5s |
| Short utterance (2s) | ~400-500ms | ~170-350ms |
| Long utterance (6s) | ~800-1000ms | ~400-650ms |
| End-to-end (record stop → inject) | ~700ms | ~220-400ms |

### FunASR Nano (CPU only, not recommended)

| Metric | Value |
|--------|-------|
| RAM (RSS) | ~1,547 MB |
| Idle CPU | ~8.7% |
| Model load time | ~3s |
| Inference (short utterance) | ~1s |

## GPU Resource Breakdown (Paraformer + ct-punc + Kokoro TTS)

| Component | Provider | VRAM | Idle CPU |
|-----------|----------|------|----------|
| Core ASR (Paraformer) | CUDA | ~400 MiB | 3.0% |
| Wakeword ASR (Paraformer) | CPU | 0 | 2.0% |
| Punctuation (ct-transformer) | CUDA | ~100 MiB | 0% |
| TTS (Kokoro v1.1) | CUDA | ~130 MiB | 0% |
| CUDA runtime overhead | — | ~100 MiB | 1.1% |
| Hotkey / IPC / Audio I/O | CPU | 0 | 1.0% |
| **Total** | | **~630 MiB** | **~7.3%** |

Note: Wakeword ASR forced to CPU due to CUDA Paraformer tensor shape bug with short (2s) sliding windows.

## Practical Observations

### Paraformer-zh (current default)

- Reliable for short and medium utterances
- Stable output — no hallucination observed
- English output in lowercase, consistent casing
- Requires ct-punc (76 MB) for punctuation
- No built-in ITN
- GPU acceleration: ~2x speedup on RTX 3070

### FunASR Nano

- Built-in punctuation (partial — commas but inconsistent question marks)
- Supports multilingual (zh/en/ja) — but may misidentify language ("ええじはいへんわ" output for Chinese input)
- **LLM hallucination observed**: input "似有标点但是不尽理想" → output gibberish ("SHER 半夜通宵整夜工作 TSMB...")
- English casing unstable: full uppercase in mixed mode, lowercase in pure English mode
- 2.4x RAM usage vs Paraformer
- Persistent idle CPU usage (~8.7%)

## Recommendation

**Paraformer-zh with CUDA** for production use. GPU acceleration cuts end-to-end latency by ~50% with minimal VRAM cost (630 MiB / 8 GB). FunASR Nano's LLM-based decoder introduces hallucination risk that is unacceptable for a voice input method.

## LLM Benchmark (Conversation Mode)

**Model:** qwen2.5:7b (Q4_K_M, 4.6 GB) via local Ollama
**VRAM:** ~4.9 GB (fully GPU-offloaded)
**Context:** 4096 tokens

### Response Latency

| Prompt | Tokens | Latency | Notes |
|--------|--------|---------|-------|
| 今天天气怎么样 | 51 | ~2.4s | Cold (first request after idle) |
| 说个笑话 | 54 | ~0.35s | Warm |
| 什么是量子计算 | 70 | ~0.63s | Warm |
| 推荐一本书 | 56 | ~0.38s | Warm |

Cold start adds ~2s for KV cache initialization. Warm requests complete in 0.3–0.7s for short replies (2 sentences).

### Resource Usage (Ollama + voicerouter combined)

| Component | VRAM | RAM |
|-----------|------|-----|
| Ollama (qwen2.5:7b) | ~4.9 GB | ~500 MB |
| voicerouter (Paraformer + TTS + ct-punc) | ~630 MiB | ~1.5 GB |
| **Total** | **~5.5 GB / 8 GB** | **~2.0 GB** |

### Model Comparison (tested on this hardware)

| Model | Quant | VRAM | Warm latency | Quality |
|-------|-------|------|-------------|---------|
| qwen2.5:7b | Q4_K_M | 4.9 GB | 0.3–0.7s | Good — concise, follows system prompt |
| qwen3.5:4b | Q4_K_M | ~3 GB | 11–30s | Poor — thinking model, too slow for voice |

**Recommendation:** qwen2.5:7b for voice conversation. Non-thinking models are essential for low-latency voice interaction. Thinking models (qwen3.5) add 10–30s overhead that is unacceptable for real-time conversation.

---

## Models Not Tested

| Model | Reason |
|-------|--------|
| SenseVoice-Small | Accuracy worse than Paraformer (~2.96 vs 1.68 CER on AISHELL-1) |
| SenseVoice-Large | Not open-source (paper only) |
| Qwen3-ASR-1.7B | No sherpa-onnx/ONNX support |
| FireRedASR2 | 799 MB, likely too slow on CPU for real-time input |
| Whisper-large-v3 | Poor Chinese accuracy (5.14% CER on AISHELL-1) |
