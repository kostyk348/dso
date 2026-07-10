# DSO Prototype v4 — Dependency Graph · Time as Events · Deterministic Replay

Rust implementation of the DSO philosophy with a full benchmark suite.

## Architecture

```
╔══════════════════ Compile Time ═══════════════════╗
║  Contracts → Dependency Graph (producer→consumer) ║
║           → Resource Graph (ownership)            ║
║           → Dispatch Table (branchless)           ║
╚════════════════════════════════════════════════════╝
╔══════════════════ Runtime ════════════════════════╗
║  Timed Event Queue → Advance to next event time  ║
║                    → Follow dep graph edges       ║
║                    → Branchless dispatch (table)  ║
║                    → Log event + hash-chain       ║
║                    → Propagate spawned events     ║
╚════════════════════════════════════════════════════╝
```

## Benchmarks

Run with `cargo run --release -- --bench`:

### Scalability

| Objects | DSO 1 event | ECS 1 scan | Speedup |
|---------|-------------|------------|---------|
| 1,000 | 2.5µs | 2.0µs | 0.8× |
| 10,000 | 6.8µs | 26.6µs | 3.9× |
| 100,000 | 47.1µs | 266.6µs | **5.7×** |

### Event burst

| Burst | Total | Per event |
|-------|-------|-----------|
| 1 | 42.5µs | 42.5µs |
| 10 | 45.5µs | 4.6µs |
| 100 | 60.4µs | 603ns |
| 1,000 | 168.7µs | **168ns** |

### Chain propagation

| Depth | Total | Per hop |
|-------|-------|---------|
| 1 | 3.9µs | 3.9µs |
| 20 | 2.5µs | **126ns** |

### Active ratio invariance

DSO: ~42µs regardless of active ratio (only woken objects touched).
ECS: ~250µs regardless (scans everything every time).
Speedup: **5–8× across all ratios.**

### Time skipping

Skipping 1ms of empty time: **201ns real time. O(1).**

### Deterministic replay

50 timer events, exact match verified at 1K, 10K, and 100K object scales.

## Interactive CLI

```
status          — world state
stats           — system stats
time            — current time
fire Timer(0)   — fire a timer event
advance 1000    — advance 1000ns
replay          — replay event log
quit            — exit
```

## DSO Principles Demonstrated

| # | Principle | Implementation |
|---|-----------|----------------|
| 1 | **Determinism First** | Full event log with hash-chain, replay verification |
| 3 | **Global Planning** | Dependency graph compiled at init, not built at runtime |
| 6 | **Sleep By Default** | 99.98% of objects never touched by CPU |
| 7 | **Nothing Executes Without Reason** | Objects wake only on events, never polled |
| 9 | **World Is A Dependency Graph** | Producer→consumer edges, not broadcast |
| 15 | **Time Is Also An Event** | Timed event queue, time skipping to next event |

## Running

```bash
# Interactive demo
cargo run --release

# Benchmarks
cargo run --release -- --bench
```
