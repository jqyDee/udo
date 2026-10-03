# Contributing

Thanks for helping with udo. This file says how to build it, what a change
needs before it goes in, and how the code is laid out.

## Setup

- Rust (stable, edition 2024): `cargo build`
- For the full test suite also `tmux`, `shellcheck` and `python3`. Tests
  that need one of them say `skipped: no …` and pass without it.

```sh
cargo test                          # everything
cargo test --lib                    # unit tests only (fast)
cargo test --test example_nvim_tmux # one integration test file
scripts/seed-testdata.sh            # throwaway data in /tmp/udo-test
UDO_ROOT=/tmp/udo-test cargo run    # the TUI on it
```

Never point a test or a try-out at your real data: set `UDO_ROOT` (the seed
script refuses anything outside `/tmp`).

## Before you send a change

- `cargo fmt` (default rustfmt settings)
- `cargo clippy --all-targets` without warnings
- `cargo test` green
- new behaviour has a test; a fixed bug has a test that failed before
- scripts in `examples/run/` pass `shellcheck` (checked by
  `tests/example_lint.rs`), and each has its Python twin in
  `examples/python/`

## Layout

| Where | What |
|-------|------|
| `src/model/` | the data: tree, nodes, settings, sessions. No I/O |
| `src/persist.rs` | the `.udo.toml` files of the tree (atomic writes) |
| `src/storage/` | recorded sessions in SQLite (`udo.db`) behind a `SessionStore` trait |
| `src/core/` | `Core`: every change goes through it (CLI and TUI alike) |
| `src/cli/` | the commands, one file each in `src/cli/commands/` |
| `src/tui/` | the terminal UI: `app/` state and keys (no terminal I/O, tested directly), `view/` drawing |
| `src/run/` | run configs: finding and launching scripts |
| `tests/` | integration tests against the real binary (`tests/common/` helpers: fresh root, pseudo-terminal, fakes on `PATH`) |
| `examples/` | run config examples, in bash and Python |
| `docs/` | the run config guide; architecture diagrams (`docs/tree/*.puml`) |

## Conventions

- **CLI and TUI stay on par.** A feature in one usually belongs in the
  other; the CLI is what scripts and hooks use.
- **Recorded time is never lost.** Sessions are not deleted, also not with
  their task: the history feeds estimates.
- **Every backend passes the same contract tests.** A new store implements
  the trait and runs the shared contract (`store_contract!`).
- **Tests use the real thing where it is cheap:** the real binary, a real
  tmux on a server of its own, a pseudo-terminal for the TUI. Fakes only
  for what cannot run in a test (GUI editors).
- **Docs on every public item;** comments say why, not what.
- **Commit messages** follow Conventional Commits:
  `feat(tui): …`, `fix(run): …`, `chore: …`, `docs: …`, scope = the
  module.

## Licensing

By contributing you agree that your work is dual licensed under MIT or
Apache-2.0, as described in the [README](README.md#license).
