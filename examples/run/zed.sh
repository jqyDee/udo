#!/usr/bin/env bash
# zed: open the node's folder in Zed; a task is timed until that window
# closes, a container opened without a task is not timed. Returns at once
# (`track run --detach`), so the TUI is back right away. Needs the `zed`
# CLI on PATH (Zed: "Install CLI").
#
#   cp examples/run/zed.sh ~/.config/udo/run/
#   udo settings set code open_with=zed
#
# `--new`: a window of its own for the folder. Without it, `zed --wait`
# returns only when all of Zed quits (Zed running or not, tried by hand
# 2026-10-04); with it, when that window closes. One owner per task
# (zed:<task id>): two windows open at once do not stop each other's
# session.
set -euo pipefail

dir=${UDO_NODE_DIR:-$UDO_CONTAINER_DIR}

# no task (a container on its own): just the window, untimed
[[ -n ${UDO_TASK_ID:-} ]] || exec zed --new "$dir"

exec "$UDO_BIN" track run --detach --task "id:$UDO_TASK_ID" \
    --source zed --owner "zed:$UDO_TASK_ID" -- zed --new --wait "$dir"
