# udo

A task, time and workflow manager for the terminal: a tree of workspaces,
projects and tasks with due dates, a timer that records where your time
goes, and run configs that open a task in your tools (nvim in tmux,
IntelliJ, Zed) and time it while you work.

## Install

Homebrew (macOS on Apple silicon, Linux x86_64):

```sh
brew install jqydee/tap/udo
```

Or the install script, which puts the binary in `~/.cargo/bin`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jqyDee/udo/releases/latest/download/udo-installer.sh | sh
```

Other platforms build from source:

```sh
cargo install --git https://github.com/jqyDee/udo
```

Data lives in `~/.config/udo` (or `$UDO_ROOT`): the tree, its settings,
and the recorded sessions (`udo.db`). Workspaces, projects and tasks can
have folders of their own anywhere on disk.

## Use

`udo` without arguments opens the TUI; `?` shows every key.

| Key | | Key | |
|-----|-|-----|-|
| `j` `k` `h` `l` | move | `t` / `c` | new task / container |
| `Space` | fold | `e` / `d` | edit / remove |
| `Tab` | details tab | `x` | done / reopen |
| `s` | start / stop timer | `o` / `O` | open (with…) |

Everything also works from the CLI, for scripts and quick edits:

```sh
udo add workspace uni --dir ~/uni
udo add project uni/cs
udo add task "uni/cs/lab 3" --due "fri 22:00"
udo ls                      # the tree, due dates, status
udo start "lab 3"           # NODE: a path, any unique end of one, or
udo stop                    # nothing for the node of the current folder
udo session list            # recorded time, last 7 days
udo estimate "lab 3"        # how long it should take, and why
udo settings set uni open_with=nvim-tmux
udo run "lab 3"             # open it with its run config
```

`--json` on any command prints the result as JSON; `udo help <command>`
explains the rest.

## Run configs

Scripts in any language in `~/.config/udo/run/` that open or set up a
node; udo hands them the node in `UDO_*` variables and the terminal. They
track time through `udo track`, so a tmux session or an editor window can
time a task by itself. See [docs/run-configs.md](docs/run-configs.md) and
the examples in [examples/run/](examples/run/).

## Develop

```sh
cargo test                  # unit + integration tests (real binary, PTY, tmux)
scripts/seed-testdata.sh    # throwaway dataset in /tmp/udo-test
UDO_ROOT=/tmp/udo-test cargo run
```

`TODO.md` has the plan and what is next.

## Contributing

Contributions are welcome: bug reports, ideas, fixes, and run configs for
more tools. Before a pull request:

- `cargo fmt`, `cargo clippy --all-targets` without warnings, `cargo test`
  green
- new behaviour comes with a test
- commit messages in Conventional Commits (`feat(tui): …`, `fix(run): …`)

[CONTRIBUTING.md](CONTRIBUTING.md) has the setup (tools for the full test
suite), the code layout and the conventions.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
