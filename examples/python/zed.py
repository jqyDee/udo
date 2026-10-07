#!/usr/bin/env python3
"""zed: open the node's folder in Zed; a task is timed until that window
closes, a container opened without a task is not timed.

The Python twin of examples/run/zed.sh, line for line, for comparison.
Returns at once (`track run --detach`), so the TUI is back right away.
Needs the `zed` CLI on PATH (Zed: "Install CLI").

    cp examples/python/zed.py ~/.config/udo/run/
    udo settings set code open_with=zed

`--new`: a window of its own for the folder. Without it, `zed --wait`
returns only when all of Zed quits (Zed running or not, tried by hand
2026-10-04); with it, when that window closes. One owner per task
(zed:<task id>): two windows open at once do not stop each other's session.

Not next to zed.sh: both are named `zed` (the file stem), and udo refuses a
name that two scripts share.
"""

import os

env = os.environ

folder = env.get("UDO_NODE_DIR") or env["UDO_CONTAINER_DIR"]

# no task (a container on its own): just the window, untimed
task_id = env.get("UDO_TASK_ID", "")
if not task_id:
    os.execvp("zed", ["zed", "--new", folder])

# the script becomes `udo track run` (`exec`)
udo = env["UDO_BIN"]
os.execv(udo, [
    udo, "track", "run", "--detach", "--task", f"id:{task_id}",
    "--source", "zed", "--owner", f"zed:{task_id}",
    "--", "zed", "--new", "--wait", folder,
])
