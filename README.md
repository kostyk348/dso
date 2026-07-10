# DSO — Deterministic Streaming Object Runtime

> **Любое решение, которое можно принять заранее, должно быть принято заранее.**

DSO — это **методология построения вычислительных систем**, основанная на пяти принципах:

1. **Determinism First** — любое решение заранее
2. **Compile Globally** — планировать до исполнения
3. **Verify Early** — проверять до действий
4. **Execute Locally** — только затронутые объекты
5. **Sleep By Default** — нет причины = нет исполнения

---

## Структура репозитория

```
dso/
├── MANIFEST.md              # Философия: 17 принципов DSO
├── BRANCHLESS.md            # Branchless by Design
├── ENGINE_SPEC.md           # Спецификация DSO Game Engine Core
├── proto/                   # Rust-прототип: событийная симуляция
│   └── src/main.rs          #   1M объектов, 99% спят
└── ai/                      # DSO-AI Runtime: LLM inference engine
    ├── dso_runtime.cpp      #   C++ engine (zero-malloc, INT8, AVX2)
    ├── run.py               #   CLI wrapper
    ├── tok.py               #   BPE tokenizer
    └── quantize.py          #   INT8 quantization
```

---

## Ключевые результаты прототипа

```
1 событие:   DSO  11µs   vs  ECS 380µs     (×35)
150 событий: DSO 110µs   vs  ECS  58ms     (×530)

1,000,000 объектов
    999,975 ни разу не проснулись (99.98%)
```

Прототип демонстрирует: **Sleep By Default** — объекты не обновляются в цикле, а спят до события.

---

## Что ещё внутри

| Документ | Суть |
|---|---|
| [`MANIFEST.md`](MANIFEST.md) | 17 принципов — от детерминизма до универсального Runtime |
| [`BRANCHLESS.md`](BRANCHLESS.md) | Почему ветвление — симптом решения, принятого слишком поздно |
| [`ENGINE_SPEC.md`](ENGINE_SPEC.md) | Архитектура DSO Game Engine Core: объекты, контракты, графы |
| [`proto/`](proto/) | Rust-прототип: сравнение branchy vs branchless, верификация контрактов |
| [`ai/`](ai/) | C++ LLM inference engine: streaming from disk, INT8, static arenas |

---

## Запуск прототипа

```bash
cd proto && cargo run --release
```

---

> **Важное замечание.** Всё это пока представляет собой архитектурную гипотезу, а не доказанную теорию. Именно работающие прототипы и бенчмарки смогут подтвердить или опровергнуть эту гипотезу.
