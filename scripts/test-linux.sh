#!/bin/sh
# The CI ubuntu jobs (clippy + tests) locally, in Docker: catches
# Linux-only failures (e.g. ETXTBSY) before a push, while `cargo test`
# stays native and fast. Named volumes keep the toolchain (rustup reads
# rust-toolchain.toml), the registry and a Linux target dir between runs,
# apart from the macOS `target/`. Arguments go to `cargo test`:
#
#   scripts/test-linux.sh
#   scripts/test-linux.sh --lib run::
set -eu
cd "$(dirname "$0")/.."
docker run --rm -t \
  -v "$PWD":/work -w /work \
  -v udo-linux-target:/work/target \
  -v udo-linux-cargo:/usr/local/cargo/registry \
  -v udo-linux-rustup:/usr/local/rustup \
  rust:1-bookworm sh -c '
    apt-get update -qq && apt-get install -y -qq tmux shellcheck python3 >/dev/null
    cargo clippy --all-targets -- -D warnings && cargo test "$@"' sh "$@"
