# DSO — Deterministic Streaming Object Runtime

> **Any decision that can be made ahead of time must be made ahead of time.**
> *At runtime, the system must not think — it must execute a pre-verified plan.*

DSO is a **methodology for building computational systems** where every decision is pushed to compile time. Objects sleep by default, never poll, and execute only when an explicit event reaches them through a pre-compiled dependency graph.

**Key result:** at 100K objects, DSO dispatches a timer event **5.7× faster** than equivalent ECS polling — and the gap grows with world size.

---

## Why DSO?

Modern engines poll. ECS iterates all entities every tick. Game loops check every object every frame. This is **O(N)** by design.

DSO flips the model: **nothing executes without a reason.** Events target specific objects through a pre-compiled dependency graph. The CPU only touches what changes.

| | ECS | DSO |
|---|---|---|
| Dispatch cost | O(total objects) | O(active objects) |
| Wake ratio | 100% polled every tick | 99.98% never wake |
| World 100K | 267µs per scan | 47µs per event dispatch |
| World 1M | ~2.6ms per scan | ~50µs per event dispatch |
| Time skip | N/A (must tick anyway) | O(1) — skip 1M ns in 201ns |

---

## Benchmark Suite

Run: `cd proto && cargo run --release -- --bench`

### 1. Scalability — DSO vs ECS at different world sizes

```
 Objects |  DSO 1 event |  ECS 1 scan |  Speedup
---------+--------------+-------------+---------
    1000 |   2.545µs    |   2.014µs   |   0.8×
   10000 |   6.793µs    |  26.550µs   |   3.9×
  100000 |  47.099µs    | 266.600µs   |   5.7×
```

DSO scales with *active* objects (constant in this test: 1 wakes). ECS scales with *total* objects. The advantage grows with N.

### 2. Event Burst — N simultaneous events

```
   Burst | DSO total | Per event
---------+-----------+----------
       1 |  42.46µs  | 42.460µs
      10 |  45.50µs  |  4.550µs
     100 |  60.39µs  |    603ns
    1000 | 168.70µs  |    168ns
```

Per-event cost drops with batching — dispatch table overhead is amortized.

### 3. Chain Depth — Linear event propagation

```
 Depth | Propagation | Per hop
-------+-------------+--------
     1 |    3.887µs  | 3.887µs
     3 |    2.014µs  |   671ns
     5 |    2.525µs  |   505ns
    10 |    1.944µs  |   194ns
    20 |    2.525µs  |   126ns
```

Chained propagation through the dependency graph costs ~126ns per hop at depth 20.

### 4. Active Ratio — % of objects that can ever wake

```
 % Active |       DSO |       ECS | Speedup
----------+-----------+-----------+--------
   0.001% |  41.39µs  | 267.00µs  |  6.5×
   0.010% |  42.22µs  | 235.64µs  |  5.6×
   0.100% |  41.99µs  | 356.63µs  |  8.5×
   1.000% |  42.51µs  | 258.27µs  |  6.1×
  10.000% |  42.13µs  | 259.13µs  |  6.2×
  50.000% |  41.20µs  | 240.91µs  |  5.8×
```

DSO is invariant to active ratio (only 1 object wakes). ECS is invariant too — it always scans everything.

### 5. Deterministic Replay

```
1000 objects: 50 events replayed in 161µs — ✓ MATCH
10000 objects: 50 events replayed in 333µs — ✓ MATCH
100000 objects: 50 events replayed in 7833µs — ✓ MATCH
```

Full event log with hash-chain verification. Replay produces identical state.

### 6. Time Skipping Efficiency

```
   Skip | Real time | Events | Woken
--------+-----------+--------+------
  1µs   |   3.597µs |      1 |     1
 10µs   |     420ns |      1 |     1
100µs   |     932ns |      1 |     1
  1ms   |     201ns |      1 |     1
```

Skipping empty time gaps is O(1) — no cost for advancing through empty time.

---

## Five Principles

| # | Principle | Meaning |
|---|-----------|---------|
| 1 | **Determinism First** | Every decision made ahead of time |
| 2 | **Compile Globally** | Plan before execution — dep graph at init |
| 3 | **Verify Early** | Verify before acting — preconditions checked |
| 4 | **Execute Locally** | Only affected objects touched |
| 5 | **Sleep By Default** | No cause = no execution |

Extended to 17 principles in the [manifesto](MANIFEST.md).

---

## Repository Structure

```
dso/
├── MANIFEST.md              # 17 principles of DSO philosophy
├── BRANCHLESS.md            # Branching is a decision made too late
├── ENGINE_SPEC.md           # DSO Game Engine Core specification
├── README.md                # This file
├── proto/                   # Rust prototype
│   ├── src/main.rs          # Full implementation + benchmark suite
│   └── README.md            # Proto details
└── ai/                      # DSO-AI Runtime (C++ LLM inference)
```

---

## Running

```bash
# Interactive demo — 1M objects, dependency graph, replay
cd proto && cargo run --release

# Benchmark suite — 6 benchmarks
cargo run --release -- --bench
```

---

## Documents

| Document | Summary |
|---|---|
| [MANIFEST.md](MANIFEST.md) | 17 principles — determinism through universal object runtime |
| [BRANCHLESS.md](BRANCHLESS.md) | Every branch is a decision made too late |
| [ENGINE_SPEC.md](ENGINE_SPEC.md) | DSO Game Engine Core architecture |
| [proto/](proto/) | Rust prototype with benchmarks |
| [ai/](ai/) | C++ LLM inference engine (INT8, AVX2, zero-malloc) |

---

> **Important note.** This is an architectural hypothesis supported by working prototypes and benchmarks. Reality will confirm or refute it.
