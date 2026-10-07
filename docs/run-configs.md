# Run configs

A run config is a script that opens or sets up a node: nvim in a tmux
session per task, an IDE on the project folder, a template copied into a
new task. udo knows no programs. It finds the script by name, runs it with
the node in environment variables, and hands it the terminal. What the
script starts, and whether it tracks time, is up to the script.

Five working examples are in [`examples/run/`](../examples/run/):
`nvim-tmux`, `tmux`, `idea`, `zed` and `typst-setup` (see
[Examples](#examples)). [`examples/python/`](../examples/python/) has the
same five in Python,
line for line, to compare: any language works. Install one or the other,
not both: `idea.sh` and `idea.py` are both `idea`, and udo refuses a name
that two scripts share.

## Scripts

- Any executable file in the run folder: bash, Python, a binary. The
  interpreter comes from the `#!` line.
- The name is the file name without its extension: `idea.py` is `idea`.
  The language can change without touching any setting.
- It must be executable (`chmod +x`). Subfolders are ignored.
- Two files with the same name (`idea.sh` and `idea.py`) are an error when
  that name is used, not before.

The run folder is `<root>/run` (the root is `~/.config/udo`, or
`$UDO_ROOT`). Elsewhere, for example in a dotfiles repository:

```sh
udo settings set / run_dir=~/dotfiles/udo-run
```

`udo run --list` shows the scripts and the one the current node opens
with.

## Choosing a script

Two container settings, inherited by everything below, like every
setting:

| Setting | Runs | Event |
|---------|------|-------|
| `open_with` | when you open a node | `open` |
| `on_create` | after a task or container is created below | `create` |

```sh
udo settings set uni open_with=nvim-tmux
udo settings set uni on_create=typst-setup
udo settings set uni/sport open_with=none   # switched off below sport
udo settings unset uni/sport open_with      # inherited again
```

`none` switches an inherited script off. An unknown name is an error when
it runs, with the names that exist (`no run config "idae" in … (have:
idea, nvim-tmux)`).

## When scripts run

**Opening** (`open_with`):

- TUI: `o` opens the node at the cursor; `O` picks the script first (the
  `open_with` one preselected and marked `(default)`).
- CLI: `udo run [NODE]`, `udo run NODE --with NAME` for another script.

A container can be opened for one of its open tasks (below it too), so a
script can time it, or on its own, without a task (lecture notes, a
folder without anything to do). udo never picks a task on its own, not
even the only one. The TUI opens a container without open tasks at once;
with some it asks with a picker, the container itself first (`uni (no
task)`, preselected), then the tasks in tree order. The CLI opens the
container on its own unless `--task` names a task below it:

```sh
udo run uni/cs                  # the container, no task
udo run uni/cs --task "lab 3"   # the container, the time on lab 3
```

**Creating** (`on_create`), after the node is saved:

- `udo add task|project|workspace …`; `--no-run` skips the script.
- The TUI's create forms (`t`, `c`) get a row
  `setup ‹ run · skip ›  typst-setup` when there is an `on_create`; `skip`
  creates without running it. The row is what counts: switching
  `on_create` off while the form is open does not make it run.

The node stays whatever the script does.

## What a script learns

It runs in the node's folder (no folder: the nearest container's), with
these variables on top of udo's own environment:

| Variable | Contents |
|----------|----------|
| `UDO_EVENT` | `open` or `create` |
| `UDO_NODE_ID`, `UDO_NODE_NAME` | the opened / created node |
| `UDO_NODE_KIND` | `task` or `container` |
| `UDO_NODE_DIR` | its folder; empty without one |
| `UDO_TASK_ID`, `UDO_TASK_NAME`, `UDO_TASK_DIR` | the task the time goes to: the node itself, or the task picked for a container. Unset without one (creating a container, opening one without a task) |
| `UDO_CONTAINER_DIR` | the nearest container folder |
| `UDO_ROOT` | the root |
| `UDO_BIN` | the running `udo`: call this, not `udo` from `PATH` |

Only these are reliable. Anything else from your shell (an `export` in
`.zshrc`) is only there if udo was started from that shell, and not when
the TUI came from a launcher or a tmux hook. To configure a script, copy
it and change it.

Without a task, `UDO_TASK_*` variables udo itself inherited (a script that
runs udo again) are removed, so a nested run never sees a stale task.

## The terminal and exit codes

udo hands the terminal over unchanged: stdin, stdout and stderr are the
terminal, nothing is captured, and Ctrl+C goes to the script (udo waits
and carries on afterwards). Then:

| Where | Exit 0 | Exit ≠ 0 | Did not start (no `+x`, wrong `#!`) |
|-------|--------|----------|----------------------------------|
| TUI | back at once | `[udo] nvim-tmux exited with 1, press Enter to return`, then a toast | toast with a hint (`chmod +x`, `check its #! line`) |
| `udo run` | exit 0 | exits with the script's code (a signal: 128 + n) | error, exit 1 |
| `udo add` | `on_create: typst-setup` in the output | warning on stderr, `add` still exits 0: the node exists | warning, exit 0 |

With `--json` (`udo run`, `udo add`, `udo track run`), the script's
stdout goes to udo's stderr, so stdout is the JSON alone and
`udo add … --json | jq` works. You still see the output; stdin and
stderr stay the terminal, so a script can still ask questions.

## Blocking or returning

udo waits until the script ends. The script decides what that means:

- **Blocks** while you work: `tmux attach` (outside tmux), a terminal
  editor. The TUI comes back when you leave.
- **Returns at once**: `tmux switch-client` (inside tmux, the udo pane stays
  where it is), `udo track run --detach` (GUI editors). The TUI is back
  right away.

## Tracking time from scripts

udo never starts a timer for a run. Tracking is the script's, through
`udo track`, so a setup script does not start one by accident:

```sh
"$UDO_BIN" track start --task "id:$UDO_TASK_ID" --source tmux --owner "tmux:udo-$UDO_TASK_ID"
"$UDO_BIN" track stop --owner "tmux:udo-$UDO_TASK_ID"
"$UDO_BIN" track run --task "id:$UDO_TASK_ID" --source idea --owner "idea:$UDO_TASK_ID" -- idea --wait "$UDO_NODE_DIR"
"$UDO_BIN" track run --detach …    # returns at once; a helper waits and stops
```

- **`--task id:$UDO_TASK_ID`**: by id, so a rename between start and stop
  does not matter.
- **`--source`**: what kind of program (`tmux`, `idea`; a-z, 0-9, `-`).
  Shown in the status line (`▶ lab 3 · 1h12 · tmux`) and the session list.
- **`--owner`**: which instance may stop the session. One per instance:
  two IntelliJ projects are `idea:<task 1>` and `idea:<task 2>`, so one
  window closing does not stop the other's session. `manual` is udo's own
  (`s`, `udo start`) and refused here.

The rules that make hooks safe:

1. **A start takes over** a session on another task (from another owner
   too): the newest place you are in wins. A start on the task already
   timed changes nothing, its owner stays: after `s` on lab 3, attaching
   lab 3's tmux session keeps the timer manual, and tmux's detach later
   does not end it.
2. **A stop by another owner is a no-op**, exit 0. A hook may fire for
   every session on a server; it only ever stops its own.
3. **A manual stop always wins**: `s` or `udo stop` stops whatever runs.

`udo track start/stop` exit 0 also when nothing happened (rule 2), 1 on a
real error (unknown task, done task, `--owner manual`), 2 on wrong
arguments. `track run` exits with the program's code.

## tmux pitfalls

Found by trying (tmux 3.6a) and kept by `tests/example_nvim_tmux.rs`:

- Attaching fires the session's `client-session-changed` and
  `client-attached`, once each: both may start, the second is harmless.
- Switching from A to B fires only B's `client-session-changed`, nothing on
  A. Leaving a session for one that is not udo's needs a global
  `client-session-changed` that stops `#{client_last_session}`.
- Quitting the program in a session ends the session and its own hooks;
  only a global `session-closed` fires.
- The session's name, per hook: `client-detached` has it in
  `#{session_name}` (`#{hook_session_name}` is empty there);
  `session-closed` in `#{hook_session_name}`.
- Hooks run in the tmux server, with its environment and `PATH`: name
  `$UDO_BIN` and `$UDO_ROOT` in the hook command.
- `set-hook -g client-detached …` replaces the user's own hook of that
  name; an array index (`client-detached[77]`) adds one next to it.
- `=name` targets the exact session; commands that take a pane
  (`set-option`, `set-hook`, `send-keys`) need `=name:`.

## Examples

Copy them into your run folder and set them where they apply. The
`open_with` ones also open a container without a task (lecture notes):
the same program on the container's folder, untimed.

**`nvim-tmux`** (`open_with`): one tmux session per task (`udo-<task
id>`) with nvim in the task's folder. Attaching or switching in starts the
timer; detaching, switching away or quitting nvim stops it. Outside tmux
it attaches, inside it switches. A container without a task gets a
session of its own (`udo-<container id>`), untimed.

```sh
cp examples/run/nvim-tmux.sh ~/.config/udo/run/
udo settings set uni open_with=nvim-tmux
```

**`tmux`** (`open_with`): as `nvim-tmux`, with a shell in the session
instead of nvim. The session stays when an editor in it quits, until you
end the shell (`exit`) or the session. Both set the same global hooks, so
they can be used side by side.

```sh
cp examples/run/tmux.sh ~/.config/udo/run/
udo settings set uni open_with=tmux
```

**`idea`** (`open_with`, macOS): opens the node's folder in IntelliJ IDEA
and times the task until that project window closes. Returns at once. If
IntelliJ is not running it starts it first and waits a few seconds
(`settle`): otherwise `idea --wait` would itself become the IDE and only
return when all of IntelliJ quits. Check by hand that `settle` is long
enough on your machine. A container without a task is opened with
`open -a`, untimed.

```sh
cp examples/run/idea.sh ~/.config/udo/run/
udo settings set code open_with=idea
```

**`zed`** (`open_with`): opens the node's folder in Zed and times the task
until that window closes. Returns at once. It opens the folder with
`zed --new --wait`: in a window of its own, `--wait` ends when that window
closes. Without `--new`, `--wait` only returns when all of Zed quits,
whether Zed was running before or not. No start-first step as for
IntelliJ is needed. Needs the CLI on `PATH` (in Zed: "Install CLI"). A
container without a task is opened with `zed --new`, untimed.

```sh
cp examples/run/zed.sh ~/.config/udo/run/
udo settings set code open_with=zed
```

**`typst-setup`** (`on_create`): copies `<root>/templates/typst/` into a
new task's folder, keeping files already there (a sample template is in
`examples/templates/typst/`). Does nothing for containers, tasks without a
folder, or when opened. A missing template is reported as a failure.

```sh
cp examples/run/typst-setup.sh ~/.config/udo/run/
mkdir -p ~/.config/udo/templates && cp -R examples/templates/typst ~/.config/udo/templates/
udo settings set uni on_create=typst-setup
```

## Writing your own

- Start with `#!/usr/bin/env bash` and `set -euo pipefail`; check what you
  need first (`: "${UDO_ROOT:?}"`).
- Exit 0 where the script does not apply: it is inherited by everything
  below where it is set.
- An `open_with` set on a container also opens the container itself,
  without a task: without `UDO_TASK_ID`, open the node untimed (or exit
  0) instead of failing.
- Call `"$UDO_BIN"`, not `udo`.
- Pick a source for the program and an owner per instance.
- Run `shellcheck` on it.
- Test it like the examples are tested: `tests/example_*.rs` run them
  against the real binary, with fakes on `PATH` for the programs, and
  `tests/example_lint.rs` runs `bash -n` and `shellcheck` on all of them.
