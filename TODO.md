# udo TODO

Priorities as of 2026-10-02, in order. Background and older plans:
`roadmap.txt` (phase numbers below refer to it).

**Next up:** to be decided. Done lately: session corrections in the TUI
(add, edit, split, cut, remove) and `?` help in the sessions list.

## Done: creation flow

- [x] **Custom dirs when creating.** New `folder ‹ auto · custom · none ›`
      choice (←/→) above the dir row:
      - auto: `<parent dir>/<folder name>`, shown live while typing the name
      - custom: editable, pre-filled with the auto path, must be absolute
        (`/…` or `~/…`); existing folders are fine; created exactly as typed
      - none: tasks only, no folder
      The task default comes from the `task_folders` setting (section 1).
      Custom text is kept while switching modes. CLI keeps resolving relative paths against
      the cwd; `add-project` gets `--dir`.
- [x] **Folder names:** one shared `folder_name()` in `model`: spaces -> `_`
      in the folder only, the node name keeps its spaces (CLI too).
- [x] **Kind choice** in the container form (`workspace · project`), guessed
      default as today; allows nested workspaces.
- [x] **Form defaults from outside:** the app passes `TaskDefaults` into the
      form, so defaults can come from settings later.
- [x] **Stable IDs** for tasks and containers (roadmap 2.3.2).
- [x] **`created_at` + `description`** in a shared `NodeHeader` (tasks and
      containers); one `Tree::create(parent, node)`. Description in the create
      forms, `--description` in the CLI, shown in the details pane. Further
      fields (estimate + source, planning opt-out, run config) only when their
      feature is built.
- [x] **Edit nodes** (`e`): name, description, due (tasks), kind
      (containers). `HeaderPatch` + `Tree::edit` with the name checks of
      `create`. Create and edit forms are built by one `Form::for_node`
      (create = a template node) and read by one `Form::values`.

## 1. Container settings (foundation for everything below)

- [x] **Inheritance:** a setting not set on a container comes from its parent
      (project -> workspace -> root -> built-in default). Every setting is
      optional, yes/no included, so a child can switch off what its parent
      switched on. (roadmap 1.2 "fallback hierarchy")
- [x] **Only set values are saved** in `.udo.toml`: a file shows exactly
      what that container overrides.
- [x] **Root-only settings get their own type** (`RootSettings`, `[root]`
      table of the root's `.udo.toml`, never inherited). Today: `theme`;
      later with their features: first weekday, run config library,
      timetable.
- [x] **Settings tab** in the details pane (Tab / Shift+Tab): every setting
      with its effective value and where it comes from (`fri 22:00 (from
      uni)`, `none (default)`). Built from one table, `SETTINGS`.
- [x] **Root row** at the top of the tree: the root can be selected (its
      settings, creating directly in it). Replaced the `C` / `T` keys.
- [x] **Edit settings** in the TUI: `e` on the settings tab opens a form
      for the container at the cursor (a task: its container), built from
      `SETTINGS` (each entry has `set`: text -> value). Empty field =
      inherit (placeholder shows the inherited value,
      `Tree::inherited_setting`); settings with fixed values (`choices`)
      are a choice with `inherit` first; `format` examples are shown below
      the form. Saved with `Tree::set_settings` (the form always sends all
      of them). Root row: also the `[root]` settings (`ROOT_SETTINGS`, same
      table shape), in the form and in the settings tab.
- [x] Edit settings from the CLI: `udo settings [NODE]`, `settings set` /
      `unset` (same `SETTINGS` table as the TUI form).
- [x] **Default deadline** for new tasks as a rule: `fri 22:00` (next
      Friday) or `+7d 23:59`. Nothing fancier for now.
- [x] **`task_folders = auto | none`** replaces the "folders only in
      projects" rule. Kinds become plain labels. Built-in default: `none`
      (opt-in: set `auto` on a container and everything below it gets task
      folders). Containers always have their own folder either way.
- [ ] Task-level exceptions (own estimate, left out of planning, own run
      config) are task fields, not settings.
- [x] Durations are written like `1h30` / `90m` (`model::time::Minutes`;
      not used by a setting yet, the default estimate is the first).
- [x] CLI: create workspaces below other containers: the NODE path names
      the parent (`udo add workspace uni/cs/labs`), no `--parent` needed.

## 2. Time tracking (the core feature)

- [x] **Default estimate** per workspace / project: how long a task takes,
      as a first guess. An inherited setting `estimate: Option<Minutes>`
      plus one `SETTINGS` entry.
- [x] **One time type:** every point in time is `model::time::Time`
      (`DateTime<FixedOffset>`: instant + the local offset when recorded,
      `time::now()`); shown via `with_timezone(&Local)`. Old `…Z` values
      still load.
- [x] **Sessions API** (`model::sessions`): `Session`, `TaskRef` (name,
      description, container path; survives a task delete),
      `SessionStore` trait (start, stop, running, query, add, split, cut,
      edit, delete), `SessionError`. Reference backend `MemorySessions`
      and a contract (`store_contract!`, one test per rule) every backend
      has to pass.
- [x] **Sessions, not totals:** every work session is stored with start and
      end (total = sum), plus its source (`manual`, `nvim`, `tmux`, `idea`,
      ...) and whether it was edited.
- [x] **Manual start/stop in the CLI:** `udo start [NODE]`, `udo stop`,
      `udo status [--short]`. One timer; starting another task stops the
      current one (`Core` rules, shared with the TUI). Today starting sets
      "in progress"; with the status change a task is "started" once it
      has a session.
- [x] **Manual start/stop in the TUI:** `s` on a task, through the same
      `Core::start` / `stop`.
- [x] **Timer survives closing udo:** a session is written to `udo.db` as
      "open" the moment it starts.
- [x] **Timer in the TUI status line** (`▶ lab 3 · 1h12`); open sessions
      found after a crash / power-off are offered for fixing in time tab.
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
- [x] **Corrections are a core feature:** edit start/end, split, cut (e.g.
      a lunch break the timer ran through), delete, add sessions manually.
      CLI: `udo session list / add / edit / split / cut / rm`, short IDs,
      times like `-45m` / `+1h30`. TUI: in the sessions tab's list, `a`
      add, `e` edit, `s` split, `c` cut, `d` remove (generic confirm
      prompt); also in an empty list. Spec:
      `docs/superpowers/specs/2026-09-30-tui-session-corrections-design.md`.
- [ ] **Warning on suspiciously long sessions** when stopping the timer
      (forgot to stop it: offer a cut / a new end right away).
- [x] **Storage: SQLite** (`udo.db` at root, `rusqlite`), sessions linked to
      tasks by ID. Several writers at once (TUI, CLI, helpers, tmux hooks)
      are safe there (WAL, `busy_timeout`, `BEGIN IMMEDIATE`). Tasks stay in
      `.udo.toml` for now. `SqliteSessions` behind `SessionStore`, passing
      the same contract as the memory store (plus a differential test
      memory vs. SQLite); migrations via `user_version`; `session_edits`
      log.
- [ ] **Learn:** per container, the average time of tasks replaces the
      default. Unfinished tasks count too, weighted lower (their time so far
      is an "at least"). Averages are calculated from the sessions, not
      stored as settings.
- [ ] Show estimate vs. actual per task and container (roadmap 5.2).
- [x] **Sessions tab** in the details pane (a third `DetailsTab`), a pure
      sessions page: sessions newest first (a container: all tasks below
      it, with task names), paged (page length from the space left,
      `h` / `l` switch pages); `e` moves the cursor into the list (the
      tree's cursor dims, also under a form or prompt opened from it),
      `esc` goes back to the tree. The corrections above work in it.
      Estimate, duration and left go into the details tab. Spec:
      `docs/superpowers/specs/2026-09-29-tui-time-tab-design.md`.
- [x] **`?` help in the sessions list:** shows `SESSION_LIST_KEYMAP`;
      help wraps the mode it was opened from (`Mode::Help(Box<Mode>)`), so
      any key goes back to the list, the tree stays dimmed under it.
      Dispatch and help read the same `Mode::keymap`.
- [ ] **Filter the sessions tab's list** (later): e.g. by date range,
      source, edited / not edited.
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
- [x] Needs **stable task IDs** first (roadmap 2.3.2): time logs must survive
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
- [ ] Start a planned block from the 7-day view (`s` starts the timer).

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

- [ ] **Description as a multi-line textbox** in the forms (Enter = new line,
      Ctrl+S = submit); details pane shows each line.
- [x] `udo run` panicked (`todo!()`): removed in the CLI rework until run
      configs exist.
- [x] `submit_form` clones the whole form on every Enter.
- [x] **CLI rework** (`docs/superpowers/specs/2026-09-28-cli-rework-design.md`,
      stages 1-5): `Core`, one path syntax for NODE, `--json`, every command
      in `src/cli/commands/` (groups as folders: `add`, `settings`,
      `session`).
- [ ] **Session time details** (small, from stage 5):
      - `-D` / `now` keep their seconds, so pieces cut in a later run show
        `44m` instead of `45m`: round session times to the minute?
      - `session rm` on a running session prints it as running, though the
        store stops it first
      - `now` is case-sensitive (`NOW` is refused)
      - `session list --from` after `--to` shows an empty list silently
        instead of an error
- [ ] **Shell completion** after the CLI rework: `clap_complete` with
      dynamic node names from the tree (`udo start la<Tab>` -> `lab 3`),
      using the same resolver as the commands.
- [ ] **More filters for `udo session list`** (beyond NODE, `--from` /
      `--to` / `--all` / `--deleted`): e.g. by source (`manual`, `nvim`,
      …), edited only, longer than X; maybe the same filters for `ls`
      (status, due before).
- [x] **Test helper leftovers** (after centralising `test_util.rs`): shared
      `task_ref` for the session store tests, local date helpers built on
      `dt`, clearer names for `rm.rs` `fake_trash` and `app/tests.rs`
      `disk_tree`. Plan:
      `docs/superpowers/plans/2026-09-29-test-util-leftovers.md`.
- [ ] `roadmap.txt` 6.1 status is out of date (event loop is async, TUI
      writes: status, delete, create, edit, settings; details tabs, root
      row).
- [ ] `TaskPatch.dir` can't remove a task's folder: optional fields need
      `Option<Option<T>>` in their patch, like `HeaderPatch.description`.
- [ ] Maybe a "jump to the root" key (`g`) if walking up with `h` gets
      tedious in deep trees.
- [x] Cursor and folding moved from `Tree` into the TUI's `TreeState`;
      `view.toml` stores folded containers and the selected node by ID, so
      the cursor comes back after a restart.
- [x] `udo edit` in the CLI (name, description, due, kind).
- [ ] Edit dirs (move folders): its own operation (rename on disk, fix the
      parent's `children` / task row), not a plain patch field.
- [x] Delete with folder: optionally remove the node's folder too (today `d`
      only unregisters, files stay). Separate, clearly worded confirm
      (`also delete /…/lab_3 and its files?`); containers take their whole
      subtree. Maybe to the trash instead of deleting for good.

## Later: understanding tasks

- [x] **Description field** per task (see "Now").
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
