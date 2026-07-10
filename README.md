# DSO — Deterministic Streaming Object Runtime

> **Any decision that can be made ahead of time must be made ahead of time.**

DSO is a **methodology for building computational systems** based on five principles:

1. **Determinism First** — every decision ahead of time
2. **Compile Globally** — plan before execution
3. **Verify Early** — verify before acting
4. **Execute Locally** — only affected objects
5. **Sleep By Default** — no cause = no execution

---

## Repository Structure

```
dso/
├── MANIFEST.md              # Philosophy: 17 principles of DSO
├── BRANCHLESS.md            # Branchless by Design
├── ENGINE_SPEC.md           # DSO Game Engine Core specification
├── proto/                   # Rust prototype: event-driven simulation
│   └── src/main.rs          #   1M objects, 99% sleep, compiled routes
└── ai/                      # DSO-AI Runtime: LLM inference engine
    ├── dso_runtime.cpp      #   C++ engine (zero-malloc, INT8, AVX2)
    ├── run.py               #   CLI wrapper
    ├── tok.py               #   BPE tokenizer
    └── quantize.py          #   INT8 quantization
```

---

## Prototype Features

The Rust prototype (`proto/`) demonstrates 6 DSO principles with real benchmarks:

| Feature | Principle | What it shows |
|---|---|---|
| Dependency Graph | **World Is A Dep Graph** | 3,100 producer→consumer edges compiled at init, no broadcast |
| Time as Events | **Time Is Also An Event** | Timed event queue, time skipping to next event |
| Deterministic Replay | **Determinism First** | 153 timer events replayed, exact match verified |
| Sleep By Default | **Nothing Executes Without Reason** | 1M objects, 99.98% never touched by CPU |
| Branchless Dispatch | **Compile Knowledge** | Function pointer table, no `match` in hot path |
| Resource Graph | **Every Resource Has An Owner** | Ownership tracking, transfer, consumption |

**Key metric:** `cargo run --release` → interactive CLI with `status`, `fire`, `advance`, `replay` commands.

---

## Key Documents

| Document | Summary |
|---|---|
| [`MANIFEST.md`](MANIFEST.md) | 17 principles — from determinism to the universal object runtime |
| [`BRANCHLESS.md`](BRANCHLESS.md) | Why branching is a symptom of a decision made too late |
| [`ENGINE_SPEC.md`](ENGINE_SPEC.md) | DSO Game Engine Core architecture: objects, contracts, graphs |
| [`proto/`](proto/) | Rust prototype: compiled routes, resource graph, deterministic event log |
| [`ai/`](ai/) | C++ LLM inference engine: streaming from disk, INT8, static arenas |

---

## Running the Prototype

```bash
cd proto && cargo run --release
```

---

> **Important note.** All of this is an architectural hypothesis, not a proven theory. Working prototypes and benchmarks will confirm or refute it.
