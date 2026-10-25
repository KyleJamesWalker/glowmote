# Ghidra analysis of JieLi AC632N firmware

Scripts behind the limits in [`docs/protocols/ideal-led.md`](../../docs/protocols/ideal-led.md#firmware-limits-tr2309r002-02).
They load a decrypted `app.bin` (from `tools/ufw_tool.py extract`) into Ghidra with the bd19
memory map and mask-ROM symbols, then find functions the auto-analysis misses.

## Setup

1. `brew install ghidra`
2. Install the JieLi processor module as a user extension:
   ```sh
   ext=~/Library/ghidra/ghidra_12.2_PUBLIC/Extensions/ghidra-jieli
   git clone https://github.com/virtualabs/ghidra-jieli "$ext"
   patch -d "$ext" -p1 < tools/ghidra/q32s.patch
   printf 'name=ghidra-jieli\nversion=12.2\n' > "$ext/extension.properties"
   ```
   `q32s.patch` adds the group-7 shifts, `clz`, compare-with-immediate branches and stack forms the
   module lacks, and masks call targets to the 25-bit bus. It also fixes the stock module's `jls` (unsigned `<=`)
   and `jge` (signed `>=`) conditions, which were inverted. With it, 9746 of 9747 instructions in
   JieLi's `rom.lst` decode to the listed length.
3. Compile it: `source tools/ghidra/env.sh && cd "$ext/data/languages" && "$G/support/sleigh" q32s.slaspec`
4. Clone [Jieli-Tech/fw-AC63_BT_SDK](https://github.com/Jieli-Tech/fw-AC63_BT_SDK) for `cpu/bd19/tools/rom.lst`
   and `cpu/bd19/maskrom_stubs.ld`.

## Use

```sh
tools/ghidra/analyze.sh <sdk dir> <app.bin> <work dir>
tools/ghidra/run.sh <work dir> Decomp.java <out.c> 1e055c8      # decompile the command parser
tools/ghidra/run.sh <work dir> Dump.java <listing.txt>          # full listing with references
```

| Script | Purpose |
|---|---|
| `Setup.java` | Pre-script: mask ROM at `0x100000`, RAM at `0`, `.data` copies, ROM symbols |
| `Discover.java` | Post-script: functions from code pointers and `push {rets}` prologues |
| `Decomp.java`, `Lin.java`, `Dump.java` | Decompile, linear disassembly, full listing |
| `RomCheck.java` | Compares the module's decoding with `rom.lst` (import `rom.bin` at `0x100000`) |
| `romlst.py`, `romsyms.py` | Rebuild the mask ROM image and symbols from the SDK listing |

`analyze.sh` uses the `.data` copies and text range of `TR2309R002-02`; another image needs the
values from its own layout.
