# DSO-AI Runtime

High-performance, **zero-malloc-in-loop** C++ inference engine for transformer LLMs
(Qwen2/Qwen2.5), with **layer-by-layer disk streaming**, a static memory **arena**,
**INT8 weight-only PTQ**, AVX2 SIMD GEMM and async page-cache windowing.

This is a from-scratch runtime: weights are `mmap`'d from disk, exactly **one weight
matrix is resident in RAM at a time**, and the OS page-cache holds only a sliding
window of ~1–2 layers (the rest is evicted with `madvise(MADV_DONTNEED)` after use).
A background thread prefetches the next layer (`MADV_WILLNEED`) so I/O overlaps compute.

## Features

- **Streaming from disk** — no need to fit the whole model in RAM. RAM usage is
  independent of model size.
- **INT8 weight-only PTQ** (per-row symmetric scale) — ~2× smaller on disk, 4×
  smaller active weight buffer vs FP32, minimal accuracy loss.
- **AVX2 INT8 GEMM** with per-row dynamic activation quantization.
- **Static arenas** — no heap allocation inside the token generation loop.
- **OpenMP** row/column parallel GEMM with SMT-aware thread cap (`OMP_NUM_THREADS`).
- **Tiled lm_head** — logits computed by streaming vocab rows in blocks.

## Build

```bash
g++ -O3 -fopenmp -std=c++17 dso_runtime.cpp -o dso_runtime
```

## Get a model

```bash
pip install huggingface_hub
python3 -c "from huggingface_hub import snapshot_download; \
  snapshot_download('Qwen/Qwen2.5-0.5B-Instruct', local_dir='model', \
  allow_patterns=['config.json','tokenizer*.json','vocab.json','merges.txt','*.safetensors'])"
```

Then quantize to INT8 `.dso` (optional but recommended):

```bash
python3 quantize.py        # -> model/model.dso  (~2x smaller than safetensors)
```

## Run

`run.py` tokenizes the prompt (Qwen BPE, pure-Python), calls the engine, decodes output.

```bash
# INT8, cool (low CPU) — ~20 tok/s on a laptop
OMP_NUM_THREADS=4 DSO_MODEL=model/model.dso python3 run.py "The capital of France is" 64

# INT8, max speed
OMP_NUM_THREADS=16 DSO_MODEL=model/model.dso python3 run.py "Hello" 64

# BF16 reference (no .dso needed)
OMP_NUM_THREADS=16 python3 run.py "Hello" 64
```

### Direct engine usage

The engine reads prompt token ids (space-separated) from a file:

```bash
DSO_MODEL=model/model.dso ./dso_runtime prompt.tok 64
# set DSO_NOEOS=1 to disable early stop (benchmarking)
```

## Benchmarks (Qwen2.5-0.5B-Instruct, this machine — 16-core x86, 14 GB RAM)

| Mode | Threads | tok/s | CPU load |
|------|---------|-------|----------|
| BF16 (safetensors) | 16 | 4.7 | 100% |
| INT8 (scalar GEMM) | 16 | 18.8 | 100% |
| INT8 + AVX2 | 4 | 20.5 | ~25% |
| INT8 + AVX2 | 16 | 24.6 | 100% |

## Architecture notes

- `dso_runtime.cpp` — engine: mmap loader, arenas, RMSNorm / RoPE / GQA attention /
  SwiGLU, INT8 + AVX2 GEMM, async streaming worker.
- `tok.py` — Qwen2 ByteLevel BPE tokenizer (pure Python, no `transformers` needed).
- `quantize.py` — produces the custom `.dso` format (int8 weights + per-row fp32 scale).
- `run.py` — CLI wrapper (tokenize → engine → decode).

The `.dso` format: 8-byte header length + JSON header (per-tensor `kind`/`shape`/`off`/
`nbytes`) + concatenated blobs. INT8 tensors store `int8[q]` then `float32[scale]`,
dequantized as `value ≈ scale[j] * q[j]`.
