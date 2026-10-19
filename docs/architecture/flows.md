# Flows

What happens on load and on each kind of change. Every change goes through
`Core` (`src/core/`), from the CLI and the TUI alike. Two rules hold
everywhere:

- **Checks before the disk:** a failed call leaves nothing behind.
- **The action first, the record second:** an estimate row is written
  after the action; if that fails, the action stands and a warning says so
  ([estimates.md](estimates.md#recording)).

## Load at startup

```mermaid
sequenceDiagram
    participant main as main.rs
    participant Core
    participant Tree
    participant fs as .udo.toml files
    participant Storage

    main->>Core: open(root)
    Core->>Tree: load_from(root)
    alt no .udo.toml in root (first run)
        Tree->>fs: save the new root
    else
        Tree->>fs: build_container(root), recursing into children
        Note over Tree,fs: a child folder without .udo.toml (or listed twice)<br/>goes to unloaded, not an error
        Tree->>Tree: fix_duplicate_ids()
        Tree->>fs: save the containers whose ids changed
    end
    Core->>Storage: open_db(root): udo.db, run new migrations
    main->>main: CLI command, or tui::run(core)
    Note over main: TUI: TreeState::load reads view.toml<br/>(folded containers, selected node by id)
```

## Create

```mermaid
sequenceDiagram
    participant UI as CLI / TUI
    participant Core
    participant Tree
    participant fs as disk
    participant Stores as Storage
    participant run

    UI->>Core: create(parent, node)
    Core->>Tree: create(parent, node)
    Tree->>Tree: check_name, check_dir_free
    opt node has a folder
        Tree->>fs: create_dir_all (never over an existing .udo.toml)
    end
    Tree->>Tree: insert(parent, node)
    opt a container
        Tree->>fs: save its own .udo.toml
    end
    Tree->>fs: save(parent): new task row / child folder
    Tree-->>Core: path
    Core->>Stores: record_estimate(Created) (tasks only)
    Core-->>UI: path
    opt on_create set and not skipped (--no-run, setup skip)
        UI->>run: RunRequest::on_create, launch
        Note over UI,run: in the caller, not in Core: the node stays<br/>whatever the script does (CLI: warning, exit 0)
    end
```

The TUI hands the terminal to the script (`Flow::Run`) and resumes after
it.

## Edit

```mermaid
sequenceDiagram
    participant UI as CLI / TUI
    participant Core
    participant Tree
    participant Stores as Storage

    UI->>Core: edit(path, NodePatch)
    Core->>Core: remember the task's own estimate
    Core->>Tree: edit(path, patch)
    Tree->>Tree: refuse the root and folder changes, check_name
    Tree->>Tree: update(path, patch)
    Tree->>Tree: save(path)
    opt own estimate changed
        Core->>Stores: record_estimate(Edited)
    end
```

Settings go through their own call (`Core::set_settings` ->
`Tree::set_settings`): only containers have them, `[root]` settings only
the root.

## Delete

```mermaid
sequenceDiagram
    participant UI as CLI / TUI
    participant Core
    participant Tree
    participant Sessions as SessionStore
    participant Trash

    alt unregister only (default)
        UI->>Core: delete(path, now)
        Core->>Sessions: stop_if_on(tasks at or below path)
        Core->>Tree: delete(path): remove from parent, save(parent)
        Note over Tree: files stay on disk
    else with its folder
        UI->>Core: purge_plan(path)
        Core->>Tree: purge_plan: safety checks, folders to trash
        Core-->>UI: PurgePlan (shown in the confirm)
        UI->>Core: purge(plan, trash, now)
        Core->>Sessions: stop_if_on(...)
        Core->>Tree: purge(plan): plan still current?
        Tree->>Tree: delete(path)
        Tree->>Trash: each folder (system trash)
        Tree-->>Core: PurgeReport: trashed / failed
        Core-->>UI: PurgeReport + the stopped session
    end
    UI->>UI: TUI: TreeState::after_remove(path)
```

- A running timer on a removed task is stopped first.
- Sessions and estimate rows of removed tasks are kept (they feed the
  estimates).
- Unregistering comes before trashing: if trashing fails halfway, udo is
  still consistent and the report names what is left on disk.
- `after_remove` keeps the TUI cursor valid: a later sibling shifts left;
  on the removed node it takes the next sibling, else the previous one,
  else the parent.

## Save

```mermaid
sequenceDiagram
    participant Tree
    participant persist

    Tree->>Tree: save(path): nearest_file_owner(path)
    Note over Tree: a task: its container, which holds its row
    Tree->>Tree: ContainerData::try_from(container)<br/>(children = child folders + unloaded)
    Tree->>persist: write_toml_atomic(dir/.udo.toml)
    Note over persist: temp file in the same folder, fsync,<br/>rename over the old one, fsync the folder
```

## Timer

```mermaid
sequenceDiagram
    participant UI as CLI / TUI / udo track
    participant Core
    participant Sessions as SessionStore
    participant Est as EstimateStore

    UI->>Core: start(path) or track_start(path, source, owner)
    Core->>Core: task_ref(path), refuse a done task (IsDone)
    Core->>Sessions: start(task, source, owner, at)
    Note over Sessions: stops the running session first: one timer
    Core->>Est: record_estimate(Started): a row only on the task's first session
    UI->>Core: stop() or track_stop(owner)
    Core->>Sessions: stop(owner, at)
    Note over Sessions: manual stops anything,<br/>a program only its own session
```

Adding a session by hand (`Core::add_session`) records `Started` the same
way. Corrections (edit, split, cut, remove) only change sessions; removed
ones are marked, never deleted ([storage.md](storage.md#sessions)).
