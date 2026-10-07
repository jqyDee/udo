#!/usr/bin/env python3
"""tmux: one tmux session per task (udo-<task id>) with a shell in the
task's folder.

The Python twin of examples/run/tmux.sh, line for line, for comparison.
Unlike nvim-tmux nothing runs in it but the shell: the session stays when
an editor in it quits, until you end it (`exit`, kill-session). The timer
runs while a client is in that session: attaching or switching in starts
it; detaching, switching away or ending the session stops it. A container
opened without a task gets a session of its own (udo-<container id>),
untimed.

    cp examples/python/tmux.py ~/.config/udo/run/
    udo settings set uni open_with=tmux

Outside tmux it attaches (udo waits until you detach); inside tmux it
switches (returns at once, the udo pane stays where it was). The hook
rules are explained in nvim-tmux.sh and docs/run-configs.md; nvim-tmux
sets the same global hooks at the same index with the same commands.

Not next to tmux.sh: both are named `tmux` (the file stem), and udo
refuses a name that two scripts share.
"""

import os
import subprocess
import sys

env = os.environ


def tmux(*args: str) -> None:
    """A tmux command that must work (`set -e`)."""
    code = subprocess.run(["tmux", *args]).returncode
    if code != 0:
        sys.exit(code)


# a task: its session, timed. No task (a container on its own): the
# container's session, untimed. By id, not name: a rename keeps the
# session, and opening a task directly or through its container is the
# same session
task_id = env.get("UDO_TASK_ID", "")
session = f"udo-{task_id or env['UDO_NODE_ID']}"
owner = f"tmux:{session}"
folder = env.get("UDO_TASK_DIR") or env.get("UDO_NODE_DIR") or env["UDO_CONTAINER_DIR"]

# hooks run in the tmux server: neither its PATH nor its environment need
# to know this udo, so name the binary and the root
udo = f"UDO_ROOT='{env['UDO_ROOT']}' '{env['UDO_BIN']}'"

# `=`: the exact name, not a prefix. set-option / set-hook take a pane
# target, so theirs ends in `:` (`=name` alone: "no such session")
exists = subprocess.run(
    ["tmux", "has-session", "-t", f"={session}"], stderr=subprocess.DEVNULL
).returncode == 0
if not exists:
    tmux("new-session", "-d", "-s", session, "-c", folder)  # a shell: stays
    if task_id:
        tmux("set-option", "-t", f"={session}:", "@udo_task", task_id)
        start = f'run-shell -b "{udo} track start --task id:{task_id} --source tmux --owner {owner}"'
        tmux("set-hook", "-t", f"={session}:", "client-session-changed", start)
        tmux("set-hook", "-t", f"={session}:", "client-attached", start)

# server-wide, every time: the server may be new since the last run.
# Without a task too: leaving a task's session for this one must stop it.
# Which format names the session differs per hook: see nvim-tmux.sh
stop = f'run-shell -b "{udo} track stop --owner tmux:'
tmux("set-hook", "-g", "client-detached[77]", stop + '#{session_name}"')
tmux("set-hook", "-g", "session-closed[77]", stop + '#{hook_session_name}"')
tmux("set-hook", "-g", "client-session-changed[77]", stop + '#{client_last_session}"')

# the last command: the script becomes it (its exit code is ours)
if env.get("TMUX"):
    os.execvp("tmux", ["tmux", "switch-client", "-t", f"={session}"])
else:
    os.execvp("tmux", ["tmux", "attach-session", "-t", f"={session}"])
