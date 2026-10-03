#!/usr/bin/env bash
# nvim-tmux: one tmux session per task (udo-<task id>) with nvim in the
# task's folder. The timer runs while a client is in that session:
# attaching or switching in starts it; detaching, switching away or
# quitting nvim stops it.
#
#   cp examples/run/nvim-tmux.sh ~/.config/udo/run/
#   udo settings set uni open_with=nvim-tmux
#
# Outside tmux it attaches (udo waits until you detach); inside tmux it
# switches (returns at once, the udo pane stays where it was).
#
# Hooks, from tmux 3.6a:
# - attach fires the session's client-session-changed and client-attached:
#   both start, the second takes over the first (same owner), harmless
# - switching A -> B fires only B's client-session-changed, nothing on A:
#   a global hook stops #{client_last_session} (a no-op when that was no
#   udo session, or when B's start came first: another owner by then)
# - quitting nvim ends the session and its own hooks: only the global
#   session-closed fires
# - detaching fires the global client-detached
# - global hooks at a fixed index ([77]): your own hooks stay, and running
#   this again overwrites only ours
# - targets: `=name` is the exact name; commands that take a pane
#   (set-option, set-hook, send-keys) need `=name:`
set -euo pipefail
: "${UDO_TASK_ID:?nvim-tmux opens a task (on a container: pick one)}"

editor="nvim ." # what runs in the session (it ends: the session ends)
session=udo-$UDO_TASK_ID
owner=tmux:$session
dir=${UDO_TASK_DIR:-${UDO_NODE_DIR:-$UDO_CONTAINER_DIR}}

# hooks run in the tmux server: neither its PATH nor its environment need
# to know this udo, so name the binary and the root
udo="UDO_ROOT='$UDO_ROOT' '$UDO_BIN'"

# `=`: the exact name, not a prefix. set-option / set-hook take a pane
# target, so theirs ends in `:` (`=name` alone: "no such session")
if ! tmux has-session -t "=$session" 2>/dev/null; then
    tmux new-session -d -s "$session" -c "$dir" "$editor"
    tmux set-option -t "=$session:" @udo_task "$UDO_TASK_ID"
    start="run-shell -b \"$udo track start --task id:$UDO_TASK_ID --source tmux --owner $owner\""
    tmux set-hook -t "=$session:" client-session-changed "$start"
    tmux set-hook -t "=$session:" client-attached "$start"
fi

# server-wide, every time: the server may be new since the last run.
# Which format names the session differs per hook (tmux 3.6a,
# tests/example_nvim_tmux.rs): client-detached has it in #{session_name}
# (#{hook_session_name} is empty there); session-closed in
# #{hook_session_name}; switching away in #{client_last_session}
stop="run-shell -b \"$udo track stop --owner tmux:"
tmux set-hook -g 'client-detached[77]' "$stop#{session_name}\""
tmux set-hook -g 'session-closed[77]' "$stop#{hook_session_name}\""
tmux set-hook -g 'client-session-changed[77]' "$stop#{client_last_session}\""

if [[ -n ${TMUX:-} ]]; then
    tmux switch-client -t "=$session"
else
    tmux attach-session -t "=$session"
fi
