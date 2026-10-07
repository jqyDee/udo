#!/usr/bin/env bash
# idea: open the node's folder in IntelliJ IDEA; a task is timed until
# that project window closes, a container opened without a task is not
# timed. Returns at once (`track run --detach`), so the TUI is back right
# away. macOS (`open -a`), the `idea` launcher on PATH (Toolbox /
# Homebrew).
#
#   cp examples/run/idea.sh ~/.config/udo/run/
#   udo settings set code open_with=idea
#
# One owner per task (idea:<task id>): two projects open at once do not
# stop each other's session.
set -euo pipefail

app="IntelliJ IDEA"
settle=5 # seconds: the process is up before it takes projects
dir=${UDO_NODE_DIR:-$UDO_CONTAINER_DIR}

# no task (a container on its own): just open the folder, untimed. `open
# -a` returns at once, running or not: no start-first, no settle (those
# are only for `idea --wait`)
[[ -n ${UDO_TASK_ID:-} ]] || exec open -a "$app" "$dir"

# not running: `idea --wait` would become the IDE itself and only return
# when all of IntelliJ quits. Start it first and give it time.
if ! pgrep -qf "$app.app/Contents/MacOS/"; then
    open -a "$app"
    for _ in $(seq 60); do
        pgrep -qf "$app.app/Contents/MacOS/" && break
        sleep 1
    done
    sleep "$settle"
fi

exec "$UDO_BIN" track run --detach --task "id:$UDO_TASK_ID" \
    --source idea --owner "idea:$UDO_TASK_ID" -- idea --wait "$dir"
