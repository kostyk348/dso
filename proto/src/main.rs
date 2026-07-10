use std::collections::HashMap;
use std::time::Instant;

// ─── Core Types ───────────────────────────────────────────────────────────────

type ObjectId = u64;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ResourceType { Wood, Iron, Steel }

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum EventType {
    Timer(u64),
    ResourceProduced(ResourceType),
    ResourceConsumed(ResourceType),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State { Sleep, Active }

// Discriminants must be 0..N for table indexing
#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
enum ObjectClass {
    Tree       = 0,
    Mine       = 1,
    Factory    = 2,
    City       = 3,
    Decoration = 4, // never wakes — exists only for headcount
}
const NUM_CLASSES: usize = 5;

#[derive(Clone, Debug)]
struct Contract {
    needs: Vec<ResourceType>,
    produces: Vec<ResourceType>,
    wakes_on: Vec<EventType>,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum Fallback {
    Skip,
    Revert,
    Terminate,
}

struct Object {
    id: ObjectId,
    class: ObjectClass,
    state: State,
    contract: Contract,
    inventory: HashMap<ResourceType, u64>,
    wake_count: u64,
    saved_inventory: HashMap<ResourceType, u64>, // for Revert fallback
}

// ─── Branchless Dispatch Table ────────────────────────────────────────────────

type ExecFn = fn(&mut Object, &mut Vec<EventType>);

fn exec_producer(obj: &mut Object, spawned: &mut Vec<EventType>) {
    for r in &obj.contract.produces {
        *obj.inventory.entry(*r).or_insert(0) += 1;
        spawned.push(EventType::ResourceProduced(*r));
    }
}

fn exec_factory(obj: &mut Object, spawned: &mut Vec<EventType>) {
    let can_produce = obj.contract.needs.iter().all(|r| {
        obj.inventory.get(r).copied().unwrap_or(0) > 0
    });
    if !can_produce { return; }
    for r in &obj.contract.needs {
        *obj.inventory.get_mut(r).unwrap() -= 1;
        spawned.push(EventType::ResourceConsumed(*r));
    }
    for r in &obj.contract.produces {
        *obj.inventory.entry(*r).or_insert(0) += 1;
        spawned.push(EventType::ResourceProduced(*r));
    }
}

fn exec_city(obj: &mut Object, _spawned: &mut Vec<EventType>) {
    let can_consume = obj.contract.needs.iter().all(|r| {
        obj.inventory.get(r).copied().unwrap_or(0) > 0
    });
    if !can_consume { return; }
    for r in &obj.contract.needs {
        *obj.inventory.get_mut(r).unwrap() -= 1;
    }
}

fn exec_nop(_obj: &mut Object, _spawned: &mut Vec<EventType>) {}

const EXEC_TABLE: [ExecFn; NUM_CLASSES] = [
    exec_producer, // Tree    — same as Mine
    exec_producer, // Mine
    exec_factory,  // Factory
    exec_city,     // City
    exec_nop,      // Decoration
];

// ─── Verification ─────────────────────────────────────────────────────────────

fn verify_pre(obj: &Object) -> bool {
    obj.contract.needs.iter().all(|r| {
        obj.inventory.get(r).copied().unwrap_or(0) > 0
    })
}

fn verify_post(obj: &Object) -> bool {
    // Post-condition: at least one resource should have changed
    // (for objects that produce or consume)
    if obj.contract.produces.is_empty() && obj.contract.needs.is_empty() {
        return true;
    }
    // We can't fully verify without saved state — this is a placeholder
    true
}

// ─── World ────────────────────────────────────────────────────────────────────

struct World {
    objects: Vec<Object>,
    event_routes: HashMap<EventType, Vec<ObjectId>>,
    event_queue: Vec<EventType>,
    total_wakes: u64,
}

impl World {
    fn new() -> Self {
        World {
            objects: Vec::new(),
            event_routes: HashMap::new(),
            event_queue: Vec::new(),
            total_wakes: 0,

        }
    }

    fn add_object(&mut self, obj: Object) {
        for ev in &obj.contract.wakes_on {
            self.event_routes
                .entry(ev.clone())
                .or_default()
                .push(obj.id as ObjectId);
        }
        self.objects.push(obj);
    }

    fn compile(&mut self) {
        for routes in self.event_routes.values_mut() {
            routes.sort();
            routes.dedup();
        }
    }

    fn emit(&mut self, event: EventType) {
        self.event_queue.push(event);
    }

    /// Process events using BRANCHLESS dispatch (function pointer table).
    /// Returns number of objects woken.
    fn tick_branchless(&mut self) -> u64 {
        let events = std::mem::take(&mut self.event_queue);
        let mut woken = 0u64;
        let mut spawned = Vec::new();

        for event in &events {
            if let Some(targets) = self.event_routes.get(event) {
                for &target_id in targets {
                    let obj = &mut self.objects[target_id as usize];
                    if obj.state != State::Sleep {
                        continue;
                    }
                    obj.state = State::Active;
                    obj.wake_count += 1;
                    woken += 1;

                    // ─── Pre-verification ────────────────────────────────
                    let pre_ok = verify_pre(obj);

                    // ─── Branchless dispatch (no match!) ─────────────────
                    if pre_ok {
                        // Save for potential revert
                        obj.saved_inventory = obj.inventory.clone();
                        EXEC_TABLE[obj.class as usize](obj, &mut spawned);
                    }

                    // ─── Post-verification + fallback ────────────────────
                    if pre_ok && !verify_post(obj) {
                        // Contract violated — revert
                        obj.inventory = std::mem::take(&mut obj.saved_inventory);
                    }

                    obj.state = State::Sleep;
                }
            }
        }

        self.event_queue.extend(spawned);
        if !self.event_queue.is_empty() {
            woken += self.tick_branchless();
        }

        self.total_wakes += woken;
        woken
    }

    /// Process events using BRANCHY dispatch (match on class).
    /// Same logic — only dispatch differs.
    fn tick_branchy(&mut self) -> u64 {
        let events = std::mem::take(&mut self.event_queue);
        let mut woken = 0u64;
        let mut spawned = Vec::new();

        for event in &events {
            if let Some(targets) = self.event_routes.get(event) {
                for &target_id in targets {
                    let obj = &mut self.objects[target_id as usize];
                    if obj.state != State::Sleep {
                        continue;
                    }
                    obj.state = State::Active;
                    obj.wake_count += 1;
                    woken += 1;

                    let pre_ok = verify_pre(obj);
                    if pre_ok {
                        obj.saved_inventory = obj.inventory.clone();

                        // ─── Branchy dispatch (match) ────────────────────
                        match obj.class {
                            ObjectClass::Tree | ObjectClass::Mine => {
                                for r in &obj.contract.produces {
                                    *obj.inventory.entry(*r).or_insert(0) += 1;
                                    spawned.push(EventType::ResourceProduced(*r));
                                }
                            }
                            ObjectClass::Factory => {
                                let can = obj.contract.needs.iter().all(|r| {
                                    obj.inventory.get(r).copied().unwrap_or(0) > 0
                                });
                                if can {
                                    for r in &obj.contract.needs {
                                        *obj.inventory.get_mut(r).unwrap() -= 1;
                                        spawned.push(EventType::ResourceConsumed(*r));
                                    }
                                    for r in &obj.contract.produces {
                                        *obj.inventory.entry(*r).or_insert(0) += 1;
                                        spawned.push(EventType::ResourceProduced(*r));
                                    }
                                }
                            }
                            ObjectClass::City => {
                                let can = obj.contract.needs.iter().all(|r| {
                                    obj.inventory.get(r).copied().unwrap_or(0) > 0
                                });
                                if can {
                                    for r in &obj.contract.needs {
                                        *obj.inventory.get_mut(r).unwrap() -= 1;
                                    }
                                }
                            }
                            ObjectClass::Decoration => {}
                        }
                    }

                    if pre_ok && !verify_post(obj) {
                        obj.inventory = std::mem::take(&mut obj.saved_inventory);
                    }

                    obj.state = State::Sleep;
                }
            }
        }

        self.event_queue.extend(spawned);
        if !self.event_queue.is_empty() {
            woken += self.tick_branchy();
        }

        self.total_wakes += woken;
        woken
    }
}

// ─── Bench ────────────────────────────────────────────────────────────────────

fn ecs_update_all(count: usize) -> u64 {
    let mut active: u64 = 0;
    for i in 0..count {
        if i % 1000 == 0 {
            active += 1;
        }
    }
    active
}

fn bench(label: &str, world: &mut World, events: Vec<EventType>, branchless: bool) {
    for e in &events {
        world.emit(e.clone());
    }
    let start = Instant::now();
    let woken = if branchless {
        world.tick_branchless()
    } else {
        world.tick_branchy()
    };
    let elapsed = start.elapsed();
    let mode = if branchless { "branchless" } else { "branchy (match)" };
    println!("  [{mode:>11}] {} objects woken in {:?}  [{label}]", woken, elapsed);
}

fn bench_ecs(label: &str, n: usize) {
    let start = Instant::now();
    let r = ecs_update_all(n);
    let elapsed = start.elapsed();
    println!("  [    ecs    ] visited {} objects ({} matched) in {:?}  [{label}]", n, r, elapsed);
}

fn bench_ecs_repeated(label: &str, n: usize, times: usize) {
    let start = Instant::now();
    let mut total = 0u64;
    for _ in 0..times {
        total += ecs_update_all(n);
    }
    let elapsed = start.elapsed();
    println!("  [    ecs    ] visited {} objects x {} ({} matched) in {:?}  [{label}]", n, times, total, elapsed);
}

// ─── World Builder ────────────────────────────────────────────────────────────

fn build_world(total_objects: u64) -> World {
    let mut world = World::new();

    // 100 trees
    for i in 0..100 {
        world.add_object(Object {
            id: world.objects.len() as u64,
            class: ObjectClass::Tree,
            state: State::Sleep,
            contract: Contract {
                needs: vec![],
                produces: vec![ResourceType::Wood],
                wakes_on: vec![EventType::Timer(i)],
            },
            inventory: HashMap::new(),
            saved_inventory: HashMap::new(),
            wake_count: 0,
        });
    }

    // 50 mines
    for i in 0..50 {
        world.add_object(Object {
            id: world.objects.len() as u64,
            class: ObjectClass::Mine,
            state: State::Sleep,
            contract: Contract {
                needs: vec![],
                produces: vec![ResourceType::Iron],
                wakes_on: vec![EventType::Timer(100 + i)],
            },
            inventory: HashMap::new(),
            saved_inventory: HashMap::new(),
            wake_count: 0,
        });
    }

    // 20 factories
    for _ in 0..20 {
        let mut inv = HashMap::new();
        inv.insert(ResourceType::Wood, 5);
        inv.insert(ResourceType::Iron, 3);
        world.add_object(Object {
            id: world.objects.len() as u64,
            class: ObjectClass::Factory,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceType::Wood, ResourceType::Iron],
                produces: vec![ResourceType::Steel],
                wakes_on: vec![
                    EventType::ResourceProduced(ResourceType::Wood),
                    EventType::ResourceProduced(ResourceType::Iron),
                ],
            },
            inventory: inv,
            saved_inventory: HashMap::new(),
            wake_count: 0,
        });
    }

    // 5 cities
    for _ in 0..5 {
        let mut inv = HashMap::new();
        inv.insert(ResourceType::Steel, 2);
        world.add_object(Object {
            id: world.objects.len() as u64,
            class: ObjectClass::City,
            state: State::Sleep,
            contract: Contract {
                needs: vec![ResourceType::Steel],
                produces: vec![],
                wakes_on: vec![EventType::ResourceProduced(ResourceType::Steel)],
            },
            inventory: inv,
            saved_inventory: HashMap::new(),
            wake_count: 0,
        });
    }

    // Fill remaining with decorations
    for _ in world.objects.len() as u64..total_objects {
        world.add_object(Object {
            id: world.objects.len() as u64,
            class: ObjectClass::Decoration,
            state: State::Sleep,
            contract: Contract {
                needs: vec![],
                produces: vec![],
                wakes_on: vec![],
            },
            inventory: HashMap::new(),
            saved_inventory: HashMap::new(),
            wake_count: 0,
        });
    }

    world.compile();
    world
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    println!("═══ DSO Prototype v2 — Branchless by Design ═══\n");

    let total_objects = 1_000_000u64;

    // ─── Warmup: build a fresh world for each benchmark ───────────────────
    // (branchy vs branchless on identical worlds)

    let mut w1 = build_world(total_objects);
    let mut w2 = build_world(total_objects);

    println!("World: {} objects (175 active types, {} decorations)\n",
        total_objects, total_objects - 175);

    // ─── Benchmark 1: Single event (both dispatch methods) ───────────────
    println!("─── 1. Single timer event ───");
    bench("1 tree timer", &mut w1, vec![EventType::Timer(0)], false);
    bench("1 tree timer", &mut w2, vec![EventType::Timer(0)], true);
    bench_ecs("1 pass of 1M", total_objects as usize);
    println!();

    // ─── Benchmark 2: 150 events ─────────────────────────────────────────
    println!("─── 2. 150 timer events (100 trees + 50 mines) ───");
    let mut events = Vec::new();
    for i in 0..100 { events.push(EventType::Timer(i)); }
    for i in 0..50 { events.push(EventType::Timer(100 + i)); }

    let mut w3 = build_world(total_objects);
    bench("150 timers", &mut w3, events.clone(), false);

    let mut w4 = build_world(total_objects);
    bench("150 timers", &mut w4, events.clone(), true);

    bench_ecs_repeated("150 passes", total_objects as usize, 150);
    println!();

    // ─── Benchmark 3: Chain reaction ─────────────────────────────────────
    println!("─── 3. Resource chain (3× Wood → Factory → Steel → City) ───");
    let chain = vec![
        EventType::ResourceProduced(ResourceType::Wood),
        EventType::ResourceProduced(ResourceType::Wood),
        EventType::ResourceProduced(ResourceType::Wood),
    ];

    let mut w5 = build_world(total_objects);
    bench("3 Wood events", &mut w5, chain.clone(), false);

    let mut w6 = build_world(total_objects);
    bench("3 Wood events", &mut w6, chain.clone(), true);
    println!();

    // ─── Benchmark 4: Microbenchmark — pure dispatch cost ────────────────
    println!("─── 4. Dispatch microbenchmark: 10M calls ───");
    const N: usize = 10_000_000;
    let mut dummy_events = vec![];
    let mut dummy_obj = Object {
        id: 0, class: ObjectClass::Tree, state: State::Sleep,
        contract: Contract { needs: vec![], produces: vec![ResourceType::Wood], wakes_on: vec![] },
        inventory: HashMap::new(), saved_inventory: HashMap::new(), wake_count: 0,
    };

    // Branchless
    let start = Instant::now();
    for _ in 0..N {
        EXEC_TABLE[0](&mut dummy_obj, &mut dummy_events);
    }
    let bl_time = start.elapsed();
    dummy_events.clear();

    // Branchy
    let start = Instant::now();
    for _ in 0..N {
        match dummy_obj.class {
            ObjectClass::Tree | ObjectClass::Mine => {
                for r in &dummy_obj.contract.produces {
                    dummy_obj.inventory.entry(*r).or_insert(0);
                }
            }
            _ => {}
        }
    }
    let by_time = start.elapsed();

    println!("  [branchless] {} calls in {:?}  ({:?}/call)", N, bl_time, bl_time / N as u32);
    println!("  [branchy   ] {} calls in {:?}  ({:?}/call)", N, by_time, by_time / N as u32);

    // ─── Summary from last branchless world ──────────────────────────────
    println!();
    println!("─── Summary ───");
    println!("Total objects in world: {}", total_objects);
    let active = w6.objects.iter().filter(|o| o.class != ObjectClass::Decoration).count();
    let sleep = total_objects as usize - active;
    println!("Ever-active: {}  |  Always-sleeping: {} ({}%)", active, sleep, sleep * 100 / w6.objects.len());
    let never = w6.objects.iter().filter(|o| o.wake_count == 0).count();
    println!("Objects that never woke: {} ({}%)", never, never * 100 / w6.objects.len());

    // Print final state of a few objects
    println!();
    println!("─── Sample objects (inventory after all events) ───");
    for obj in w6.objects.iter().take(180).skip(140) {
        let inv: Vec<String> = obj.inventory.iter()
            .map(|(r, c)| format!("{:?}: {}", r, c)).collect();
        let cn = match obj.class {
            ObjectClass::Tree => "Tree", ObjectClass::Mine => "Mine",
            ObjectClass::Factory => "Factory", ObjectClass::City => "City",
            ObjectClass::Decoration => "Deco",
        };
        println!("  [{:>3}] {:>7} — wakes: {:>2} — {}", obj.id, cn, obj.wake_count, inv.join(", "));
    }

    println!("\n═══ Done ═══");
}
