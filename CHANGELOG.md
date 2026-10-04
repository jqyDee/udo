## [0.1.1] - 2026-10-04

### ⚙️ Miscellaneous Tasks

- Test the dev branch; contributing: branches and data formats
- Release from dev, main only through pull requests
## [0.1.0] - 2026-10-04

### 🚀 Features

- Initial impl
- Model rewrite to tree
- Cleanup
- Tree navigation, collapse with saved view state, list command
- TUI prototype with tree browser and detail pane
- Roadmap update
- Async event loop
- *(tui)* Set task status with x/p/s/u
- *(tui)* Keymap table based inputs
- *(tui)* Toast notifications that expire on their own
- *(tui)* Cleanup
- *(tui)* Delete functionality
- *(tui)* Group help overlay into titled sections
- *(tui)* Initial form and creation logic
- *(tui)* Date editor
- *(tui)* Form cleanup
- *(tui)* Refactor
- Cargo fmt
- *(tui)* Shell-like ctrl shortcuts
- Readable kinds/statuses, local-time CLI dates, adaptive help, field ids, scrolling fields
- Name and folder normalization and consistency in tui and cli
- *(tui)* Dir/folder creation, container kind choosing
- *(model)* Task and container ids
- *(model)* Node struct holding common fields now!
- *(model)* NodeHeader + description,created_at
- Description in forms, CLI and details; single Tree::create
- *(model)* HeaderPatch, setup for editing
- *(tui)* Editing
- Dir purging added to delete
- TreeState moved into tui and from the Tree
- *(model)* Inherited container settings, root settings, time values
- Cursor gets saved on close
- *(tui)* Settings tab and root row
- Settings editing
- *(settings)* Fixed choices, format examples, root rows
- *(tui)* Choice fields and format hints in the settings form
- Default estimate
- *(sessions)* SessionStore API, mem backend, contract tests
- *(sessions)* Injected clock for session store
- *(sessions)* Sqlite barebones
- *(storage)* Sqlite conversions, tests
- *(sessions/sqlite)* Start, stop, running, query
- *(sessions/sqlite)* Corrections and edit log
- *(session)* SessionStore dispatch
- *(cli)* Start, stop and status
- *(core)* Initial move
- Relative paths in downstream dirs
- *(cli)* Add, ls, reporting, timer, path resolving and parsing
- *(cli)* Edit, mark, rm, show
- *(cli)* Settings
- *(cli)* Sessions
- *(status)* Status calculated through sessions
- *(tui)* Timer start / stop, status line
- *(tui)* Time rows in details view
- *(tui)* Session tab (still empty, but wired up)
- *(tui)* Session list entries
- *(tui)* Enter session list and move over control
- *(tui)* Session list cursor movement
- *(tui)* Edit sessions
- *(tui)* Form refactor
- *(tui)* Enter and stay in an empty sessions list
- *(tui)* Remove sessions from the list
- *(tui)* Split and cut sessions in the list
- *(tui)* Add sessions in the list
- *(tui)* ? help in the sessions list
- *(sessions)* Session owners and owner rules in the stores
- *(core)* Timer track start/stop
- Resolve id in task argument in cli
- *(cli)* Udo track start / stop
- *(cli)* Udo track run [--detach]
- *(mode)* Run settings
- *(run)* Run config library
- *(run)* Run moved into module, env context added
- *(cli)* Run
- *(tui)* O opens a task with its run config
- *(tui)* Run_with groundwork
- *(tui)* O picks script, setup
- *(tui)* O opens containers via a task picker, O picks the script
- *(tui)* Status line names the program that started the timer
- *(cli)* Add command runs on_create script, --no-run flag
- *(tui)* On_create script now runs
- *(examples)* Run config examples, guide and tests

### 🐛 Bug Fixes

- Lib non existent list.rs still in
- Removed .DS_Store
- *(tui)* Id in details pane
- *(model)* Tree constructor
- *(model)* Missed error msg change
- *(tui)* Readability
- *(sessions)* Shared rules
- *(storage)* Deadlock prevention because of WAL on sqlite
- *(test)* Moved common test utils, TODO.md
- Check times before now moved into core
- *(tui)* Dimmed tree cursor when in a session form
- *(run)* Move the run script stdout to stderr when json flag is set

### 📚 Documentation

- TODO roadmap + verified CalDAV prototype
- License section in the README, CONTRIBUTING.md

### 🚜 Refactor

- *(tui)* Generic confirm popup
- *(tree)* Open_with and open_tasks for CLI and TUI

### 🧪 Testing

- *(sessions)* Differential test memory vs sqlite

### ⚙️ Miscellaneous Tasks

- UDO_ROOT override and test data seed script
- *(model)* Cleanup 1
- *(model)* Tree refactor 2
- TODO.md update
- Cleanup
- Cleanup
- TODO.md update
- Time to fixed timezone
- Fmt imports
- Docs TODO.md
- *(refactor)* Rustfmt fn_call_width = 80
- TODO.md
- Docs, storage passed through to tui
- TODO.md update
- *(cli)* Refactor
- *(docs)* TODO.md
- *(docs)* Update TODO.md
- *(run)* Moved the RunRequest build pipeline out
- Update TODO.md
- *(scripts)* Seed-testdata for the current cli
- Licences
- README.md
- Drop roadmap.txt and move into TODO.md
- Default rust formatting
- Automatic releases
- Tests on macOS and Linux, release workflow (cargo-dist)
- Pin Rust 1.96, drop async-recursion
- Skip the changelog hook in dry runs
- Release 0.1.0
