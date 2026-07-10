# DSO Prototype v4 — Dependency Graph · Time as Events · Deterministic Replay

Minimal demonstration of DSO philosophy in Rust.

## Architecture

```
╔══════════════════ Compile Time ═══════════════════╗
║  Contracts → Dependency Graph (producer→consumer) ║
║           → Resource Graph (ownership)            ║
╚════════════════════════════════════════════════════╝
╔══════════════════ Runtime ════════════════════════╗
║  Timed Event Queue → Advance to next event time  ║
║                    → Follow dep graph edges       ║
║                    → Branchless dispatch (table)  ║
║                    → Log event + hash-chain       ║
║                    → Propagate spawned events     ║
╚════════════════════════════════════════════════════╝
```

## Key Results

```
1M objects, 175 active types, 999,825 always-sleeping
3,100 dependency edges compiled at init

Timer(0) → 1 object woken (chain: Tree → 20 factories → 5 cities)
Time skip: 5000ns jump, 152 woken
Deterministic replay: 153 timer events, verified exact match
99.98% of objects never woke up
```

## DSO Principles Demonstrated

| # | Principle | Implementation |
|---|---|---|
| 3 | **Global Planning** | Dependency graph compiled at init, not built at runtime |
| 9 | **World Is A Dependency Graph** | Producer→consumer edges, not broadcast |
| 15 | **Time Is Also An Event** | Timed event queue, time skipping to next event |
| 1 | **Determinism First** | Full event log with hash-chain, replay verification |
| 7 | **Nothing Executes Without Reason** | Objects wake only on events, never polled |
| 6 | **Sleep By Default** | 99.98% of objects never touched by CPU |

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

## Running

```bash
cargo run --release
```
