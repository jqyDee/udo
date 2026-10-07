#!/usr/bin/env python3
"""idea: open the node's folder in IntelliJ IDEA; a task is timed until
that project window closes, a container opened without a task is not
timed.

The Python twin of examples/run/idea.sh, line for line, for comparison.
Returns at once (`track run --detach`), so the TUI is back right away.
macOS (`open -a`), the `idea` launcher on PATH (Toolbox / Homebrew).

    cp examples/python/idea.py ~/.config/udo/run/
    udo settings set code open_with=idea

One owner per task (idea:<task id>): two projects open at once do not stop
each other's session.

Not next to idea.sh: both are named `idea` (the file stem), and udo
refuses a name that two scripts share.
"""

import os
import subprocess
import time

env = os.environ

app = "IntelliJ IDEA"
settle = 5  # seconds: the process is up before it takes projects
folder = env.get("UDO_NODE_DIR") or env["UDO_CONTAINER_DIR"]

# no task (a container on its own): just open the folder, untimed. `open
# -a` returns at once, running or not: no start-first, no settle (those
# are only for `idea --wait`)
task_id = env.get("UDO_TASK_ID", "")
if not task_id:
    os.execvp("open", ["open", "-a", app, folder])


def running() -> bool:
    """Is IntelliJ's own process up (`pgrep -qf`)?"""
    probe = ["pgrep", "-qf", f"{app}.app/Contents/MacOS/"]
    return subprocess.run(probe).returncode == 0


# not running: `idea --wait` would become the IDE itself and only return
# when all of IntelliJ quits. Start it first and give it time.
if not running():
    subprocess.run(["open", "-a", app], check=True)
    for _ in range(60):
        if running():
            break
        time.sleep(1)
    time.sleep(settle)

# the script becomes `udo track run` (`exec`)
udo = env["UDO_BIN"]
os.execv(udo, [
    udo, "track", "run", "--detach", "--task", f"id:{task_id}",
    "--source", "idea", "--owner", f"idea:{task_id}",
    "--", "idea", "--wait", folder,
])
