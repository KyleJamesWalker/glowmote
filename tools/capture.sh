#!/bin/bash
# usage: capture.sh check
#        capture.sh pull <name>             pull the phone's snoop log over adb and decode it
#        capture.sh decode <btsnoop> <name> decode a log that is already on disk
#
# Raw logs go to research/captures/ (git-ignored: they hold every Bluetooth device's traffic).
# Decoded ATT traffic goes to docs/captures/<date>-<name>.txt for review before committing.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
raw_dir=$root/research/captures
doc_dir=$root/docs/captures
today=$(date +%F)

die() { echo "error: $*" >&2; exit 1; }

phone() {
  adb get-state >/dev/null 2>&1 || die "no phone on adb; plug it in and allow USB debugging"
}

check() {
  phone
  local mode
  mode=$(adb shell getprop persist.bluetooth.btsnooplogmode | tr -d '\r')
  echo "phone: $(adb shell getprop ro.product.model | tr -d '\r')"
  echo "snoop log mode: ${mode:-unset}"
  case $mode in
    full) echo "ready: toggle Bluetooth off and on if you changed the setting, then use the vendor app" ;;
    filtered) die "set Developer options > Enable Bluetooth HCI snoop log to Enabled, not Filtered" ;;
    *) die "turn on Developer options > Enable Bluetooth HCI snoop log, then toggle Bluetooth" ;;
  esac
}

decode() {
  local log=$1 name=$2
  local out=$doc_dir/$today-$name.txt
  [ -e "$out" ] && die "$out already exists; pick another name"
  python3 -I "$root/tools/parse_snoop.py" "$log" > "$out"
  echo "decoded: $out ($(grep -c '^[0-9]' "$out") ATT packets)"
  echo "packets per connection (keep only the light's before committing):"
  grep -o 'conn=0x[0-9a-f]*' "$out" | sort | uniq -c | sed 's/^/  /'
  local predictions=$root/research/firmware/predictions/predict.txt
  [ -f "$predictions" ] || predictions=
  python3 -I "$root/tools/summarize_capture.py" "$out" $predictions
}

pull() {
  local name=$1 tmp
  phone
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  echo "collecting a bug report (about 2 minutes)..."
  (cd "$tmp" && adb bugreport bug.zip >/dev/null)
  unzip -q -j "$tmp/bug.zip" 'FS/data/misc/bluetooth/logs/btsnoop_hci.log' -d "$tmp" \
    || die "no snoop log in the bug report; is the snoop log enabled?"
  mkdir -p "$raw_dir"
  local raw=$raw_dir/$today-$name.btsnoop
  [ -e "$raw" ] && die "$raw already exists; pick another name"
  cp "$tmp/btsnoop_hci.log" "$raw"
  echo "raw log: $raw"
  decode "$raw" "$name"
  echo "remember to turn the snoop log off again"
}

case ${1:-} in
  check) check ;;
  pull) [ $# -eq 2 ] || die "usage: capture.sh pull <name>"; pull "$2" ;;
  decode) [ $# -eq 3 ] || die "usage: capture.sh decode <btsnoop> <name>"; decode "$2" "$3" ;;
  *) sed -n '2,4p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
