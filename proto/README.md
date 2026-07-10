# DSO Prototype: Event-Driven Simulation

Минимальная демонстрация философии DSO.

## Ключевые результаты

### 1M объектов, 99.98% никогда не проснулись

```
─── 1. Single timer event ───
  DSO:    121 objects woken in 11µs
  ECS:    visited 1M objects in 380µs       (35× дольше)

─── 2. 150 timer events ───
  DSO:    3450 objects woken in 110µs
  ECS:    visited 1M × 150 in 58ms          (530× дольше)

─── 3. Resource chain (3 Wood events) ───
  DSO:    360 objects woken in 20µs
```

## Что демонстрирует

- **Sleep By Default** — 999,975 объектов ни разу не проснулись
- **Nothing Executes Without Reason** — только события будят объекты
- **World Is A Dependency Graph** — Tree→Factory→City
- **Branchless by Design** — dispatch через таблицу функций (ноль `match` в hot path)
- **Verify Early** — pre/post проверка контрактов до/после исполнения
- **Compile Knowledge** — Event Graph строится при инициализации

## Branchless vs Branchy

```
Dispatch microbenchmark (10M calls):
  Branchless (table):  17ns/call
  Branchy    (match):  12ns/call
```

`match` на `#[repr(u8)]` enum компилируется в jump table — разница с функциональной таблицей минимальна. Настоящий выигрыш: **структурный** — мы не обходим 999k спящих объектов.

## Архитектура

```
Event → Route Table → Object (Wake → Execute → Sleep)
                        │
                        ▼
                   Action Table (branchless dispatch)
                        │
                        ▼
                   New Events → Propagate
```

## Запуск

```bash
cargo run --release
```
