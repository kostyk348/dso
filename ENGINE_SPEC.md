# DSO Game Engine Core — Specification v0.1

> *A deterministic event-driven game runtime.*

---

## 1. Concept

DSO Game Engine Core is not a game engine in the classical sense. It is an **Object Runtime** where:

- every object sleeps by default
- an object wakes only when an event occurs
- execution is local (no global `Update()`)
- all world knowledge is compiled ahead of time (graphs, contracts, routes)
- rendering is an independent consumer of state, not part of game logic

### Comparison with ECS

| Aspect | ECS | DSO |
|---|---|---|
| Unit | Entity (int) | Object (int + type + contract) |
| Logic | `System::Update()` per entity | `Wake → Execute → Sleep` |
| Activity | All entities, always | Only affected objects |
| Relationships | Components (data) | Contracts + dependencies |
| Planning | Runtime (query filtering) | Compile-time (graph) |
| Determinism | Optional | Primary |

---

## 2. Object Model

### 2.1. Object Definition

```rust
struct Object {
    id: u64,
    type_id: TypeId,
    state: State,           // Sleep | Active | Waiting | Terminated | Error
    contracts: Vec<Contract>,
    owned_resources: Vec<ResourceId>,
    dependencies: Vec<Dependency>,
    actions: Vec<Action>,
    events: Vec<EventHandler>,
}
```

### 2.2. Contract

A contract is a behavioral declaration: "what I need, what I give, what I react to."

```rust
struct Contract {
    needs: Vec<ResourceSlot>,     // Iron, Wood, ...
    produces: Vec<ResourceSlot>,  // Steel, Damage, ...
    wakes_on: Vec<EventPattern>,  // IronChanged, PowerChanged
}
```

The contract is the primary source of behavior. Code is merely the implementation.

### 2.3. States

```
  ┌──────────┐
  │  Sleep   │ ──── event ────▶ ┌──────────┐
  └──────────┘                   │  Active  │
         ▲                       └────┬─────┘
         │                             │
         │                             ▼
         │                      ┌──────────────┐
         │                      │  Execution    │
         │                      └──────┬───────┘
         │                             │
         │                   ┌─────────┴──────────┐
         │                   ▼                    ▼
         │            ┌──────────┐        ┌──────────┐
         │            │  Sleep   │        │ Waiting  │ ──► (awaits resource)
         │            └──────────┘        └──────────┘
         │
    terminate
         │
         ▼
   ┌──────────┐
   │ Terminated│
   └──────────┘
```

---

## 3. Event System

### 3.1. Events

An event is the sole reason for waking. Events have a type, source, target, and payload.

```rust
struct Event {
    id: u64,
    source: ObjectId,
    target: Option<ObjectId>,   // None = broadcast
    event_type: EventType,
    payload: Vec<u8>,
    timestamp: u64,
    priority: i8,
}
```

### 3.2. Propagation

Events propagate through the **Event Graph** — a compiled route map.

```
  TreeFelled ──▶ Wood (depends on Tree)
                      │
                      ▼
               Factory (depends on Wood)
                      │
                      ▼
                Steel (depends on Factory)
```

The graph is built at compile time. Runtime does not compute routes — it only follows them.

### 3.3. Event Queue

There is no global queue. Each object has an inbound channel. Events are delivered directly from source to recipient via the graph.

```
  Source ──event──▶ Object A ──event──▶ Object B
                        │
                        └──event──▶ Object C
```

---

## 4. Lifecycle

### 4.1. Wake → Execute → Sleep

```
  [Sleep] ──event──▶ [Active] ──execute──▶ [Sleep]
                        │                     ▲
                        ▼                     │
                   [Verification]──────────────┘
                        │ (fallback)
                        ▼
                    [Error/Recovery]
```

1. **Wake** — object received an event, contract is checked (required resources available?)
2. **Verification** — contract verified before execution
3. **Execute** — actions are performed
4. **Sleep** — object sleeps until the next event
5. **Error** — if the contract is violated, a fallback is activated

### 4.2. Time

Time is a regular event. The system does not tick. If the next timer is in 5 minutes, the system pauses for 5 minutes.

```rust
struct TimerEvent {
    target: ObjectId,
    at: u64,              // timestamp
    repeat: Option<u64>,  // interval, if periodic
}
```

No `Update()` — only `TimerExpired`.

---

## 5. Graph Compilation

### 5.1. Build Phase

Before execution (at initialization or level load), the following graphs are built:

1. **Object Graph** — all objects and their types
2. **Dependency Graph** — who depends on whom (directed edges)
3. **Event Graph** — where events of each type are routed
4. **Resource Graph** — who owns which resources
5. **Lifetime Graph** — creation and destruction order

### 5.2. Format

Graphs are compiled into flat tables for zero-cost runtime access:

```rust
struct CompiledGraph {
    objects: Vec<CompiledObject>,
    event_routes: Vec<(EventType, ObjectId)>,   // lookup table
    dependents: Vec<(ObjectId, ObjectId)>,      // who watches whom
    resource_owners: Vec<(ResourceId, ObjectId)>,
}
```

---

## 6. Resource Management

### 6.1. Ownership

Every resource has exactly one owner.

```rust
struct Resource {
    id: ResourceId,
    owner: ObjectId,
    kind: ResourceKind,   // Memory, GpuBuffer, FileHandle, Socket, ...
    size: u64,
}
```

### 6.2. Transfer

Resources are transferred through events or contracts. No hidden shared state.

---

## 7. Presentation

### 7.1. Rendering Is a Subscriber

The renderer is an independent object subscribed to state change events.

```
  GameStateChanged ──▶ Renderer (reads state, draws)
```

Rendering does not affect simulation. It can run at any frequency (30fps, 60fps, 144fps); the simulation is unaffected.

### 7.2. View vs Model

```
  Object (model) ──event──▶ View (presentation)
       │                          │
       │                   ┌──────┴──────┐
       │                   │  UI / 3D /  │
       │                   │  Sound / FX │
       │                   └─────────────┘
       │
  (lives independently, sleeps/wakes)
```

---

## 8. Verification

### 8.1. Pre-execution

Before executing an action:

- are all dependencies satisfied?
- are all required resources available?
- does the action violate any contracts?

### 8.2. Post-execution

After execution:

- verify results match the contract
- if error — activate Fallback

### 8.3. Fallback

```rust
enum Fallback {
    Retry,          // retry the action
    Skip,           // skip, continue
    Revert,         // roll back state
    Terminate,      // destroy object
    PropagateError, // send Event::Error to subscribers
}
```

---

## 9. MVP: Colony Sim Core

### 9.1. What We Model

A simplified colony simulator (Factorio/RimWorld-lite):

- `Tree` — resource, regenerates over time
- `Mine` — extracts ore
- `Factory` — consumes resources, produces goods
- `Transporter` — moves resources between objects
- `Player` — sets goals
- `City` — consumes goods, grows

### 9.2. Objects and Contracts

```
  Tree:
    Produces: Wood (1/10s)
    Wake: Timer(10s)

  Mine:
    Produces: Iron (1/5s)
    Wake: Timer(5s)

  Factory:
    Needs: Wood(2) + Iron(1)
    Produces: Steel(1)
    Wake: ResourceChanged(Wood), ResourceChanged(Iron)

  Transporter:
    Needs: Cargo
    Produces: Transport(from, to)
    Wake: ResourceAvailable(Cargo), TransferComplete

  City:
    Needs: Steel(1/30s)
    Wake: Timer(30s), ResourceChanged(Steel)
```

### 9.3. Event Flow

```
  Timer(10s) ──▶ Tree ──event: WoodProduced──▶ Factory (contract: Wood)
                                                   │
                                                   ▼
                                              (awaits Iron)
                                                   │
  Timer(5s) ──▶ Mine ──event: IronProduced───▶ Factory (contract: Iron)
                                                   │
                                                   ▼
                                              Produces Steel
                                                   │
                                                   ▼
                                        event: SteelProduced
                                                   │
                                                   ▼
                                              City (contract: Steel)
```

### 9.4. MVP Metrics

| Metric | ECS approach | DSO approach |
|---|---|---|
| Active objects | 100% | ~0.01–1% |
| CPU per frame | O(N) | O(k), k = active count |
| Predictability | Low | High |
| Causal graph | Implicit | Explicit |

---

## 10. Roadmap

### Phase 0: Rust prototype (current)
- Event loop without global `Update()`
- 3 object types: Tree, Factory, City
- Contracts via definitions
- Benchmark: 1M objects, all sleeping, single event

### Phase 1: Completeness
- All 5 graphs (Dependency, Event, Resource, Lifetime, Memory)
- Pre/post execution verification
- Fallback system
- Timer-as-event

### Phase 2: Integration
- DSO-IR export
- Adapter for Godot / Bevy
- Incremental state persistence

### Phase 3: Distribution
- Event Graph can span machines
- Dependency Graph distributed
- Deterministic lockstep for multiplayer
