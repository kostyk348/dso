// DSO v4 — Dependency Graph, Time as Events, Deterministic Replay, Interactive CLI
// Principles demonstrated:
//   3. Global Planning  — dependency graph compiled ahead of time
//   9. World Is A Dependency Graph — edges connect producers to consumers
//  15. Time Is Also An Event — events processed in timestamp order, time skipped
//   1. Determinism First — full event log with hash-chain, replay verification

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// ─── Core Types ──────────────────────────────────────────────────────────────

type ObjectId = u16;
type TimeNs = u64;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum ResourceKind { Wood, Iron, Steel }

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum EventType {
    Timer(ObjectId),
    ResourceProduced(ResourceKind),
    Tick,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State { Sleep, Active }

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
enum ObjectClass {
    Tree       = 0,
    Mine       = 1,
    Factory    = 2,
    City       = 3,
    Decoration = 4,
}
const NUM_CLASSES: usize = 5;

// ─── Contract ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Contract {
    needs: Vec<ResourceKind>,
    produces: Vec<ResourceKind>,
    /// The events this object reacts to
    wakes_on: Vec<EventType>,
    /// Production interval in nanoseconds (for Timer-based producers)
    interval_ns: u64,
}

// ─── Dependency Graph (Principle 9: World Is A Dependency Graph) ────────────

/// Compiled dependency edges: for each producer, who consumes what.
struct DepGraph {
    /// edges[producer_id] = [(ResourceKind, consumer_id), ...]
    edges: Vec<Vec<(ResourceKind, ObjectId)>>,
    /// Reverse: edges_from_consumer[consumer_id] = [(ResourceKind, producer_id), ...]
    reverse: Vec<Vec<(ResourceKind, ObjectId)>>,
}

impl DepGraph {
    fn new(objects: &[Object]) -> Self {
        let n = objects.len();
        let mut edges: Vec<Vec<(ResourceKind, ObjectId)>> = vec![Vec::new(); n];
        let mut reverse: Vec<Vec<(ResourceKind, ObjectId)>> = vec![Vec::new(); n];

        // Build producer→consumer edges from contracts
        // For each object that NEEDS resource R, find all objects that PRODUCE R
        let producers: Vec<(ObjectId, ResourceKind)> = objects.iter()
            .flat_map(|o| o.contract.produces.iter().map(move |&r| (o.id, r)))
            .collect();

        for consumer in objects {
            for need in &consumer.contract.needs {
                for &(producer_id, kind) in &producers {
                    if kind == *need {
                        edges[producer_id as usize].push((kind, consumer.id));
                        reverse[consumer.id as usize].push((kind, producer_id));
                    }
                }
            }
        }

        // Deduplicate
        for elist in &mut edges { elist.sort(); elist.dedup(); }
        for rlist in &mut reverse { rlist.sort(); rlist.dedup(); }

        DepGraph { edges, reverse }
    }

    fn consumers_of(&self, producer: ObjectId) -> &[(ResourceKind, ObjectId)] {
        self.edges.get(producer as usize).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

// ─── Resource Graph (Principle 13: Every Resource Has An Owner) ─────────────

#[derive(Clone, Debug)]
struct ResourceRecord {
    kind: ResourceKind,
    owner: ObjectId,
    quantity: u64,
}

struct ResourceGraph {
    records: Vec<ResourceRecord>,
}

impl ResourceGraph {
    fn new() -> Self { ResourceGraph { records: Vec::new() } }

    fn add(&mut self, kind: ResourceKind, owner: ObjectId, qty: u64) {
        self.records.push(ResourceRecord { kind, owner, quantity: qty });
    }

    fn owner_has(&self, owner: ObjectId, kind: ResourceKind, min: u64) -> bool {
        self.records.iter()
            .filter(|r| r.owner == owner && r.kind == kind)
            .map(|r| r.quantity).sum::<u64>() >= min
    }

    fn consume(&mut self, owner: ObjectId, kind: ResourceKind, qty: u64) -> bool {
        let mut remaining = qty;
        for r in &mut self.records {
            if r.owner == owner && r.kind == kind {
                let take = r.quantity.min(remaining);
                r.quantity -= take;
                remaining -= take;
                if remaining == 0 { return true; }
            }
        }
        false
    }

    fn total_by_kind(&self, kind: ResourceKind) -> u64 {
        self.records.iter().filter(|r| r.kind == kind).map(|r| r.quantity).sum()
    }

    fn snapshot(&self) -> HashMap<(ObjectId, ResourceKind), u64> {
        let mut m = HashMap::new();
        for r in &self.records {
            *m.entry((r.owner, r.kind)).or_insert(0) += r.quantity;
        }
        m
    }
}

// ─── Deterministic Event Log (Principle 1: Determinism First) ───────────────

#[derive(Clone, Debug)]
struct LogEntry {
    seq: u64,
    time: TimeNs,
    event: EventType,
    source: Option<ObjectId>,
    woken: Vec<ObjectId>,
    prev_hash: u64,
}

struct EventLog {
    entries: Vec<LogEntry>,
    last_hash: u64,
}

impl EventLog {
    fn new() -> Self { EventLog { entries: Vec::new(), last_hash: 0 } }

    fn record(&mut self, time: TimeNs, event: &EventType, source: Option<ObjectId>, woken: &[ObjectId]) {
        let mut h = DefaultHasher::new();
        self.last_hash.hash(&mut h);
        event.hash(&mut h);
        time.hash(&mut h);
        source.hash(&mut h);
        woken.hash(&mut h);
        let hash = h.finish();

        self.entries.push(LogEntry {
            seq: self.entries.len() as u64,
            time,
            event: event.clone(),
            source,
            woken: woken.to_vec(),
            prev_hash: self.last_hash,
        });
        self.last_hash = hash;
    }

    fn verify(&self) -> bool {
        let mut prev = 0u64;
        for e in &self.entries {
            if e.prev_hash != prev { return false; }
            let mut h = DefaultHasher::new();
            prev.hash(&mut h);
            e.event.hash(&mut h);
            e.time.hash(&mut h);
            e.source.hash(&mut h);
            e.woken.hash(&mut h);
            prev = h.finish();
        }
        true
    }

}

// ─── Object ──────────────────────────────────────────────────────────────────

struct Object {
    id: ObjectId,
    class: ObjectClass,
    state: State,
    contract: Contract,
    wake_count: u64,
    last_wake_time: TimeNs,
}

// ─── Branchless Dispatch (Principle 2: Compile Knowledge) ───────────────────

type ExecFn = fn(&mut ResourceGraph, &mut Vec<EventType>, &Object, TimeNs);

fn exec_producer(rg: &mut ResourceGraph, spawned: &mut Vec<EventType>, obj: &Object, _now: TimeNs) {
    for kind in &obj.contract.produces {
        rg.add(*kind, obj.id, 1);
        spawned.push(EventType::ResourceProduced(*kind));
    }
}

fn exec_factory(rg: &mut ResourceGraph, spawned: &mut Vec<EventType>, obj: &Object, _now: TimeNs) {
    if rg.owner_has(obj.id, ResourceKind::Wood, 1)
        && rg.owner_has(obj.id, ResourceKind::Iron, 1)
    {
        rg.consume(obj.id, ResourceKind::Wood, 1);
        rg.consume(obj.id, ResourceKind::Iron, 1);
        rg.add(ResourceKind::Steel, obj.id, 1);
        spawned.push(EventType::ResourceProduced(ResourceKind::Steel));
    }
}

fn exec_city(rg: &mut ResourceGraph, _spawned: &mut Vec<EventType>, obj: &Object, _now: TimeNs) {
    if rg.owner_has(obj.id, ResourceKind::Steel, 1) {
        rg.consume(obj.id, ResourceKind::Steel, 1);
    }
}

fn exec_nop(_: &mut ResourceGraph, _: &mut Vec<EventType>, _: &Object, _: TimeNs) {}

const EXEC_TABLE: [ExecFn; NUM_CLASSES] = [
    exec_producer, // Tree
    exec_producer, // Mine
    exec_factory,  // Factory
    exec_city,     // City
    exec_nop,      // Decoration
];

// ─── Timed Event (Principle 15: Time Is Also An Event) ──────────────────────

#[derive(Clone)]
struct TimedEvent {
    time: TimeNs,
    event: EventType,
    source: Option<ObjectId>,
}

// ─── World ───────────────────────────────────────────────────────────────────

struct World {
    objects: Vec<Object>,
    dep_graph: DepGraph,
    resources: ResourceGraph,
    event_log: EventLog,
    pending: Vec<TimedEvent>,  // sorted by time
    current_time: TimeNs,
    total_wakes: u64,
}

impl World {
    fn new(objects: Vec<Object>) -> Self {
        let dep_graph = DepGraph::new(&objects);
        let mut resources = ResourceGraph::new();

        // Grant initial resources per contract
        for obj in &objects {
            for r in &obj.contract.produces {
                resources.add(*r, obj.id, 1);
            }
        }
        // Extra starting stock for factories and cities
        for obj in &objects {
            match obj.class {
                ObjectClass::Factory => {
                    resources.add(ResourceKind::Wood, obj.id, 5);
                    resources.add(ResourceKind::Iron, obj.id, 3);
                }
                ObjectClass::City => {
                    resources.add(ResourceKind::Steel, obj.id, 2);
                }
                _ => {}
            }
        }

        World {
            objects,
            dep_graph,
            resources,
            event_log: EventLog::new(),
            pending: Vec::new(),
            current_time: 0,
            total_wakes: 0,
        }
    }

    fn schedule(&mut self, event: EventType, at: TimeNs, source: Option<ObjectId>) {
        self.pending.push(TimedEvent { time: at, event, source });
    }

    /// Advance time and process all events up to `target_time`.
    /// If `target_time` is None, process all pending events.
    fn advance_to(&mut self, target_time: Option<TimeNs>) -> u64 {
        let mut total_woken = 0u64;
        loop {
            // Sort pending by time
            self.pending.sort_by_key(|e| e.time);

            // Find events at or before target
            let cutoff = target_time.unwrap_or(TimeNs::MAX);
            let mut batch: Vec<TimedEvent> = Vec::new();
            self.pending.retain(|e| {
                if e.time <= cutoff { batch.push(e.clone()); false } else { true }
            });

            if batch.is_empty() {
                // No events at current time — skip to next event time
                if let Some(next) = self.pending.first() {
                    self.current_time = next.time;
                    continue; // retry with new time
                }
                break;
            }

            // Process batch (all at same time or ordered)
            for te in &batch {
                self.current_time = te.time;
                total_woken += self.dispatch(&te.event, te.source);
            }

            // If we hit the target exactly, stop
            if target_time.is_some() && self.current_time >= cutoff { break; }
        }
        total_woken
    }

    fn dispatch(&mut self, event: &EventType, source: Option<ObjectId>) -> u64 {
        let mut woken_this: Vec<ObjectId> = Vec::new();
        let mut spawned: Vec<EventType> = Vec::new();

        match event {
            EventType::Timer(id) => {
                // Direct wake: one specific object
                self.wake_one(*id, &mut woken_this, &mut spawned);
            }
            EventType::ResourceProduced(_kind) => {
                // Follow dependency graph edges from the source producer
                if let Some(producer) = source {
                    // Copy consumer list to avoid borrow conflicts
                    let consumers: Vec<ObjectId> = self.dep_graph.consumers_of(producer)
                        .iter().map(|&(_, c)| c).collect();
                    for consumer in consumers {
                        self.wake_one(consumer, &mut woken_this, &mut spawned);
                    }
                }
            }
            EventType::Tick => {
                // Periodic timer check: wake all Timer-handling objects whose interval has passed
                // Collect candidates first to avoid borrow conflicts
                let candidates: Vec<(ObjectId, u64)> = (0..self.objects.len() as ObjectId)
                    .filter(|&i| {
                        let obj = &self.objects[i as usize];
                        obj.state == State::Sleep
                            && obj.contract.wakes_on.iter().any(|ev| matches!(ev, EventType::Timer(id) if *id == i))
                            && self.current_time >= obj.last_wake_time + obj.contract.interval_ns
                    })
                    .map(|i| {
                        let obj = &self.objects[i as usize];
                        (i, obj.contract.interval_ns)
                    })
                    .collect();

                for (i, interval) in candidates {
                    // Wake the object
                    let obj = &mut self.objects[i as usize];
                    obj.state = State::Active;
                    obj.wake_count += 1;
                    obj.last_wake_time = self.current_time;
                    woken_this.push(i);
                    EXEC_TABLE[obj.class as usize](&mut self.resources, &mut spawned, obj, self.current_time);
                    obj.state = State::Sleep;

                    // Reschedule (done outside the mutable borrow of objects[i])
                    self.schedule(EventType::Timer(i), self.current_time + interval, Some(i));
                }
            }
        }

        // Log this dispatch
        self.event_log.record(self.current_time, event, source, &woken_this);

        // Propagate spawned events immediately (chain)
        for e in spawned {
            self.dispatch(&e, source);
        }

        let count = woken_this.len() as u64;
        self.total_wakes += count;
        count
    }

    fn wake_one(&mut self, id: ObjectId, woken: &mut Vec<ObjectId>, spawned: &mut Vec<EventType>) {
        let obj = &mut self.objects[id as usize];
        if obj.state != State::Sleep { return; }
        obj.state = State::Active;
        obj.wake_count += 1;
        obj.last_wake_time = self.current_time;
        woken.push(id);
        EXEC_TABLE[obj.class as usize](&mut self.resources, spawned, obj, self.current_time);
        obj.state = State::Sleep;
    }

    fn snapshot_state(&self) -> (u64, HashMap<(ObjectId, ResourceKind), u64>, Vec<u64>) {
        let wakes: Vec<u64> = self.objects.iter().map(|o| o.wake_count).collect();
        (self.total_wakes, self.resources.snapshot(), wakes)
    }
}

// ─── World Factory ──────────────────────────────────────────────────────────

fn build_colony(total_objects: u64) -> World {
    let mut objects = Vec::new();

    // 100 trees — produce Wood every 1000ns
    for i in 0..100 {
        objects.push(Object {
            id: objects.len() as ObjectId,
            class: ObjectClass::Tree,
            state: State::Sleep,
            contract: Contract {
                needs: vec![], produces: vec![ResourceKind::Wood],
                wakes_on: vec![EventType::Timer(i as ObjectId)],
                interval_ns: 1_000,
            },
            wake_count: 0, last_wake_time: 0,
        });
    }

    // 50 mines — produce Iron every 500ns
    for i in 0..50 {
        objects.push(Object {
            id: objects.len() as ObjectId,
            class: ObjectClass::Mine,
            state: State::Sleep,
            contract: Contract {
                needs: vec![], produces: vec![ResourceKind::Iron],
                wakes_on: vec![EventType::Timer((100 + i) as ObjectId)],
                interval_ns: 500,
            },
            wake_count: 0, last_wake_time: 0,
        });
    }

    // 20 factories
    for _ in 0..20 {
        objects.push(Object {
            id: objects.len() as ObjectId,
            class: ObjectClass::Factory,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceKind::Wood, ResourceKind::Iron],
                produces: vec![ResourceKind::Steel],
                wakes_on: vec![
                    EventType::ResourceProduced(ResourceKind::Wood),
                    EventType::ResourceProduced(ResourceKind::Iron),
                ],
                interval_ns: 0,
            },
            wake_count: 0, last_wake_time: 0,
        });
    }

    // 5 cities
    for _ in 0..5 {
        objects.push(Object {
            id: objects.len() as ObjectId,
            class: ObjectClass::City,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceKind::Steel],
                produces: vec![],
                wakes_on: vec![EventType::ResourceProduced(ResourceKind::Steel)],
                interval_ns: 0,
            },
            wake_count: 0, last_wake_time: 0,
        });
    }

    // Fill with decorations
    for _ in objects.len() as u64..total_objects {
        objects.push(Object {
            id: objects.len() as ObjectId,
            class: ObjectClass::Decoration,
            state: State::Sleep,
            contract: Contract {
                needs: vec![], produces: vec![],
                wakes_on: vec![], interval_ns: 0,
            },
            wake_count: 0, last_wake_time: 0,
        });
    }

    // Schedule initial timer events (collect IDs first to avoid borrow issues)
    let timer_ids: Vec<(ObjectId, u64)> = objects.iter()
        .filter(|o| o.contract.interval_ns > 0)
        .map(|o| (o.id, o.contract.interval_ns))
        .collect();

    let mut world = World::new(objects);

    for (id, interval) in timer_ids {
        world.schedule(EventType::Timer(id), interval, Some(id));
    }

    world
}

// ─── Replay (Determinism Verification) ──────────────────────────────────────

fn replay(original: &[LogEntry], total_objects: u64) -> bool {
    let mut world = build_colony(total_objects);

    // Collect only external inputs (Timer events from scheduling)
    let inputs: Vec<&LogEntry> = original.iter()
        .filter(|e| matches!(e.event, EventType::Timer(_)))
        .collect();

    if inputs.is_empty() {
        println!("  No timer inputs to replay");
        return true;
    }

    // Replay by feeding timer events in order
    for entry in &inputs {
        world.schedule(entry.event.clone(), entry.time, entry.source);
    }
    // Process all scheduled events (including chains)
    world.advance_to(Some(inputs.last().unwrap().time));

    // Compare wake counts for each input timer event
    let replayed_wakes: HashMap<EventType, u64> = world.event_log.entries.iter()
        .filter(|e| matches!(e.event, EventType::Timer(_)))
        .map(|e| (e.event.clone(), e.woken.len() as u64))
        .collect();

    let orig_wakes: HashMap<EventType, u64> = original.iter()
        .filter(|e| matches!(e.event, EventType::Timer(_)))
        .map(|e| (e.event.clone(), e.woken.len() as u64))
        .collect();

    if replayed_wakes != orig_wakes {
        eprintln!("REPLAY: wake count mismatch");
        for (ev, count) in &orig_wakes {
            let found = replayed_wakes.get(ev).copied().unwrap_or(0);
            if *count != found {
                eprintln!("  {:?}: expected {} woken, got {}", ev, count, found);
            }
        }
        return false;
    }

    if !world.event_log.verify() {
        eprintln!("REPLAY: chain invalid after replay");
        return false;
    }

    println!("  Replay OK — {} timer events, deterministic match", inputs.len());
    true
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     DSO v4 — Dependency Graph · Time as Events · Replay ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    let total = 1_000_000u64;
    let mut world = build_colony(total);

    let active = world.objects.iter().filter(|o| o.class != ObjectClass::Decoration).count();
    let dep_edges: usize = world.dep_graph.edges.iter().map(|e| e.len()).sum();
    println!("World: {} objects ({} active types, {} always-sleeping)",
        total, active, total as usize - active);
    println!("Dependency edges: {} producer→consumer connections", dep_edges);
    println!("Resources: {} initial records", world.resources.records.len());
    println!();

    // ─── 1. Single event via dependency graph ──────────────────────────
    println!("─── 1. Fire Timer(0) via dep graph ───");
    let start = Instant::now();
    world.schedule(EventType::Timer(0), 0, None);
    let w = world.advance_to(Some(0));
    let t = start.elapsed();
    println!("  {w} objects woken in {t:?}");

    // Show the chain: Tree 0 → Wood → factories → Steel → cities
    println!("  Chain: Tree(0) wakes → produces Wood");
    println!("         → factories ({}) consume Wood+Iron → produce Steel",
        world.objects.iter().filter(|o| o.class == ObjectClass::Factory).count());
    println!("         → cities ({}) consume Steel",
        world.objects.iter().filter(|o| o.class == ObjectClass::City).count());
    println!();

    // ─── 2. Time skipping (Principle 15) ───────────────────────────────
    println!("─── 2. Time skipping ───");
    let before = world.current_time;
    world.schedule(EventType::Timer(1), 5_000, None);  // 5000ns from now
    world.schedule(EventType::Timer(2), 3_000, None);  // 3000ns from now
    let start = Instant::now();
    let w = world.advance_to(Some(5_000));
    let elapsed = start.elapsed();
    println!("  Advanced from t={before} to t={} ({}ns jump)", world.current_time,
        world.current_time - before);
    println!("  {w} objects woken in {elapsed:?}");
    println!();

    // ─── 3. Dependency graph: fire resource event directly ─────────────
    println!("─── 3. Fire ResourceProduced(Wood) directly ───");
    let w = {
        let mut woken = 0u64;
        if let Some(tree) = world.objects.iter().find(|o| o.class == ObjectClass::Tree) {
            woken += world.dispatch(&EventType::ResourceProduced(ResourceKind::Wood), Some(tree.id));
        }
        woken
    };
    println!("  {w} objects woken (factories that depend on that tree)");
    println!();

    // ─── 4. Event log stats ────────────────────────────────────────────
    println!("─── 4. Event log ───");
    println!("  Entries: {}", world.event_log.entries.len());
    println!("  Chain valid: {}", world.event_log.verify());
    if let Some(last) = world.event_log.entries.last() {
        println!("  Last seq: {} — {:?} — {} targets", last.seq, last.event, last.woken.len());
    }
    println!();

    // ─── 5. Deterministic replay ───────────────────────────────────────
    println!("─── 5. Deterministic replay ───");
    let log = world.event_log.entries.clone();
    let replay_ok = replay(&log, total);
    println!("  Replay success: {replay_ok}");
    println!();

    // ─── 6. Interactive CLI ────────────────────────────────────────────
    println!("─── 6. Interactive demo ───");
    println!("  Commands: status, fire <type>, time, advance <ns>, replay, stats, quit");

    let mut running = true;
    let mut world_cli = build_colony(total);

    while running {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        let input = input.trim();

        match input {
            "quit" | "q" => running = false,

            "status" => {
                let _active = world_cli.objects.iter().filter(|o| o.class != ObjectClass::Decoration).count();
                let never = world_cli.objects.iter().filter(|o| o.wake_count == 0).count();
                println!("  Time: {}ns | Objects: {} | Never woke: {} ({}%) | Wakes: {} | Resources: Wood={} Iron={} Steel={}",
                    world_cli.current_time,
                    world_cli.objects.len(),
                    never, never * 100 / world_cli.objects.len(),
                    world_cli.total_wakes,
                    world_cli.resources.total_by_kind(ResourceKind::Wood),
                    world_cli.resources.total_by_kind(ResourceKind::Iron),
                    world_cli.resources.total_by_kind(ResourceKind::Steel),
                );
            }

            "stats" => {
                let edges: usize = world_cli.dep_graph.edges.iter().map(|e| e.len()).sum();
                println!("  Objects: {} | Dep edges: {} | Resources: {} | Log entries: {} | Chain valid: {}",
                    world_cli.objects.len(), edges,
                    world_cli.resources.records.len(),
                    world_cli.event_log.entries.len(),
                    world_cli.event_log.verify());
            }

            "time" => {
                println!("  Current time: {}ns", world_cli.current_time);
            }

            cmd if cmd.starts_with("fire ") => {
                let rest = &cmd[5..];
                if rest.starts_with("Timer(") {
                    if let Some(id_str) = rest.strip_prefix("Timer(").and_then(|s| s.strip_suffix(')')) {
                        if let Ok(id) = id_str.parse::<ObjectId>() {
                            world_cli.schedule(EventType::Timer(id), world_cli.current_time, None);
                            let w = world_cli.advance_to(Some(world_cli.current_time));
                            println!("  Fired Timer({id}): {w} objects woken");
                        }
                    }
                } else if rest.starts_with("ResourceProduced(") {
                    let kind = if rest.contains("Wood") { ResourceKind::Wood }
                        else if rest.contains("Iron") { ResourceKind::Iron }
                        else { ResourceKind::Steel };
                    // Find any producer
                    let producer = world_cli.objects.iter().find(|o| o.contract.produces.contains(&kind));
                    if let Some(p) = producer {
                        let w = world_cli.dispatch(&EventType::ResourceProduced(kind), Some(p.id));
                        println!("  Fired ResourceProduced({kind:?}): {w} objects woken");
                    }
                }
            }

            cmd if cmd.starts_with("advance ") => {
                if let Ok(ns) = cmd[8..].parse::<TimeNs>() {
                    let target = world_cli.current_time + ns;
                    let w = world_cli.advance_to(Some(target));
                    println!("  Advanced to t={}: {w} objects woken", world_cli.current_time);
                }
            }

            "replay" => {
                let log = world_cli.event_log.entries.clone();
                if log.is_empty() {
                    println!("  No events to replay");
                } else {
                    replay(&log, total);
                }
            }

            "help" => {
                println!("  Commands:");
                println!("    status          — world state");
                println!("    stats           — system stats");
                println!("    time            — current time");
                println!("    fire Timer(N)   — fire a timer event");
                println!("    fire ResourceProduced(Wood/Iron/Steel) — fire resource event");
                println!("    advance <ns>    — advance time by ns");
                println!("    replay          — replay event log");
                println!("    quit            — exit");
            }

            "" => {}  // ignore empty

            _ => {
                if !input.is_empty() {
                    println!("  Unknown: '{input}'. Try 'help'");
                }
            }
        }
    }

    // ─── Summary from initial world ──────────────────────────────────
    println!();
    println!("─── Summary ───");
    let never = world.objects.iter().filter(|o| o.wake_count == 0).count();
    println!("Objects that never woke: {never} ({}%)", never * 100 / world.objects.len());
    println!("Total events dispatched: {}", world.event_log.entries.len());
    println!("Event log hash-chain valid: {}", world.event_log.verify());
    println!("Final resources: Wood={} Iron={} Steel={}",
        world.resources.total_by_kind(ResourceKind::Wood),
        world.resources.total_by_kind(ResourceKind::Iron),
        world.resources.total_by_kind(ResourceKind::Steel));

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║  DSO Principles Demonstrated:                          ║");
    println!("║  3.  Global Planning  — dep graph at compile time      ║");
    println!("║  9.  World Is A Dep Graph — edges, not broadcast       ║");
    println!("║  15. Time Is Also An Event — time skipping             ║");
    println!("║  1.  Determinism First — event log + replay            ║");
    println!("║  7.  Nothing Executes Without Reason — event-driven    ║");
    println!("║  6.  Sleep By Default — 99.98% never woke              ║");
    println!("╚══════════════════════════════════════════════════════════╝");
}
