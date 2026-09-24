#!/usr/bin/env bash
# Create a fresh, throwaway udo test dataset.
#
#   scripts/seed-testdata.sh                 # root /tmp/udo-test, data /tmp/udo-data
#   UDO_ROOT=/tmp/a UDO_DATA=/tmp/b scripts/seed-testdata.sh
#
# Then use it with:  UDO_ROOT=/tmp/udo-test cargo run [-- list]
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

rm -rf "$UDO_ROOT" "$DATA"

{
  # root tasks
  $udo add-task -t pay-rent -d "2026-10-03 09:00"
  $udo add-task -t return-library-books -d "2026-09-20 18:00" # overdue

  # uni: workspace with archive, three projects
  $udo create-workspace -n uni -d "$DATA/uni" -a "$DATA/uni/archive"
  $udo add-task -w uni -t enrol-exams -d "2026-10-15 23:59"

  $udo add-project -w uni -p algorithms
  $udo add-task -w uni -p algorithms -t sheet-3 -d "2026-10-01 12:00"
  $udo add-task -w uni -p algorithms -t sheet-4 -d "2026-10-08 12:00"
  $udo add-task -w uni -p algorithms -t exam-prep -d "2027-02-10 09:00" --no-auto-create-folder

  $udo add-project -w uni -p compilers
  $udo add-task -w uni -p compilers -t lexer -d "2026-10-05 18:00"
  $udo add-task -w uni -p compilers -t parser -d "2026-10-19 18:00"

  $udo add-project -w uni -p thesis # empty project

  # home: workspace with tasks only
  $udo create-workspace -n home -d "$DATA/home"
  $udo add-task -w home -t tax-return -d "2026-10-31 12:00"
  $udo add-task -w home -t dentist -d "2026-11-04 08:30"
} >/dev/null

echo "Test data ready:  root $UDO_ROOT, data $DATA"
echo
$udo list
echo
echo "Open the TUI:  UDO_ROOT=$UDO_ROOT cargo run"
