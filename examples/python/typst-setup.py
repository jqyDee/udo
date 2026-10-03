#!/usr/bin/env python3
"""typst-setup: on_create for typst tasks.

The Python twin of examples/run/typst-setup.sh, line for line, for
comparison. Copies the template folder <root>/templates/typst/ into the new
task's folder; files already there are kept. Does nothing for containers,
tasks without a folder, or when opened: it is inherited by everything below
where it is set.

    cp examples/python/typst-setup.py ~/.config/udo/run/
    udo settings set uni on_create=typst-setup

Not next to typst-setup.sh: both are named `typst-setup` (the file stem),
and udo refuses a name that two scripts share.
"""

import os
import shutil
import sys
from pathlib import Path

env = os.environ

if env.get("UDO_EVENT") != "create" or env.get("UDO_NODE_KIND") != "task":
    sys.exit(0)
node_dir = env.get("UDO_NODE_DIR", "")
if not node_dir:
    sys.exit(0)  # a task without a folder

template = Path(env["UDO_ROOT"]) / "templates" / "typst"
if not template.is_dir():
    # set up on purpose, so a missing template is a mistake: exit 1 is a
    # warning in `udo add`, a toast in the TUI (the task stays)
    print(f"typst-setup: no template at {template}", file=sys.stderr)
    sys.exit(1)

# `cp -Rn`: everything below the template, never overwriting
for src in sorted(template.rglob("*")):
    dst = Path(node_dir) / src.relative_to(template)
    if src.is_dir():
        dst.mkdir(parents=True, exist_ok=True)
    elif not dst.exists():
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)

print(f"typst-setup: {env['UDO_NODE_NAME']} ready")
