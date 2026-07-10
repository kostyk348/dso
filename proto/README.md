# DSO Prototype: Event-Driven Simulation

Minimal demonstration of DSO philosophy in Rust.

## Key Results

### 1M objects, 99.99% never woke up

```
─── 1. Single timer event ───
  DSO:    121 objects woken in 15µs
  ECS:    visited 1M objects in 385µs    (25× slower)

─── 2. 150 timer events ───
  DSO:    3450 objects woken in 154µs
  ECS:    visited 1M objects in 385µs    (2.5× per event)

─── 3. Resource chain ───
  DSO:    360 objects woken in 27µs (3 Wood events → Factory → City)
```

## What It Demonstrates

- **Sleep By Default** — 999,974 objects never woke up (99.99%)
- **Nothing Executes Without Reason** — only events wake objects
- **World Is A Dependency Graph** — Tree→Factory→City chain propagation
- **Compile Knowledge** — event routes compiled into flat array at init (no HashMap in hot path)
- **Branchless by Design** — dispatch through function pointer table (no `match` in hot path)
- **Every Resource Has An Owner** — Resource Graph tracks ownership, transfers, consumption
- **Determinism First** — append-only event log with hash-chain for replay verification

## Architecture v3

```
Compilation phase:
  Contracts → Compiled Routes (flat Vec<Vec<ObjectId>>)
           → Resource Graph (ownership tracking)

Runtime:
  Event → Flat route lookup → Object(Wake → Execute via table → Sleep)
                              │
                              ▼
                         New Events → Propagate
                              │
                              ▼
                         Event Log (append-only, hash-chain)
```

## Running

```bash
cargo run --release
```
