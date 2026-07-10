# DSO Game Engine Core — Specification v0.1

> *Детерминированный событийно-управляемый игровой Runtime.*

---

## 1. Концепция

DSO Game Engine Core — это не игровой движок в классическом смысле. Это **Runtime для объектов**, где:

- каждый объект по умолчанию спит
- объект просыпается только по событию
- объект исполняется локально (нет глобального `Update()`)
- все знания о мире скомпилированы заранее (графы, контракты, маршруты)
- рендеринг — независимый потребитель состояния, а не часть логики

### Сравнение с ECS

| | ECS | DSO |
|---|---|---|
| Единица | Entity (int) | Object (int + type + contract) |
| Логика | System::Update() на каждой entity | Wake → Execute → Sleep |
| Активность | Всегда все | Только затронутые |
| Связи | Компоненты (данные) | Контракты + зависимости |
| Планирование | Runtime (фильтрация запросов) | Compile-time (граф) |
| Детерминизм | Опционально | Первичен |

---

## 2. Object Model

### 2.1. Определение объекта

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

### 2.2. Контракт

Контракт — декларация поведения: "что нужно, что даю, на что реагирую".

```rust
struct Contract {
    needs: Vec<ResourceSlot>,   // Iron, Wood, ...
    produces: Vec<ResourceSlot>,// Steel, Damage, ...
    wakes_on: Vec<EventPattern>,// IronChanged, PowerChanged
}
```

Контракт — первичный источник поведения. Код — только реализация.

### 2.3. Состояния

```
  ┌──────────┐
  │  Sleep   │ ──── событие ────▶ ┌──────────┐
  └──────────┘                    │  Active  │
         ▲                        └────┬─────┘
         │                             │
         │                             ▼
         │                      ┌──────────────┐
         │                      │  Execution    │
         │                      └──────┬───────┘
         │                             │
         │                   ┌─────────┴──────────┐
         │                   ▼                    ▼
         │            ┌──────────┐        ┌──────────┐
         │            │  Sleep   │        │ Waiting  │ ──► (ждёт ресурс)
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

### 3.1. События

Событие — единственная причина пробуждения. Событие имеет тип, источник, цель, данные.

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

### 3.2. Распространение

События распространяются по **Event Graph** — скомпилированному графу маршрутов.

```
  TreeFelled ──▶ Wood (зависит от Tree)
                      │
                      ▼
               Factory (зависит от Wood)
                      │
                      ▼
                Steel (зависит от Factory)
```

Граф строится на этапе компиляции. Runtime не вычисляет маршруты — только следует им.

### 3.3. Очередь событий

Глобальной очереди нет. Каждый объект имеет inbound-канал. События доставляются по графу напрямую от источника к получателю.

```
  Источник ──event──▶ Object A ──event──▶ Object B
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

1. **Wake** — объект получил событие, проверен контракт (нужные ресурсы есть?)
2. **Verification** — контракт проверен до исполнения
3. **Execute** — выполнение действий
4. **Sleep** — объект засыпает до следующего события
5. **Error** — если контракт не выполнен, активируется fallback

### 4.2. Время

Время — обычное событие. Система не тикает. Если следующий таймер через 5 минут, система делает pause на 5 минут.

```rust
struct TimerEvent {
    target: ObjectId,
    at: u64,              // timestamp
    repeat: Option<u64>,  // interval, если периодический
}
```

Нет `Update()` — есть `TimerExpired`.

---

## 5. Graph Compilation

### 5.1. Этап сборки

Перед запуском (на этапе инициализации или загрузки уровня) строится карта графов:

1. **Object Graph** — все объекты и их типы
2. **Dependency Graph** — кто от кого зависит (направленные связи)
3. **Event Graph** — куда идут события каждого типа
4. **Resource Graph** — кто владеет какими ресурсами
5. **Lifetime Graph** — порядок создания и уничтожения

### 5.2. Формат

Графы компилируются в плоские таблицы для zero-cost runtime-доступа:

```rust
struct CompiledGraph {
    objects: Vec<CompiledObject>,
    event_routes: Vec<(EventType, ObjectId)>,   // lookup table
    dependents: Vec<(ObjectId, ObjectId)>,      // кто за кем следит
    resource_owners: Vec<(ResourceId, ObjectId)>,
}
```

---

## 6. Resource Management

### 6.1. Владение

Каждый ресурс имеет ровно одного владельца.

```rust
struct Resource {
    id: ResourceId,
    owner: ObjectId,
    kind: ResourceKind,   // Memory, GpuBuffer, FileHandle, Socket, ...
    size: u64,
}
```

### 6.2. Передача

Ресурсы передаются через события или контракты. Никакого скрытого shared state.

---

## 7. Presentation

### 7.1. Рендеринг — подписчик

Рендерер — независимый объект, который подписан на события изменения состояния.

```
  GameStateChanged ──▶ Renderer (читает состояние, рисует)
```

Рендерер не влияет на симуляцию. Он может работать на любой частоте (30fps, 60fps, 144fps), симуляция от этого не зависит.

### 7.2. View vs Model

```
  Object (модель) ──event──▶ View (презентация)
       │                           │
       │                    ┌──────┴──────┐
       │                    │  UI / 3D /  │
       │                    │  Sound / FX │
       │                    └─────────────┘
       │
  (живёт своей жизнью, спит/просыпается)
```

---

## 8. Verification

### 8.1. Pre-execution

Перед выполнением действия проверяется:

- все ли зависимости удовлетворены?
- все ли необходимые ресурсы доступны?
- не нарушает ли действие контракты?

### 8.2. Post-execution

После выполнения:

- проверить, что результаты соответствуют контракту
- если ошибка — активировать Fallback

### 8.3. Fallback

```rust
enum Fallback {
    Retry,          // повторить
    Skip,           // пропустить, продолжить
    Revert,         // откатить состояние
    Terminate,      // уничтожить объект
    PropagateError, // отправить Event::Error подписчикам
}
```

---

## 9. MVP: Colony Sim Core

### 9.1. Что моделируем

Упрощённый colony simulator (Factorio/RimWorld-lite):

- `Tree` — ресурс, восстанавливается со временем
- `Mine` — добывает руду
- `Factory` — потребляет ресурсы, производит продукты
- `Transporter` — перемещает ресурсы между объектами
- `Player` — ставит цели
- `City` — потребляет продукты, растёт

### 9.2. Объекты и контракты

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

### 9.3. Поток событий

```
  Timer(10s) ──▶ Tree ──event: WoodProduced──▶ Factory (контракт: Wood)
                                                   │
                                                   ▼
                                              (ждёт Iron)
                                                   │
  Timer(5s) ──▶ Mine ──event: IronProduced───▶ Factory (контракт: Iron)
                                                   │
                                                   ▼
                                              Produces Steel
                                                   │
                                                   ▼
                                        event: SteelProduced
                                                   │
                                                   ▼
                                              City (контракт: Steel)
```

### 9.4. Метрики MVP

| Метрика | ECS-подход | DSO-подход |
|---|---|---|
| Активных объектов | 100% | ~0.01–1% |
| CPU на кадр | O(N) | O(k), k = активные |
| Предиктивность | низкая | высокая |
| Граф причин | неявный | явный |

---

## 10. Дорожная карта

### Phase 0: Rust prototype (текущая)
- Event loop без глобального `Update()`
- 3 типа объектов: Tree, Factory, City
- Контракты через дефинишены
- Бенчмарк: 1M объектов, все спят, одно событие

### Phase 1: Полнота
- Все 5 графов (Dependency, Event, Resource, Lifetime, Memory)
- Verification pre/post-execution
- Fallback-система
- Таймеры как события

### Phase 2: Интеграция
- DSO-IR экспорт
- Adapter для Godot / Bevy
- Инкрементальное сохранение состояния

### Phase 3: Distribution
- Event Graph может跨越 машины
- Dependency Graph распределённый
- Deterministic lockstep для multiplayer
