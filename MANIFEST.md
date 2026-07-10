# DSO Manifesto v3.0

> **Determinism First**
>
> *Any decision that can be made ahead of time must be made ahead of time.*
> *At runtime, the system must not think.*
> *It must execute a pre-verified plan.*

---

## Introduction

Modern software is built around continuous execution.

Loops. Ticks. Polling. Periodic updates. Runtime schedulers making decisions on the fly.

DSO proposes a fundamentally different model. The core idea: **any decision that can be made ahead of time must be made ahead of time.** At runtime, the system must not think — it must execute a pre-verified plan.

### The Goal

The primary goal of DSO is to **minimize runtime uncertainty.**

Not FPS. Not RAM. Not CPU. Those are consequences.

The goal is to make the system maximally:

- **predictable**
- **verifiable**
- **explainable**
- **deterministic**

---

## Principles

### 1. Determinism First

Determinism is not identical results. Determinism is the ability to explain ahead of time:

- why an object exists
- why it woke up
- who woke it
- what resources it uses
- what happens next
- when it goes back to sleep

No hidden runtime magic.

---

### 2. Compile Knowledge

Runtime is not a place for decisions. Runtime is an executor.

All knowledge is moved ahead of time:

- memory
- dependencies
- graphs
- events
- resources
- lifetimes
- verification
- execution paths

---

### 3. Global Planning

Before execution, the system builds a plan.

It constructs:

- **Dependency Graph** — object dependencies
- **Lifetime Graph** — object lifecycles
- **Resource Graph** — resource ownership
- **Memory Graph** — memory allocations
- **Event Graph** — event propagation paths

Runtime computes nothing from scratch.

---

### 4. Verify Early

Every action is verified first. There is no unverified execution.

```
  Event
    │
    ▼
  Verification
    │
    ▼
  Execution
    │
    ▼
  Propagation
```

---

### 5. Local Execution

After the graph is built, each object executes locally. No global world traversal. No endless `Update()` / `Tick()`.

---

### 6. Sleep By Default

Every object sleeps by default. Activity is the exception. The normal system state is sleep.

---

### 7. Nothing Executes Without Reason

No object may execute without a reason.

Reasons:

- event
- state change
- dependency
- contract
- timer
- external signal

Without a reason, the object does not exist for the CPU.

---

### 8. Everything Is An Object

There are no special entities. Player. NPC. Train. LLM. File. Process. Window. Document. Database. Controller. All are a single type of object.

```
Object
├── ID
├── Type
├── State
├── Lifetime
├── Resources
├── Contracts
├── Dependencies
├── Actions
├── Events
├── Verification
├── Presentation
├── Priority
└── SleepState
```

---

### 9. World Is A Dependency Graph

The world is not a list of objects. The world is a graph of causal relationships.

```
  Player
    │
    ▼
  Tree
    │
    ▼
  Wood
    │
    ▼
  Factory
    │
    ▼
  Steel
    │
    ▼
  Train
    │
    ▼
  City
```

Change propagates only through the graph.

---

### 10. Contracts Instead Of Hidden Logic

Behavior is defined by contracts. Not by hidden code.

```
  Factory
  ├── Needs:    Iron, Coal, Workers
  ├── Produces: Steel
  └── Wake:     IronChanged, CoalChanged, PowerChanged
```

The contract is the primary source of behavior.

---

### 11. Actions Are Objects

An Action is not a function. An Action is a graph node.

```
  Attack
  ├── Needs:    Weapon, Target
  ├── Produces: Damage, Animation, Sound, Events
  └── ...
```

Every action is an object.

---

### 12. Presentation Is Independent

UI. Animations. Sound. Effects. They contain no logic. They subscribe to events. Logic exists separately.

---

### 13. Every Resource Has An Owner

Memory. GPU. File. Thread. Buffer. Socket.

Every resource has an owner. No anonymous resources.

---

### 14. Total Processing

Every object must complete its processing. Even an error is a formal state. No `Unhandled`. Only `Fallback`.

---

### 15. Time Is Also An Event

Time is a regular object. There is no need to update the world 60 times per second. If the next event is in 5 minutes, the system can jump directly to it.

---

### 16. One Runtime

OCR. LLM. Speech. DSP. Physics. Gameplay. Database. Operating System.

Not separate runtimes. One universal executor.

---

### 17. Universal Object Runtime

DSO is not a game engine. Not an OS. Not a library. DSO is a universal object runtime.

---

## Applications

### Games

Instead of `Update()` / `Tick()`:

```
  Wake → Execute → Sleep
```

NPCs are not updated constantly. The economy wakes only when an event occurs. The player is not the center of the world. The world exists independently.

### DSO World

The primary demonstration of the philosophy. One million objects. Only 20 active. Everything else sleeps. The world continues to live even when there are no players.

### DSO World IR

Any game can theoretically export not code, but the meaning of objects:

```
  Object → State → Contracts → Dependencies → Actions → Resources
```

This enables adapters between different engines using a shared object semantics.

### AI

Whisper, LLM, OCR, TTS, DSP, Vision — become a single execution graph.

### Embedded

ESP32. STM32. PLC. Wake only on events. No wasted work.

### Operating System

Processes. Files. Windows. Drivers. Scheduler. Everything exists as a Dependency Graph.

### Database

Not rows. Not tables. Objects with lifecycles, contracts, dependencies, and events.

### CAD

Changing one bolt does not rebuild the entire model. Only affected objects are activated.

### IDE

A file changes. Only affected objects wake: AST, index, diagnostics, autocomplete.

### DSP

Effects. Equalizer. Processing. Run as an event graph. Do not execute without reason.

---

## Core Philosophy

DSO is not about memory. Not about ECS. Not about arenas. Not about games. Not even about determinism itself.

It is a **general methodology for building computational systems**, based on five fundamental ideas:

1. **Determinism First** — any decision that can be made ahead of time must be made ahead of time.
2. **Compile Globally** — maximally analyze, plan, and build graphs before execution.
3. **Verify Early** — verify correctness before executing actions.
4. **Execute Locally** — execute only locally affected objects and dependencies.
5. **Sleep By Default** — every computation must have a formal cause; absence of cause means no execution.

### The Key Question

Most software today answers: *"What needs to execute now?"*

DSO answers a different question: *"Why should this execute at all?"*

Without a formal cause, the system must not do work.

---

> **Important note.** All of this is an architectural hypothesis, not a proven theory. Some principles (event-driven processing, dependency graphs, ahead-of-time planning) are already used in specific domains. DSO's novelty will be determined by whether a unified methodology can demonstrate measurable advantages across multiple system classes (AI runtime, game simulations, embedded, databases, DSP). Working prototypes and benchmarks will confirm or refute this hypothesis.
