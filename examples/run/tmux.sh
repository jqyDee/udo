#!/usr/bin/env bash
# tmux: one tmux session per task (udo-<container>/<task>) with a shell in
# the task's folder. Unlike nvim-tmux nothing runs in it but the shell: the
# session stays when an editor in it quits, until you end it (`exit`,
# kill-session). The timer runs while a client is in that session:
# attaching or switching in starts it; detaching, switching away or ending
# the session stops it. A container opened without a task gets a session
# of its own (udo-<container>), untimed.
#
#   cp examples/run/tmux.sh ~/.config/udo/run/
#   udo settings set uni open_with=tmux
#
# Outside tmux it attaches (udo waits until you detach); inside tmux it
# switches (returns at once, the udo pane stays where it was).
#
# The hook rules (tmux 3.6a) and the session names are explained in
# nvim-tmux.sh, which sets the same global hooks at the same index ([77])
# with the same commands and names its sessions the same way: which
# script ran last does not matter.
set -euo pipefail

# a task: its session, timed. No task (a container on its own): the
# container's session, untimed. Found by id (@udo_key), not name: a rename
# keeps the session, and opening a task directly or through its container
# is the same session
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
    tmux new-session -d -s "$session" -c "$dir" # a shell: stays
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
# Which format names the session differs per hook: see nvim-tmux.sh
stop="run-shell -b \"$udo track stop --owner tmux:"
tmux set-hook -g 'client-detached[77]' "$stop#{session_name}\""
tmux set-hook -g 'session-closed[77]' "$stop#{hook_session_name}\""
tmux set-hook -g 'client-session-changed[77]' "$stop#{client_last_session}\""

if [[ -n ${TMUX:-} ]]; then
    tmux switch-client -t "=$session"
else
    tmux attach-session -t "=$session"
fi
