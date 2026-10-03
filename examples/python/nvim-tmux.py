#!/usr/bin/env python3
"""nvim-tmux: one tmux session per task (udo-<task id>) with nvim in the
task's folder.

The Python twin of examples/run/nvim-tmux.sh, line for line, for
comparison. The timer runs while a client is in that session: attaching or
switching in starts it; detaching, switching away or quitting nvim stops
it.

    cp examples/python/nvim-tmux.py ~/.config/udo/run/
    udo settings set uni open_with=nvim-tmux

Outside tmux it attaches (udo waits until you detach); inside tmux it
switches (returns at once, the udo pane stays where it was). The hook
rules (which format names the session in which hook, `=name:` targets)
are explained in nvim-tmux.sh and docs/run-configs.md.

Not next to nvim-tmux.sh: both are named `nvim-tmux` (the file stem), and
udo refuses a name that two scripts share.
"""

import os
import subprocess
import sys

env = os.environ


def need(var: str, why: str) -> str:
    """`${var:?why}`: the value, or stop with the reason."""
    value = env.get(var, "")
    if not value:
        sys.exit(f"{var}: {why}")
    return value


def tmux(*args: str) -> None:
    """A tmux command that must work (`set -e`)."""
    code = subprocess.run(["tmux", *args]).returncode
    if code != 0:
        sys.exit(code)


task_id = need("UDO_TASK_ID", "nvim-tmux opens a task (on a container: pick one)")

editor = "nvim ."  # what runs in the session (it ends: the session ends)
session = f"udo-{task_id}"
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
    tmux("new-session", "-d", "-s", session, "-c", folder, editor)
    tmux("set-option", "-t", f"={session}:", "@udo_task", task_id)
    start = f'run-shell -b "{udo} track start --task id:{task_id} --source tmux --owner {owner}"'
    tmux("set-hook", "-t", f"={session}:", "client-session-changed", start)
    tmux("set-hook", "-t", f"={session}:", "client-attached", start)

# server-wide, every time: the server may be new since the last run.
# Which format names the session differs per hook (tmux 3.6a):
# client-detached: #{session_name}; session-closed: #{hook_session_name};
# switching away: #{client_last_session}
stop = f'run-shell -b "{udo} track stop --owner tmux:'
tmux("set-hook", "-g", "client-detached[77]", stop + '#{session_name}"')
tmux("set-hook", "-g", "session-closed[77]", stop + '#{hook_session_name}"')
tmux("set-hook", "-g", "client-session-changed[77]", stop + '#{client_last_session}"')

# the last command: the script becomes it (its exit code is ours)
if env.get("TMUX"):
    os.execvp("tmux", ["tmux", "switch-client", "-t", f"={session}"])
else:
    os.execvp("tmux", ["tmux", "attach-session", "-t", f"={session}"])
