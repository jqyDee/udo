#!/usr/bin/env bash
# typst-setup: on_create for typst tasks. Copies the template folder
# <root>/templates/typst/ into the new task's folder; files already there
# are kept. Does nothing for containers, tasks without a folder, or when
# opened: it is inherited by everything below where it is set.
#
#   cp examples/run/typst-setup.sh ~/.config/udo/run/
#   udo settings set uni on_create=typst-setup
#
# Another template: copy this script and change `template`. Scripts only
# get the UDO_* variables reliably; anything else from your shell is only
# there if udo was started from that shell.
set -euo pipefail

[[ ${UDO_EVENT:-} == create && ${UDO_NODE_KIND:-} == task ]] || exit 0
[[ -n ${UDO_NODE_DIR:-} ]] || exit 0 # a task without a folder

template=$UDO_ROOT/templates/typst
if [[ ! -d $template ]]; then
    # set up on purpose, so a missing template is a mistake: exit 1 is a
    # warning in `udo add`, a toast in the TUI (the task stays)
    echo "typst-setup: no template at $template" >&2
    exit 1
fi

cp -Rn "$template/." "$UDO_NODE_DIR/" # -n: never overwrite
echo "typst-setup: $UDO_NODE_NAME ready"
