#!/bin/bash
# usage: analyze.sh <fw-AC63_BT_SDK dir> <decrypted app.bin> <work dir>
# Builds the bd19 mask ROM from the SDK listing, then imports app.bin into <work dir>/proj (project "app").
# The .data copies and text range are those of TR2309R002-02; other images need their own values.
set -euo pipefail
source "$(dirname "$0")/env.sh"
sdk=$1 app=$2 work=$3
mkdir -p "$work/rom" "$work/proj"
python3 -I "$HERE/romlst.py" "$sdk/cpu/bd19/tools/rom.lst" "$work/rom"
python3 -I "$HERE/romsyms.py" "$sdk/cpu/bd19/tools/rom.lst" "$sdk/cpu/bd19/maskrom_stubs.ld" > "$work/rom/syms.txt"
"$G/support/analyzeHeadless" "$work/proj" app \
  -import "$app" -overwrite \
  -processor q32s:LE:32:default -loader BinaryLoader -loader-baseAddr 0x1e00120 -loader-blockName flash \
  -scriptPath "$HERE/scripts" \
  -preScript Setup.java "$work/rom" 0x1e210a0 0x840 0xc80 0x1e21d20 0x6e40 0x1f0c \
  -postScript Discover.java 0x1e00120 0x1e1617c 0x6e40 0x8d4c
