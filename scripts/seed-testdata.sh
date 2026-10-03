#!/usr/bin/env bash
# Create a fresh, throwaway udo test dataset.
#
#   scripts/seed-testdata.sh                 # root /tmp/udo-test, data /tmp/udo-data
#   UDO_ROOT=/tmp/a UDO_DATA=/tmp/b scripts/seed-testdata.sh
#
# Then use it with:  UDO_ROOT=/tmp/udo-test cargo run [-- ls]
#
# Dates are relative to today, so the data never goes stale: something is
# always overdue, due soon, or done. What it covers:
# - tree: root tasks, two workspaces, projects (one empty), a done task,
#   tasks with and without folders, a name with a space
# - settings: task_folders, default_deadline, archive_dir, estimate
# - time: sessions over the last days, and a running timer started by a
#   program (tmux), so the status line shows `▶ lexer · … · tmux`
# - run configs: the examples installed, plus `hello` (prints what a
#   script learns, then waits for Enter): `o` works on every node.
#   `algorithms` opens with nvim-tmux (sets hooks on your tmux server),
#   `compilers` with zed, `thesis` runs typst-setup on create
set -euo pipefail

export UDO_ROOT="${UDO_ROOT:-/tmp/udo-test}"
DATA="${UDO_DATA:-/tmp/udo-data}"

# Both dirs get wiped, so only allow throwaway locations.
for d in "$UDO_ROOT" "$DATA"; do
  case "$d" in
    *..*) echo "refusing to wipe $d (no '..' allowed)" >&2; exit 1 ;;
    /tmp/?* | /private/tmp/?*) ;;
    *) echo "refusing to wipe $d (only /tmp/... allowed)" >&2; exit 1 ;;
  esac
done

cd "$(dirname "$0")/.."
cargo build -q
udo=./target/debug/udo

# YYYY-MM-DD, `$1` days from today (+3, -2, +0)
day() {
  if date -v+0d +%F >/dev/null 2>&1; then
    date -v"$1"d +%F # BSD / macOS
  else
    date -d "$1 days" +%F # GNU
  fi
}

rm -rf "$UDO_ROOT" "$DATA"

{
  # root tasks
  $udo add task pay-rent --due "$(day +2) 09:00"
  $udo add task return-library-books --due "$(day -3) 18:00" # overdue
  $udo add task call-grandma --due "$(day -1) 20:00"
  $udo "done" call-grandma # quoted: shellcheck reads a bare `done` as the keyword

  # uni: workspace with archive, task folders, a deadline rule, three projects
  $udo add workspace uni --dir "$DATA/uni"
  $udo settings set uni task_folders=auto default_deadline="+7d 23:59" \
    archive_dir="$DATA/uni/archive"
  $udo add task uni/enrol-exams --due "$(day +11) 23:59"

  $udo add project uni/algorithms
  $udo settings set uni/algorithms estimate=3h
  $udo add task uni/algorithms/sheet-3 --due "$(day -2) 12:00" # overdue
  $udo add task uni/algorithms/sheet-4 --due "$(day +5) 12:00"
  $udo add task uni/algorithms/exam-prep --due "$(day +130) 09:00" --no-dir

  $udo add project uni/compilers
  $udo add task uni/compilers/lexer --due "$(day +1) 18:00"
  $udo add task "uni/compilers/parser tests" --due "$(day +15) 18:00"
  $udo add task uni/compilers/setup-toolchain --due "$(day -6) 12:00"
  $udo "done" setup-toolchain

  $udo add project uni/thesis # empty project

  # home: workspace with tasks only, no folders
  $udo add workspace home --dir "$DATA/home"
  $udo add task home/tax-return --due "$(day +27) 12:00" --no-dir
  $udo add task home/dentist --due "$(day +31) 08:30" --no-dir

  # time recorded over the last days (no overlaps: one timer at a time)
  $udo session add sheet-3 "$(day -4) 09:00" "$(day -4) 11:15"
  $udo session add sheet-3 "$(day -3) 14:00" "$(day -3) 15:40"
  $udo session add sheet-4 "$(day -2) 10:00" "$(day -2) 12:30"
  $udo session add sheet-4 "$(day -1) 16:00" "$(day -1) 17:05"
  $udo session add setup-toolchain "$(day -7) 19:00" "$(day -7) 21:45"
  $udo session add lexer "$(day -1) 09:30" "$(day -1) 11:00"
  $udo session add tax-return "$(day -5) 18:00" "$(day -5) 18:45"

  # run configs: the examples, a template for typst-setup, and `hello`
  mkdir -p "$UDO_ROOT/run" "$UDO_ROOT/templates"
  cp examples/run/*.sh "$UDO_ROOT/run/"
  cp -R examples/templates/typst "$UDO_ROOT/templates/"
  cat >"$UDO_ROOT/run/hello.sh" <<'EOF'
#!/usr/bin/env bash
# hello: shows what a run config learns, then waits (try `o` in the TUI)
echo "udo ran hello for: $UDO_EVENT"
env | grep '^UDO_' | sort
echo "working folder: $PWD"
read -rp "Enter to return to udo "
EOF
  chmod +x "$UDO_ROOT/run/hello.sh"
  $udo settings set / open_with=hello
  $udo settings set uni/algorithms open_with=nvim-tmux
  $udo settings set uni/compilers open_with=zed
  $udo settings set uni/thesis on_create=typst-setup

  # last: a timer a program started (as a tmux hook would), still running
  $udo track start --task lexer --source tmux --owner tmux:seed
} >/dev/null

echo "Test data ready:  root $UDO_ROOT, data $DATA"
echo
$udo ls
echo
$udo status
$udo run --list /
echo
echo "Open the TUI:  UDO_ROOT=$UDO_ROOT cargo run"
