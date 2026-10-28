# Estimates

udo estimates how long each task will take and shows that next to the
time you tracked on it: per task, and summed over a container. You can set
an estimate yourself; without one, udo learns it from the tasks next to
it. Most examples below come from the dataset of
`scripts/seed-testdata.sh`.

## Where a task's estimate comes from

1. **Yours**, if you set one: `udo edit "lab 3" --estimate 2h` (`""`
   removes it), or the estimate field of the edit form (`e`) in the TUI.
   Yours always wins. udo's own estimate is still shown next to it, so
   you can compare the two.
2. **Learned**, from the other tasks in the same container: an average
   of their tracked time, blended with a starting value (the *prior*):
   - done tasks count fully, with all their tracked time;
   - open tasks count only once they are already over what the done
     tasks give, and then at half weight (their time so far is only an
     "at least");
   - tasks without tracked time do not count;
   - tasks you removed still count: their sessions are kept;
   - the prior counts like 3 tasks of its length. So with few done
     tasks the estimate stays close to the prior, and it moves towards
     your real times as more tasks get done.
3. **Nothing** (`-`): no estimate of yours, no prior, and no done task to
   learn from.

The prior is, in this order:

1. the container's own `estimate` setting
   (`udo settings set uni/algorithms estimate=3h`);
2. else what the parent learned from its tasks outside this container,
   blended with the parent's own prior (`from uni's tasks`);
3. else, when the parent has no such tasks, the parent's prior itself.
   This is how a setting higher up reaches down (`from uni`);
4. else nothing: the root without a setting.

As a formula: `(3·prior + Σ w·tracked) / (3 + Σ w)`, with `w = 1` for done
tasks and `w = ½` for counted open ones. Example: `sheet-4` in
`uni/algorithms` (setting 3h). The only other task with time is `sheet-3`,
open, with 3h55: more than 3h, so it counts at half weight.
`(3·180 + ½·235) / 3.5 = 188 min = 3h08`.

### Done tasks keep the estimate of their start

A done task's learned estimate is **frozen at its first session** (no
session: at its creation): it is what udo estimated when you started the
task, computed on what was known then. Tasks done later do not change it,
and neither does changing the container's `estimate` setting afterwards.
So `result` compares the estimate you started with to the time it really
took. A task finished before udo had anything to learn from has no
estimate (`-`) and no `result`. Your own estimate is never frozen: it is
always the current value.

Correcting the task's own sessions moves it: editing, cutting or
deleting its first session freezes the estimate anew at the session that
is first now. With no session left, the estimate at its creation counts.

Work logged afterwards counts as of when it happened. A session added
back-dated is a task's first session even if it lies before the task was
created, and the task counts as existing from then on. So a task created
today for yesterday's work is frozen at yesterday's session, and learns
from what was done before it.

For that, "done" can be back-dated too:

```sh
udo done "lab 3" --at "2026-10-17 18:00"   # or --at -2h
```

`--at` refuses a time in the future, a time before one of the task's
sessions ended, a task whose timer still runs, and a task already done.
`-f` / `--force` stops the running timer at that time, or changes the done
time of a task already done. A timer that started after `--at` is never
stopped: the session would end before it began.

## A task

The TUI details (Info tab) and `udo show` have the same rows. The TUI
calls the tracked row `duration`, `udo show` calls it `tracked`.

```
estimate:    3h05 (learned from 1 open task)
tracked:     3h55 in 2 sessions
left:        over by 50m
```

| Row | Means |
|-----|-------|
| `estimate` | The estimate udo goes by, and why it is that value (see the table below). |
| `tracked` / `duration` | All sessions on the task added up, and how many there are. A running timer counts up to now (`, running`). |
| `left` | Open tasks: estimate minus tracked (`1h15`), or `over by 50m` once you are past the estimate (red in the TUI). `-`: no estimate. |
| `result` | Done tasks with an estimate, instead of `left`: `over by 20m`, `under by 10m` or `as estimated` (over is red in the TUI). |

What the brackets after the estimate mean:

| Text | Means |
|------|-------|
| `1h30 (yours, udo: 1h20)` | Your own estimate, and udo's learned one for comparison. |
| `1h30 (yours)` | Your own; udo has nothing to learn from yet. |
| `(learned from 3 done tasks)` | Blended from 3 done tasks in the same container (and the prior). |
| `(learned from 2 open tasks)` | Only open tasks that are already over the prior. |
| `(learned from 5 done, 1 open)` | Both. |
| `(from uni)` | No task to learn from in its own container yet: the `estimate` setting of `uni` (this container's or one above). |
| `(from uni's tasks)` | No task to learn from in its own container yet: what the other tasks in `uni` give. |
| `-` | No estimate. |

## A container

A container shows sums over **every task below it** (at any depth). The
`done` row here is made up: `uni` in the seed data has no done task with
an estimate yet.

```
estimated:   14h54 (5 of 7 tasks)
tracked:     11h45 in 7 sessions, running
left:        7h11 (2 over)
done:        7h estimated, 8h10 tracked (over by 1h10)
```

| Row | Means |
|-----|-------|
| `estimated` | The estimates of the tasks below added up, and how many of them have one (`5 of 7 tasks`). Tasks without an estimate are left out of every sum below. `- (0 of 7 tasks)`: none has one. |
| `tracked` / `duration` | All time tracked on the tasks below, **with** the tasks that have no estimate. If not every task has an estimate (`5 of 7`), do not compare it with `estimated` directly. |
| `left` | What is still to do: for each open task with an estimate, its estimate minus its tracked time, added up. A task already over its estimate adds 0 and is counted in `(2 over)`. Missing: no task has an estimate. |
| `done` | The done tasks with an estimate: their (frozen) estimates against their tracked time, and the `result` of the two sums. Missing: no such task. |

### Two different numbers for a container

- `estimated` (TUI details, `udo show`, `udo ls -t`) is a **sum**: how
  long all its tasks take together.
- `udo estimate <container>` is a **typical task** in it: what a new task
  there would get, learned from every task in its subtree.
  `3h11 for a task in uni/algorithms`.

## `udo estimate`

Why a task has the estimate it has:

```
$ udo estimate sheet-4
3h08 for uni/algorithms/sheet-4
  learned from 1 open task (over the estimate, half weight)
  prior: 3h, set on uni/algorithms
```

- The first line: the estimate. With your own estimate it reads
  `2h for … (yours)`, and the next line is `udo: 3h08`, udo's learned one.
- `done: as estimated on …`: only for done tasks: when the estimate was
  frozen (the first session).
- `learned from …`: how many done and open tasks counted.
- `prior: …`: the starting value: `set on <container>` (its setting) or
  from the tasks of a container higher up.
- Nothing to learn from: one line saying so, and how to set a prior.

## `udo ls -t`

`-t` / `--time` adds a `tracked/estimate` column; without it, `ls` looks as
it always did.

```
uni/                          11h45/14h54   workspace
  algorithms/                 7h30/9h24     project
    sheet-3                   3h55/3h05     started     2026-10-17 12:00  overdue
  compilers/                  4h15/5h30     project
    setup-toolchain           2h45/-        done        2026-10-13 12:00
```

A task: its tracked time and estimate. A container: `tracked` and
`estimated` from above (tracked over all its tasks, the estimate only
over those that have one). `-`: no estimate.

## JSON

`--json` gives minutes as numbers, and `null` where there is no value.

- `udo show` on a task: `minutes` (tracked), `estimate` (what udo goes
  by), `own`, `learned`, and `diff` (tracked minus estimate, so positive
  means over).
- `udo show` on a container: `minutes`, `estimated`, `estimated_tasks`,
  `tasks`, `left`, `over`, `done_estimated`, `done_actual`.
- `udo ls -t`: per row `minutes`, and `estimate` (tasks) or `estimated`
  (containers).
- `udo estimate`: `own`, `minutes` (always the learned one), `done`,
  `open`, `prior` (`source`: `setting` or `parent`, with `from` and
  `minutes`), `as_of` (when a done task's estimate was frozen),
  `container` (`true`: a typical task in it).

## When the TUI updates them

The TUI computes every estimate at once, not each time it draws. It does
that after every change you make in the TUI, every minute, when a timer is
started or stopped from somewhere else (the CLI, a tmux hook), and when
you come back from a run config. Moving the cursor or typing in a form
recomputes nothing. Other changes made outside the TUI show up within a
minute.
