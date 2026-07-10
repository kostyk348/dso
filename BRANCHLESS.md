# DSO: Branchless by Design

> *Ветвление — это решение. Решение, принятое слишком поздно.*

---

## Суть

Каждая инструкция `if`, `switch`, `match`, `??`, `?.`, ветвь тернарного оператора, цикл с неизвестным числом итераций — это **решение, принятое в Runtime**.

DSO утверждает: любое решение, которое можно принять заранее, должно быть принято заранее. Следовательно, **каждое ветвление в hot path — это либо ошибка архитектуры, либо незаконченная работа**.

Цель: **ноль ветвлений в критическом пути исполнения.**

---

## Что заменяет ветвления

### 1. Match → Dispatch Table

Вместо:

```rust
match obj.class {
    Tree => { /* ... */ }
    Factory => { /* ... */ }
    City => { /* ... */ }
}
```

Используем таблицу функций, скомпилированную на этапе инициализации:

```rust
type ActionFn = fn(&mut Object, &Event);

const DISPATCH: [[ActionFn; MAX_EVENTS]; MAX_TYPES] = build_dispatch();

// hot path — ноль ветвлений
DISPATCH[obj.type_id][event.type_id](obj, event);
```

### 2. Polling → Event Routing

Вместо:

```rust
for obj in world {
    if obj.has_event() {  // branch
        process(obj);
    }
}
```

Используем предварительно построенный граф маршрутов. Событие доставляется напрямую получателю. Цикл обхода отсутствует — есть только прямая адресация.

### 3. Type Checks → Compile-time Dispatch

Вместо:

```rust
if let Some(damage) = entity.get::<Damage>() {  // branch
    // ...
}
```

Каждый объект уже знает свою роль. Action table скомпилирована. Проверка типов заменена индексом.

### 4. State Machines → State Tables

Вместо:

```rust
match state {
    State::Idle => { if event == Enter { state = Active; } }
    State::Active => { /* ... */ }
}
```

Таблица переходов, где `next_state = TRANSITIONS[state][event]`. Ноль ветвлений.

### 5. Null/Option Checks → Pre-allocated Resources

Вместо:

```rust
if let Some(resource) = obj.get_resource() {  // branch
    use(resource);
}
```

Ресурс гарантирован контрактом. Если он не может быть доступен — объект не будет разбужен. Проверка вынесена в этап верификации до исполнения.

---

## Иерархия ветвлений и их устранение

| Тип ветвления | Пример | DSO-замена |
|---|---|---|
| Dispatch | `match type` | Action table (indirect call) |
| Polling | `if has_event` | Event routing graph |
| State | `if state == X` | State transition table |
| Null-check | `if let Some(x)` | Pre-allocated, contract-guaranteed |
| Resource | `if resource.available()` | Pre-verified before wake |
| Loop | `for obj in all_objects` | Direct event delivery to N targets |
| Boundary | `if i < len` | Known-size arrays, compile-time |

Каждый тип ветвления заменяется **данными, скомпилированными заранее**.

---

## Почему indirect call — не ветвление

Indirect call (`call *rax`) — это передача управления, а не ветвление. CPU не спекулирует (или спекулирует лучше, чем на ветви). Branch misprediction penalty отсутствует — цена только I-cache и BTB.

Разница на Skylake:

| Инструкция | Цена при mispredict |
|---|---|
| `jcc` (branch) | ~15-20 cycles |
| `call *rax` (indirect) | ~1-2 cycles (BTB hit) |

Indirect call — допустимая цена за устранение ветвления.

---

## Branchless в прототипе DSO

В `dso_proto` ветвления в hot path заменены:

1. **Event → Target** — HashMap вместо линейного поиска
2. **Object → Action** — table-driven dispatch вместо `match`
3. **State check** — объект либо Sleep (пропускаем), либо Active (исполняем). Только одна проверка на входе.
4. **Resource availability** — проверяется на этапе `tick()` ДО исполнения, а не во время

Следующий шаг: вынести resource check в этап верификации перед wake.

---

## Измерения

Бранчлесс подход даёт измеримые преимущества когда:

- **Частота ветвления высокая** (>50% объектов не проходят проверку)
- **Предиктор не справляется** (случайные данные, паттерн меняется)
- **Pipeline deep** (современные CPU: 14-19 стадий)
- **Code footprint большой** (BTB не хватает)

Классический пример: `Update()` в ECS, где 99% объектов не имеют нужного компонента.

DSO устраняет ветвление не аппаратными трюками, а **изменением модели исполнения**: объект не проверяется — он либо спит и не существует для CPU, либо активен и гарантированно должен выполнить действие.

---

## Связь с принципами DSO

| Принцип | Как устраняет ветвление |
|---|---|
| **Determinism First** | Решения приняты заранее → нечего ветвить |
| **Compile Knowledge** | Данные вместо логики → таблицы вместо if |
| **Global Planning** | Граф вместо цикла → адресация вместо обхода |
| **Verify Early** | Гарантии до исполнения → без проверок в рантайме |
| **Sleep By Default** | Мёртвые объекты не проверяются → ноль циклов |
| **Local Execution** | Каждый объект знает свою роль → dispatch протабулирован |

---

## Вывод

**Branchless — не оптимизация. Это следствие.**

Когда система спроектирована по DSO, она естественным образом не содержит ветвлений в критическом пути. Runtime не выбирает — runtime исполняет.

Ветвление — симптом того, что решение отложено до Runtime.
