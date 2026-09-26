# udo TODO

Priorities as of 2026-09-25, in order. Background and older plans:
`roadmap.txt` (phase numbers below refer to it).

## Now: creation flow

- [x] **Custom dirs when creating.** New `folder ‹ auto · custom · none ›`
      choice (←/→) above the dir row:
      - auto: `<parent dir>/<folder name>`, shown live while typing the name
      - custom: editable, pre-filled with the auto path, must be absolute
        (`/…` or `~/…`); existing folders are fine; created exactly as typed
      - none: tasks only, no folder
      The task default comes from the parent (projects auto, others none)
      until the `task_folders` setting (section 1) replaces that rule.
      Custom text is kept while switching modes. CLI keeps resolving relative paths against
      the cwd; `add-project` gets `--dir`.
- [x] **Folder names:** one shared `folder_name()` in `model`: spaces -> `_`
      in the folder only, the node name keeps its spaces (CLI too).
- [x] **Kind choice** in the container form (`workspace · project`), guessed
      default as today; allows nested workspaces.
- [x] **Form defaults from outside:** the app passes `TaskDefaults` into the
      form, so defaults can come from settings later.
- [ ] **Next, separate step (task format):** stable task IDs (existing
      tasks get one on load), `created_at`, optional one-line `description`.
      One file format change. Further fields (estimate + source, planning
      opt-out, run config) only when their feature is built.

## 1. Container settings (foundation for everything below)

- [ ] **Inheritance:** a setting not set on a container comes from its parent
      (project -> workspace -> root -> built-in default). Every setting is
      optional, yes/no included, so a child can switch off what its parent
      switched on. (roadmap 1.2 "fallback hierarchy")
- [ ] **Only set values are saved** in `.udo.toml`: a file shows exactly
      what that container overrides.
- [ ] **Root-only settings get their own type** (`RootSettings`: theme,
      default workspace, first weekday, run config library, timetable),
      instead of "Global only" comments.
- [ ] **Edit settings** from the TUI (and the CLI). The editor shows where a
      value comes from (`1h30 (from uni)`), can override and reset to
      inherited.
- [ ] **Default deadline** for new tasks as a rule: `fri 22:00` (next
      Friday) or `+7d 23:59`. Nothing fancier for now.
- [ ] **`task_folders = auto | none`** replaces the "folders only in
      projects" rule. Kinds become plain labels. Built-in default: `auto`.
- [ ] Task-level exceptions (own estimate, left out of planning, own run
      config) are task fields, not settings.
- [ ] Durations are written like `1h30` / `90m`.
- [ ] CLI: create workspaces below other containers (e.g. `--parent uni/cs`).

## 2. Time tracking (the core feature)

- [ ] **Default estimate** per workspace / project: how long a task takes,
      as a first guess.
- [ ] **Sessions, not totals:** every work session is stored with start and
      end (total = sum), plus its source (`manual`, `nvim`, `tmux`, `idea`,
      ...) and whether it was edited.
- [ ] **Manual start/stop first** (`w` in the TUI; `udo start <task>`,
      `udo stop`, `udo status` in the CLI). Only one timer at a time;
      starting another task stops the current one. Starting sets the status
      to "in progress".
- [ ] **Timer survives closing udo:** a session is written as "open" the
      moment it starts; the status line shows `▶ lab 3 · 1h12`. Open
      sessions found after a crash / power-off are offered for fixing.
- [ ] **Tracking by program (with run configs):**
      - nvim in the foreground: udo suspends the TUI and waits (roadmap 6.2)
      - nvim in tmux: tmux hooks (`client-attached`, `client-detached`,
        `session-closed`) call e.g. `udo track attach <task>`; a detached
        session does not count
      - GUI editors: only with `--wait` (IntelliJ, Zed); a small background
        helper `udo track-wait <task> -- <cmd>` waits and records the session
- [ ] **No daemon for now.** A real background service (launchd / systemd)
      later, when idle detection, file watching or reminders come. Same
      session data either way.
- [ ] **Corrections are a core feature:** edit start/end, split, delete, add
      sessions manually. Warn on suspiciously long sessions when stopping.
- [ ] **Storage: SQLite** (`udo.db` at root, `rusqlite`), sessions linked to
      tasks by ID. Several writers at once (TUI, CLI, helpers, tmux hooks)
      are safe there. Tasks stay in `.udo.toml` for now.
- [ ] **Learn:** per container, the average time of tasks replaces the
      default. Unfinished tasks count too, weighted lower (their time so far
      is an "at least"). Averages are calculated from the sessions, not
      stored as settings.
- [ ] Show estimate vs. actual per task and container (roadmap 5.2).
- [ ] **Time tab** in the TUI (next to the tree): running session, sessions
      per task with corrections, 7-day view of the schedule. The details
      pane shows estimate, time so far, remaining, number of sessions.
- [ ] **Better local estimates, step by step** (each measured against the
      recorded actual times; build the next only if needed):
      robust stats (median, recency weighting, a range instead of one
      number) -> similar past tasks by words (TF-IDF + nearest neighbours)
      -> small local sentence-embedding model for similarity (optional
      Cargo feature) -> regression over several features once there is
      enough data. No neural net trained on own data alone: too few tasks.
- [ ] **AI estimates (later):** udo calls a model itself (key in the create
      form). History stays local: each request sends the new task plus the
      most relevant past tasks (estimate + actual time) and the average;
      optional model-written notes stored locally (memory-tool style).
      Needs: task descriptions, the original estimate kept next to the
      actual time, where an estimate came from (default / average / ai /
      manual), opt-in `ai_access` setting (only name, description and times
      by default). Provider behind a small interface, so a local model can be
      used for full privacy. Rust: plain HTTP (no official SDK).
- [ ] Needs **stable task IDs** first (roadmap 2.3.2): time logs must survive
      renaming a task.

## 3. Timetable / scheduling

udo is a **suggestion engine**, not a strict schedule: it fills the free time
around your calendar and shows the result on your phone.

- [ ] **Free time = not blocked:** everything is available except busy
      calendar entries and fixed exclusions. No slots typed in by hand.
- [ ] **Calendar integration via CalDAV** (iCloud, Google, Fastmail, ...),
      reading and writing through one protocol. Password / app-specific
      password (Apple ID + app-specific password) in the macOS Keychain.
      Flow: find calendars -> you pick which block time and which one is the
      udo calendar -> read events for the next days -> write plan blocks;
      only fetch what changed since the last sync.
- [ ] **Subscriptions (uni timetable):** iCloud's CalDAV lists them as
      "subscribed" calendars with their ICS link (`cs:source`) but without
      events, so udo downloads that link itself. Found automatically if the
      subscription is stored in iCloud (not "On My Mac").
- [x] **Prototype verified** (`prototypes/caldav`, 2026-09-25): discovery,
      the "Studium" subscription (read), writing into the udo calendar.
      Still untested: reading events from normal iCloud calendars (none were
      coming up), incl. recurring ones.
- [ ] **Recurring events** (weekly lectures, with exceptions, time zones):
      the hard part. iCloud can expand them server-side; for ICS
      subscriptions udo expands them itself (e.g. `rrule` crate). Test well.
- [ ] Google also speaks CalDAV but only with OAuth: later, if ever.
- [ ] **What blocks:** the calendars you pick (uni + personal); all-day
      events block too; entries marked "free" don't. Configurable buffer
      around appointments / between tasks (e.g. 15 min).
- [ ] **Own "udo" calendar:** created once by you in Apple Calendar, found by
      name. udo writes only there, never into your calendars; every block
      has a fixed udo ID, so it can update / delete exactly its own blocks.
      It must be ignored when reading busy times (otherwise the plan blocks
      itself).
- [ ] **Exclusions:** weekdays (no weekends) and time windows (no tasks
      22:00-08:00); global at root first, per container later.
- [ ] **Planning:** earliest deadline first, remaining time = estimate -
      time spent; tasks split with min / max session length; buffer before
      the deadline. Clear warning when a task no longer fits before its
      deadline ("lab 3: 2h missing").
- [ ] **Stable plan:** blocks in the next hours (e.g. today) stay fixed; only
      changed blocks are written to the calendar.
- [ ] **Phone edits:** udo overwrites its own blocks for now; moved blocks
      as "pinned" maybe later.
- [ ] **Learn when you work:** sessions show when you usually work on which
      project (e.g. cs101 on Tuesday evenings); the planner prefers those
      times. No new data needed.
- [ ] **Sync as a scheduled job** (launchd interval, e.g. every 15 min:
      read calendars, re-plan, write), not a daemon. Runs when the laptop
      is awake. Last known busy times cached locally (SQLite) for offline
      planning.
- [ ] **Opt out:** exclude single tasks or whole containers from planning
      (inherited setting).
- [ ] **Privacy:** optional generic block titles ("udo: cs101" instead of the
      task name); encryption maybe later.
- [ ] Start a planned block from the 7-day view (`w` starts the timer).

## 4. Run configurations

- [ ] **Library at root:** all run configs are defined once at root, by name
      (`[run.typst] cmd = "..."`). Workspaces / projects only pick a
      default by name (inherited like any setting); any task or container
      can use any config.
- [ ] Open: one default, or one per occasion (`on_create` once vs.
      `default_run` repeatedly); placeholders like `{task_dir}`; unknown
      names warn instead of failing to load.
- [ ] Use cases: set up files once per task (roadmap 2.2 templates), build,
      open editor, ...

## Ideas for later

- [ ] **Task groups:** split a task into smaller sub-tasks (leaves). Time of
      a group = sum of its parts; the planner can place parts in separate
      slots. Possible use for container kinds beyond labels. Keep in mind:
      tasks may get children one day (stable IDs help).

## Smaller / later

- [ ] Edit existing nodes (`e`, `FormAction::EditNode`).
- [ ] `udo run` is a `todo!()` and panics: error out until it is built.
- [ ] `submit_form` clones the whole form on every Enter.
- [ ] `cli.rs` cleanup.
- [ ] `roadmap.txt` 6.1 status is out of date (event loop is async, TUI
      writes: status, delete, create).

## Later: understanding tasks

- [ ] **Description field** per task (typed by you, one line is enough).
- [ ] **Local assignment analysis, no AI:** read the assignment PDF from the
      task folder, extract structure (pages, number of exercises, word
      count, code vs. report) and keywords.
- [ ] **Optional AI summary:** summary, keywords, task type from a model.
      Same opt-in (`ai_access`) as AI estimates.
- [ ] Storage: your input (name, description) stays untouched; derived data
      (keywords, structure, summary) in its own section with the version of
      the method, so old tasks can be re-analysed. The file stays in the
      task folder; udo only stores which file it read.
- [ ] Fits with: auto task folders (where the PDF goes), run configs
      (`on_create` could copy + analyse the sheet), time tracking (the data
      the estimate stages learn from).
- [ ] **AI provider interface:** one small interface, configured once at
      root (endpoint URL, model name, API key from an env var). Two
      backends cover almost everything: "OpenAI-compatible" (local runners
      like Ollama / LM Studio / llama.cpp, and many hosted providers) and
      the Anthropic API. Local model first choice for privacy.
