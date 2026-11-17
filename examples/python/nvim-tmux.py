#!/usr/bin/env python3
"""nvim-tmux: one tmux session per task (udo-<container>/<task>) with nvim
in the task's folder.

The Python twin of examples/run/nvim-tmux.sh, line for line, for
comparison. The timer runs while a client is in that session: attaching or
switching in starts it; detaching, switching away or quitting nvim stops
it. A container opened without a task gets a session of its own
(udo-<container>), untimed.

    cp examples/python/nvim-tmux.py ~/.config/udo/run/
    udo settings set uni open_with=nvim-tmux

Outside tmux it attaches (udo waits until you detach); inside tmux it
switches (returns at once, the udo pane stays where it was). The hook
rules (which format names the session in which hook, `=name:` targets)
and the session names are explained in nvim-tmux.sh and
docs/run-configs.md.

Not next to nvim-tmux.sh: both are named `nvim-tmux` (the file stem), and
udo refuses a name that two scripts share.
"""

import os
import re
import subprocess
import sys

env = os.environ


def tmux(*args: str) -> None:
    """A tmux command that must work (`set -e`)."""
    code = subprocess.run(["tmux", *args]).returncode
    if code != 0:
        sys.exit(code)


editor = "nvim ."  # what runs in the session (it ends: the session ends)
# a task: its session, timed. No task (a container on its own): the
# container's session, untimed
task_id = env.get("UDO_TASK_ID", "")
key = task_id or env["UDO_NODE_ID"]
if task_id:
    # an older udo has no UDO_CONTAINER_NAME: the folder's name
    container = env.get("UDO_CONTAINER_NAME") or os.path.basename(env["UDO_CONTAINER_DIR"])
    label = f"udo-{container}/{env['UDO_TASK_NAME']}"
else:
    label = f"udo-{env['UDO_NODE_NAME']}"
label = re.sub(r"[^\w/-]", "_", label)
folder = env.get("UDO_TASK_DIR") or env.get("UDO_NODE_DIR") or env["UDO_CONTAINER_DIR"]

# hooks run in the tmux server: neither its PATH nor its environment need
# to know this udo, so name the binary, the root and the data dir
udo = f"UDO_ROOT='{env['UDO_ROOT']}' UDO_DATA='{env['UDO_DATA']}' '{env['UDO_BIN']}'"


def exists(name: str) -> bool:
    """`tmux has-session` for exactly `name`."""
    return subprocess.run(
        ["tmux", "has-session", "-t", f"={name}"], stderr=subprocess.DEVNULL
    ).returncode == 0


# this node's session, if there is one (no server yet: none)
listed = subprocess.run(
    ["tmux", "list-sessions", "-F", "#{@udo_key}|#{session_name}"],
    capture_output=True, text=True,
).stdout
session = next(
    (name for k, _, name in (l.partition("|") for l in listed.splitlines()) if k == key),
    "",
)

# `=`: the exact name, not a prefix. set-option / set-hook take a pane
# target, so theirs ends in `:` (`=name` alone: "no such session")
if not session:
    session = label
    # the name taken (the same names in another container): add the id
    if exists(session):
        session = f"{label}-{key}"
    owner = f"tmux:{session}"
    tmux("new-session", "-d", "-s", session, "-c", folder, editor)
    tmux("set-option", "-t", f"={session}:", "@udo_key", key)
    if task_id:
        tmux("set-option", "-t", f"={session}:", "@udo_task", task_id)
        start = f'run-shell -b "{udo} track start --task id:{task_id} --source tmux --owner {owner}"'
        tmux("set-hook", "-t", f"={session}:", "client-session-changed", start)
        tmux("set-hook", "-t", f"={session}:", "client-attached", start)

# server-wide, every time: the server may be new since the last run.
# Without a task too: leaving a task's session for this one must stop it.
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
