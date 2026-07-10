# DSO: Branchless by Design

> *Branching is a decision. A decision made too late.*

---

## The Core Idea

Every `if`, `switch`, `match`, `??`, `?.`, ternary branch, or variable-length loop is a **decision made at runtime.**

DSO asserts: any decision that can be made ahead of time must be made ahead of time. Therefore, **every branch in the hot path is either an architecture mistake or unfinished work.**

The goal: **zero branches in the critical execution path.**

---

## What Replaces Branching

### 1. Match → Dispatch Table

Instead of:

```rust
match obj.class {
    Tree => { /* ... */ }
    Factory => { /* ... */ }
    City => { /* ... */ }
}
```

Use a function table compiled at initialization:

```rust
type ActionFn = fn(&mut Object, &Event);

const DISPATCH: [[ActionFn; MAX_EVENTS]; MAX_TYPES] = build_dispatch();

// hot path — zero branches
DISPATCH[obj.type_id][event.type_id](obj, event);
```

### 2. Polling → Event Routing

Instead of:

```rust
for obj in world {
    if obj.has_event() {  // branch
        process(obj);
    }
}
```

Use a pre-built route graph. Events are delivered directly to recipients. No scan loop — just direct addressing.

### 3. Type Checks → Compile-time Dispatch

Instead of:

```rust
if let Some(damage) = entity.get::<Damage>() {  // branch
    // ...
}
```

Every object already knows its role. The action table is compiled. Type checks are replaced by array indices.

### 4. State Machines → State Tables

Instead of:

```rust
match state {
    State::Idle => { if event == Enter { state = Active; } }
    State::Active => { /* ... */ }
}
```

A transition table: `next_state = TRANSITIONS[state][event]`. Zero branches.

### 5. Null/Option Checks → Pre-allocated Resources

Instead of:

```rust
if let Some(resource) = obj.get_resource() {  // branch
    use(resource);
}
```

The resource is guaranteed by contract. If it cannot be available, the object will not be woken. The check is moved to the verification phase before execution.

---

## Branch Hierarchy and Elimination

| Branch type | Example | DSO replacement |
|---|---|---|
| Dispatch | `match type` | Action table (indirect call) |
| Polling | `if has_event` | Event route graph |
| State | `if state == X` | State transition table |
| Null-check | `if let Some(x)` | Pre-allocated, contract-guaranteed |
| Resource | `if resource.available()` | Pre-verified before wake |
| Loop | `for obj in all_objects` | Direct delivery to N targets |
| Boundary | `if i < len` | Known-size arrays, compile-time |

Each branch type is replaced by **pre-compiled data**.

---

## Why Indirect Call Is Not a Branch

An indirect call (`call *rax`) is control transfer, not branching. The CPU does not speculate (or speculates better than on branches). Branch misprediction penalty is absent — the cost is only I-cache and BTB.

Difference on Skylake:

| Instruction | Mispredict penalty |
|---|---|
| `jcc` (branch) | ~15-20 cycles |
| `call *rax` (indirect) | ~1-2 cycles (BTB hit) |

Indirect call is an acceptable price for eliminating branches.

---

## Branchless in the DSO Prototype

In `dso_proto`, hot-path branches are replaced:

1. **Event → Target** — flat route table instead of linear search
2. **Object → Action** — function pointer table instead of `match`
3. **State check** — object is either Sleep (skipped) or Active (executes). Single check at entry.
4. **Resource availability** — verified pre-execution via Resource Graph, not during execution

---

## Measurements

Branchless design gives measurable advantages when:

- **Branch frequency is high** (>50% of objects fail the check)
- **Branch predictor fails** (random data, changing patterns)
- **Pipeline is deep** (modern CPUs: 14-19 stages)
- **Code footprint is large** (BTB capacity exceeded)

Classic example: `Update()` in ECS where 99% of entities lack the required component.

DSO eliminates branching not through hardware tricks, but by **changing the execution model**: an object is either asleep (does not exist for the CPU) or active (guaranteed to execute an action).

---

## Relationship to DSO Principles

| Principle | How it eliminates branching |
|---|---|
| **Determinism First** | Decisions made ahead → nothing to branch on |
| **Compile Knowledge** | Data replaces logic → tables replace ifs |
| **Global Planning** | Graph replaces loop → addressing replaces scanning |
| **Verify Early** | Guarantees before execution → no runtime checks |
| **Sleep By Default** | Inactive objects never checked → no loop overhead |
| **Local Execution** | Each object knows its role → dispatch pre-tabulated |

---

## Conclusion

**Branchlessness is not an optimization. It's a consequence.**

When a system is designed according to DSO, it naturally contains no branches in the critical path. Runtime does not choose — runtime executes.

Branching is a symptom of a decision deferred to runtime.
