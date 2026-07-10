#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// ─── Core Types ───────────────────────────────────────────────────────────────

type ObjectId = u32;
type SeqNo = u64;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum ResourceKind { Wood, Iron, Steel }

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum EventType {
    Timer(ObjectId),
    ResourceProduced(ResourceKind),
    ResourceConsumed(ResourceKind),
    ResourceTransferred { kind: ResourceKind, from: ObjectId, to: ObjectId },
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

#[derive(Clone, Debug)]
struct Contract {
    needs: Vec<ResourceKind>,
    produces: Vec<ResourceKind>,
    wakes_on: Vec<EventType>,
}

#[derive(Clone, Copy, Debug)]
enum Fallback { Skip, Revert, Terminate }

// ─── Resource Graph ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ResourceRecord {
    id: u64,
    kind: ResourceKind,
    owner: ObjectId,
    quantity: u64,
}

struct ResourceGraph {
    records: Vec<ResourceRecord>,
    by_owner: HashMap<ObjectId, Vec<usize>>,
    by_kind: HashMap<ResourceKind, Vec<usize>>,
}

impl ResourceGraph {
    fn new() -> Self {
        ResourceGraph { records: Vec::new(), by_owner: HashMap::new(), by_kind: HashMap::new() }
    }

    fn allocate(&mut self, kind: ResourceKind, owner: ObjectId, quantity: u64) -> u64 {
        let id = self.records.len() as u64;
        self.records.push(ResourceRecord { id, kind, owner, quantity });
        self.by_owner.entry(owner).or_default().push(self.records.len() - 1);
        self.by_kind.entry(kind).or_default().push(self.records.len() - 1);
        id
    }

    fn transfer(&mut self, id: u64, new_owner: ObjectId) -> bool {
        if let Some(rec) = self.records.iter_mut().find(|r| r.id == id) {
            rec.owner = new_owner;
            true
        } else { false }
    }

    fn owner_has(&self, owner: ObjectId, kind: ResourceKind, min_qty: u64) -> bool {
        self.by_owner.get(&owner)
            .and_then(|indices| {
                indices.iter()
                    .filter(|&&i| self.records[i].kind == kind)
                    .map(|&i| self.records[i].quantity)
                    .reduce(|a, b| a + b)
            })
            .map_or(false, |total| total >= min_qty)
    }

    fn consume(&mut self, owner: ObjectId, kind: ResourceKind, qty: u64) -> bool {
        let indices: Vec<usize> = self.by_owner.get(&owner)
            .map(|v| v.iter().filter(|&&i| self.records[i].kind == kind).copied().collect())
            .unwrap_or_default();
        let mut remaining = qty;
        for &i in &indices {
            let rec = &mut self.records[i];
            let take = rec.quantity.min(remaining);
            rec.quantity -= take;
            remaining -= take;
            if remaining == 0 { break; }
        }
        remaining == 0
    }

    fn count_by(&self, kind: ResourceKind) -> u64 {
        self.by_kind.get(&kind)
            .map(|v| v.iter().map(|&i| self.records[i].quantity).sum())
            .unwrap_or(0)
    }
}

// ─── Deterministic Event Log ──────────────────────────────────────────────────

struct EventLogEntry {
    seq: SeqNo,
    event_type: EventType,
    source: Option<ObjectId>,
    targets: Vec<ObjectId>,
    prev_hash: u64,
}

struct EventLog {
    entries: Vec<EventLogEntry>,
    last_hash: u64,
    tick_count: u64,
}

impl EventLog {
    fn new() -> Self {
        EventLog { entries: Vec::new(), last_hash: 0, tick_count: 0 }
    }

    fn log(&mut self, event: &EventType, source: Option<ObjectId>, targets: &[ObjectId]) {
        let mut hasher = DefaultHasher::new();
        self.last_hash.hash(&mut hasher);
        event.hash(&mut hasher);
        source.hash(&mut hasher);
        targets.hash(&mut hasher);
        let hash = hasher.finish();

        self.entries.push(EventLogEntry {
            seq: self.entries.len() as SeqNo,
            event_type: event.clone(),
            source,
            targets: targets.to_vec(),
            prev_hash: self.last_hash,
        });
        self.last_hash = hash;
    }

    #[allow(dead_code)]
    fn verify(&self) -> bool {
        let mut prev = 0u64;
        for entry in &self.entries {
            let mut hasher = DefaultHasher::new();
            prev.hash(&mut hasher);
            entry.event_type.hash(&mut hasher);
            entry.source.hash(&mut hasher);
            entry.targets.hash(&mut hasher);
            let expected = hasher.finish();
            // We can't recompute without storing the hash, so just check chain integrity
            // by verifying prev_hash matches the previous computed hash
            if entry.prev_hash != prev { return false; }
            prev = expected;
        }
        true
    }
}

// ─── Object ───────────────────────────────────────────────────────────────────

struct Object {
    id: ObjectId,
    class: ObjectClass,
    state: State,
    contract: Contract,
    wake_count: u64,
}

// ─── Compiled Event Routes (Compile Knowledge) ────────────────────────────────

struct CompiledRoutes {
    /// Flat array: route_table[event_type_id] = list of target ObjectIds
    table: Vec<Vec<ObjectId>>,
    /// Maps EventType -> dense index into table
    type_to_idx: HashMap<EventType, usize>,
}

impl CompiledRoutes {
    fn new() -> Self {
        CompiledRoutes { table: Vec::new(), type_to_idx: HashMap::new() }
    }

    fn compile(&mut self, objects: &[Object]) {
        // Collect all unique event types
        let mut all_events: Vec<&EventType> = objects.iter()
            .flat_map(|o| o.contract.wakes_on.iter())
            .collect();
        all_events.sort();
        all_events.dedup();

        // Build dense index
        for (idx, ev) in all_events.iter().enumerate() {
            self.type_to_idx.insert((*ev).clone(), idx);
        }

        // Build flat route table
        self.table = vec![Vec::new(); all_events.len()];
        for obj in objects {
            for ev in &obj.contract.wakes_on {
                if let Some(&idx) = self.type_to_idx.get(ev) {
                    self.table[idx].push(obj.id);
                }
            }
        }

        // Sort for deterministic execution order
        for targets in &mut self.table {
            targets.sort();
        }
    }

    fn targets_for(&self, event: &EventType) -> &[ObjectId] {
        self.type_to_idx.get(event)
            .and_then(|&idx| self.table.get(idx))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

// ─── Branchless Dispatch Table ────────────────────────────────────────────────

type ExecFn = fn(&mut ResourceGraph, &mut Vec<EventType>, &Object);

fn exec_producer(rg: &mut ResourceGraph, spawned: &mut Vec<EventType>, obj: &Object) {
    for kind in &obj.contract.produces {
        rg.allocate(*kind, obj.id, 1);
        spawned.push(EventType::ResourceProduced(*kind));
    }
}

fn exec_factory(rg: &mut ResourceGraph, spawned: &mut Vec<EventType>, obj: &Object) {
    if rg.owner_has(obj.id, ResourceKind::Wood, 1) && rg.owner_has(obj.id, ResourceKind::Iron, 1) {
        rg.consume(obj.id, ResourceKind::Wood, 1);
        rg.consume(obj.id, ResourceKind::Iron, 1);
        rg.allocate(ResourceKind::Steel, obj.id, 1);
        spawned.push(EventType::ResourceProduced(ResourceKind::Steel));
    }
}

fn exec_city(rg: &mut ResourceGraph, _spawned: &mut Vec<EventType>, obj: &Object) {
    if rg.owner_has(obj.id, ResourceKind::Steel, 1) {
        rg.consume(obj.id, ResourceKind::Steel, 1);
    }
}

fn exec_nop(_rg: &mut ResourceGraph, _spawned: &mut Vec<EventType>, _obj: &Object) {}

const EXEC_TABLE: [ExecFn; NUM_CLASSES] = [
    exec_producer, // Tree
    exec_producer, // Mine
    exec_factory,  // Factory
    exec_city,     // City
    exec_nop,      // Decoration
];

// ─── World ────────────────────────────────────────────────────────────────────

struct World {
    objects: Vec<Object>,
    routes: CompiledRoutes,
    event_queue: Vec<EventType>,
    resources: ResourceGraph,
    event_log: EventLog,
    total_wakes: u64,
}

impl World {
    fn new() -> Self {
        World {
            objects: Vec::new(),
            routes: CompiledRoutes::new(),
            event_queue: Vec::new(),
            resources: ResourceGraph::new(),
            event_log: EventLog::new(),
            total_wakes: 0,
        }
    }

    fn add_object(&mut self, obj: Object) {
        self.objects.push(obj);
    }

    fn compile(&mut self) {
        self.routes.compile(&self.objects);
    }

    fn emit(&mut self, event: EventType) {
        self.event_queue.push(event);
    }

    /// Process all pending events. This is the only hot loop.
    fn tick(&mut self) -> u64 {
        let events = std::mem::take(&mut self.event_queue);
        let mut woken = 0u64;
        let mut spawned = Vec::new();

        for event in &events {
            let targets = self.routes.targets_for(event);

            // Log this dispatch
            self.event_log.log(event, None, targets);

            for &target_id in targets {
                let obj = &mut self.objects[target_id as usize];
                if obj.state != State::Sleep { continue; }

                obj.state = State::Active;
                obj.wake_count += 1;
                woken += 1;

                // Branchless dispatch via function table
                EXEC_TABLE[obj.class as usize](&mut self.resources, &mut spawned, obj);

                obj.state = State::Sleep;
            }
        }

        // Chain: propagate spawned events
        if !spawned.is_empty() {
            self.event_queue.extend(spawned);
            woken += self.tick();
        }

        self.total_wakes += woken;
        woken
    }
}

// ─── World Builder ────────────────────────────────────────────────────────────

fn build_world(total_objects: u64) -> World {
    let mut world = World::new();

    // 100 trees (odd IDs = Wood producers)
    for _ in 0..100 {
        let id = world.objects.len() as ObjectId;
        world.add_object(Object {
            id,
            class: ObjectClass::Tree,
            state: State::Sleep,
            contract: Contract {
                needs: vec![],
                produces: vec![ResourceKind::Wood],
                wakes_on: vec![EventType::Timer(id)],
            },
            wake_count: 0,
        });
        world.resources.allocate(ResourceKind::Wood, id, 1);
    }

    // 50 mines (even IDs = Iron producers)
    for _ in 0..50 {
        let id = world.objects.len() as ObjectId;
        world.add_object(Object {
            id,
            class: ObjectClass::Mine,
            state: State::Sleep,
            contract: Contract {
                needs: vec![],
                produces: vec![ResourceKind::Iron],
                wakes_on: vec![EventType::Timer(id)],
            },
            wake_count: 0,
        });
        world.resources.allocate(ResourceKind::Iron, id, 1);
    }

    // 20 factories
    for _ in 0..20 {
        let id = world.objects.len() as ObjectId;
        world.add_object(Object {
            id,
            class: ObjectClass::Factory,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceKind::Wood, ResourceKind::Iron],
                produces: vec![ResourceKind::Steel],
                wakes_on: vec![
                    EventType::ResourceProduced(ResourceKind::Wood),
                    EventType::ResourceProduced(ResourceKind::Iron),
                ],
            },
            wake_count: 0,
        });
        // Initial inventory
        world.resources.allocate(ResourceKind::Wood, id, 5);
        world.resources.allocate(ResourceKind::Iron, id, 3);
    }

    // 5 cities
    for _ in 0..5 {
        let id = world.objects.len() as ObjectId;
        world.add_object(Object {
            id,
            class: ObjectClass::City,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceKind::Steel],
                produces: vec![],
                wakes_on: vec![EventType::ResourceProduced(ResourceKind::Steel)],
            },
            wake_count: 0,
        });
        world.resources.allocate(ResourceKind::Steel, id, 2);
    }

    // Fill remaining with decorations
    for _ in world.objects.len() as u64..total_objects {
        let id = world.objects.len() as ObjectId;
        world.add_object(Object {
            id,
            class: ObjectClass::Decoration,
            state: State::Sleep,
            contract: Contract { needs: vec![], produces: vec![], wakes_on: vec![] },
            wake_count: 0,
        });
    }

    world.compile();
    world
}

// ─── Bench helpers ────────────────────────────────────────────────────────────

fn ecs_scan(count: usize) -> u64 {
    let mut active = 0u64;
    for i in 0..count {
        if i % 1000 == 0 { active += 1; }
    }
    active
}

fn bench_dso(world: &mut World, events: Vec<EventType>, label: &str) {
    for e in &events { world.emit(e.clone()); }
    let start = Instant::now();
    let woken = world.tick();
    let elapsed = start.elapsed();
    println!("  [dso]  {woken:>6} woken in {elapsed:?}  [{label}]");
}

fn bench_ecs(label: &str, n: usize) {
    let start = Instant::now();
    let r = ecs_scan(n);
    let elapsed = start.elapsed();
    println!("  [ecs]  visited {n} objects ({r} matched) in {elapsed:?}  [{label}]");
}

fn bench_ecs_repeated(label: &str, n: usize, times: usize) {
    let start = Instant::now();
    let mut total = 0u64;
    for _ in 0..times { total += ecs_scan(n); }
    let elapsed = start.elapsed();
    println!("  [ecs]  visited {n} × {times} ({total} matched) in {elapsed:?}  [{label}]");
}

// ─── Route compilation benchmark ──────────────────────────────────────────────

fn bench_route_lookup(routes: &CompiledRoutes, events: &[EventType], trials: usize) {
    // Flat array lookup (DSO)
    let start = Instant::now();
    for _ in 0..trials {
        for e in events {
            let _t = routes.targets_for(e);
        }
    }
    let flat = start.elapsed();

    // HashMap lookup (baseline) — rebuild a hashmap for comparison
    let mut hm: HashMap<&EventType, &[ObjectId]> = HashMap::new();
    for e in events {
        hm.insert(e, routes.targets_for(e));
    }
    let start = Instant::now();
    for _ in 0..trials {
        for e in events {
            let _t = hm.get(e);
        }
    }
    let hash = start.elapsed();

    println!("  [route] flat: {flat:?}  |  hashmap: {hash:?}  ({trials} × {} lookups)", events.len());
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    println!("═══ DSO v3 — Compiled Routes + Resource Graph + Event Log ═══\n");

    let total = 1_000_000u64;
    let mut world = build_world(total);

    let active_objs = world.objects.iter().filter(|o| o.class != ObjectClass::Decoration).count();
    let sleeping = total as usize - active_objs;
    println!("World: {total} objects ({active_objs} active types, {sleeping} always-sleeping)");
    println!("Routes: {} compiled event types", world.routes.table.len());

    // Resource graph initial state
    println!("Resources: {} records", world.resources.records.len());
    println!();

    // ── 1. Single event ─────────────────────────────────────────────────
    println!("─── 1. Single timer event ───");
    bench_dso(&mut world, vec![EventType::Timer(0)], "1 timer");
    bench_ecs("full scan", total as usize);
    println!();

    // ── 2. 150 events ───────────────────────────────────────────────────
    println!("─── 2. 150 timer events ───");
    let mut evs = Vec::new();
    for i in 0..100 { evs.push(EventType::Timer(i)); }
    for i in 0..50 { evs.push(EventType::Timer(100 + i)); }
    let mut w2 = build_world(total);
    bench_dso(&mut w2, evs.clone(), "150 timers");
    bench_ecs_repeated("150 scans", total as usize, 1);
    println!();

    // ── 3. Chain reaction ───────────────────────────────────────────────
    println!("─── 3. Resource chain (3× Wood → Factory → City) ───");
    let chain = vec![
        EventType::ResourceProduced(ResourceKind::Wood),
        EventType::ResourceProduced(ResourceKind::Wood),
        EventType::ResourceProduced(ResourceKind::Wood),
    ];
    let mut w3 = build_world(total);
    bench_dso(&mut w3, chain.clone(), "3 Wood events");
    println!();

    // ── 4. Route compilation benchmark ──────────────────────────────────
    println!("─── 4. Route lookup benchmark ({lookups} lookups × {trials} trials) ───",
        lookups = evs.len(), trials = 1000);
    bench_route_lookup(&world.routes, &evs, 1000);
    println!();

    // ── 5. Event log ────────────────────────────────────────────────────
    println!("─── 5. Event log stats ───");
    println!("  Entries: {}", world.event_log.entries.len());
    println!("  Last hash: {:x}", world.event_log.last_hash);
    println!("  Chain valid: {}", world.event_log.verify());

    // Show last few log entries
    for entry in world.event_log.entries.iter().rev().take(3).rev() {
        println!("  [{:>4}] {:?} → {} targets (prev_hash: {:x})",
            entry.seq, entry.event_type, entry.targets.len(), entry.prev_hash);
    }
    println!();

    // ── Summary ──────────────────────────────────────────────────────────
    println!("─── Summary ───");
    let never = world.objects.iter().filter(|o| o.wake_count == 0).count();
    println!("Objects that never woke: {never} ({}%)", never * 100 / world.objects.len());

    // Resource summary
    println!("Final resources:");
    println!("  Wood:  {}", world.resources.count_by(ResourceKind::Wood));
    println!("  Iron:  {}", world.resources.count_by(ResourceKind::Iron));
    println!("  Steel: {}", world.resources.count_by(ResourceKind::Steel));

    println!("\n═══ Done ═══");
}
