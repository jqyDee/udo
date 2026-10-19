# Estimates

How long a task will take, learned from the recorded time (`src/estimate/`).
The module is pure: `Core` builds a snapshot (`History`), the estimators
only read it. The math (the average, its prior, weights) is explained in
the doc comments of `src/estimate/average.rs`.

```mermaid
classDiagram
    class History {
        tasks : Vec~TaskRecord~
        containers
        build(tree, sessions, now)$
        as_of(t) History
    }
    class TaskRecord {
        id, container : NodeId
        created_at : Time
        done_at : Option~Time~
        actual : Minutes
        first_session : Option~Time~
        own_estimate : Option~Minutes~
        frozen_at() Time
    }
    class Estimator {
        <<trait>>
        method() str
        version() u32
        estimate(task, History) Option~Estimate~
    }
    class Manual {
        method "manual"
    }
    class Average {
        method "average"
        of_container(container, History)
    }
    class Estimate {
        minutes : Minutes
        basis : Basis
        as_of : Option~Time~
    }
    class Basis {
        <<enum>>
        Own
        Prior(Prior)
        Learned(container, done_tasks, open_tasks, prior)
    }
    class Shown {
        own : Option~Minutes~
        learned : Option~Estimate~
        minutes() Option~Minutes~
    }

    History *-- TaskRecord
    Manual ..|> Estimator
    Average ..|> Estimator
    Estimator ..> History : reads
    Estimator ..> Estimate
    Estimate *-- Basis
    Shown o-- Estimate : learned
```

- **`History`** is the tree's tasks plus their sessions, read once
  (`Core::history`). Deleted tasks are in it too, from their sessions.
  `as_of(t)` cuts it back to what was known at `t`: later tasks dropped,
  sessions cut at `t`, tasks done later count as open.
- **`Average`** blends a container's tasks with a prior (the container's
  `estimate` setting, else what the parent's tasks give). `Manual` gives
  the task's own estimate. Both are `Estimator`s; method and version go
  into every recorded row.
- **A done task's learned estimate is frozen** at its first session
  (`TaskRecord::frozen_at`): computed on `history.as_of(frozen_at)`, so it
  never changes afterwards and can be compared with the actual time.

## What udo shows and goes by

```mermaid
flowchart TD
    node["of_node(node, history)"] --> kind{task or container?}
    kind -- container --> pooled["Shown { own: None,<br/>learned: Average.of_container }<br/>(a typical task in it)"]
    kind -- task --> both["Shown { own: Task.estimate,<br/>learned: Average.estimate }"]
    both --> done{done?}
    done -- yes --> frozen["learned on history.as_of(frozen_at)"]
    done -- no --> today["learned on today's history"]
    both --> minutes["Shown::minutes(): yours, else learned<br/>(used for left)"]
    pick["pick(task, history)<br/>[Manual, Average]: the first that answers"] --> row["recorded row:<br/>estimate + its estimator"]
```

- **`of_node`** gives both values (`Shown`), so the TUI and `udo estimate`
  can show yours next to what udo learned.
- **`pick`** is the one place the order "yours, else learned" lives; it
  also returns the estimator, whose method and version go into the row.
  `Shown::minutes` follows the same order.

## Recording

`Core` writes an estimate row (see [storage.md](storage.md#estimate-rows))
after an action, never before it. A failed write never undoes the action:
it becomes a warning (`estimate not recorded: …`) that the CLI prints to
stderr and the TUI shows as a toast.

| Moment | When | The row |
|--------|------|---------|
| `Created` | `Core::create` of a task | `pick` as of its creation, reason `created` |
| `Started` | its first session (`start`, `track start`, an added session) | `pick` as of that session, reason `started`; later sessions: no row |
| `Edited` | `Core::edit` changed the own estimate | set or changed: `Manual`, reason `manual`; removed: the learned one as shown, reason `cleared` |

No row for containers, when nothing can be estimated yet, or when it would
repeat the task's latest row.
