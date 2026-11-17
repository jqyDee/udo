#!/usr/bin/env bash
# nvim-tmux: one tmux session per task (udo-<container>/<task>) with nvim
# in the task's folder. The timer runs while a client is in that session:
# attaching or switching in starts it; detaching, switching away or
# quitting nvim stops it. A container opened without a task gets a session
# of its own (udo-<container>), untimed.
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
# - names: found by id (@udo_key), named once when made: a rename keeps
#   the session and its name (the timer's owner is that name), and opening
#   a task directly or through its container is the same session. Only
#   letters, digits, `_`, `-` and `/`: `.` and `:` split tmux targets,
#   `#` is a tmux format, quotes and `$` would break the hook commands
#   and `--owner` takes no spaces
set -euo pipefail

editor="nvim ." # what runs in the session (it ends: the session ends)
# a task: its session, timed. No task (a container on its own): the
# container's session, untimed
task=${UDO_TASK_ID:-}
key=${task:-$UDO_NODE_ID}
if [[ -n $task ]]; then
    # an older udo has no UDO_CONTAINER_NAME: the folder's name
    container=${UDO_CONTAINER_NAME:-$(basename "$UDO_CONTAINER_DIR")}
    label="udo-$container/$UDO_TASK_NAME"
else
    label="udo-$UDO_NODE_NAME"
fi
label=${label//[^[:alnum:]_\/-]/_}
dir=${UDO_TASK_DIR:-${UDO_NODE_DIR:-$UDO_CONTAINER_DIR}}

# hooks run in the tmux server: neither its PATH nor its environment need
# to know this udo, so name the binary, the root and the data dir
udo="UDO_ROOT='$UDO_ROOT' UDO_DATA='$UDO_DATA' '$UDO_BIN'"

# this node's session, if there is one (no server yet: none)
session=$(tmux list-sessions -F '#{@udo_key}|#{session_name}' 2>/dev/null |
    awk -F'|' -v k="$key" '$1 == k { print $2; exit }')

# `=`: the exact name, not a prefix. set-option / set-hook take a pane
# target, so theirs ends in `:` (`=name` alone: "no such session")
if [[ -z $session ]]; then
    session=$label
    # the name taken (the same names in another container): add the id
    if tmux has-session -t "=$session" 2>/dev/null; then
        session=$label-$key
    fi
    owner=tmux:$session
    tmux new-session -d -s "$session" -c "$dir" "$editor"
    tmux set-option -t "=$session:" @udo_key "$key"
    if [[ -n $task ]]; then
        tmux set-option -t "=$session:" @udo_task "$task"
        start="run-shell -b \"$udo track start --task id:$task --source tmux --owner $owner\""
        tmux set-hook -t "=$session:" client-session-changed "$start"
        tmux set-hook -t "=$session:" client-attached "$start"
    fi
fi

# server-wide, every time: the server may be new since the last run.
# Without a task too: leaving a task's session for this one must stop it.
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
