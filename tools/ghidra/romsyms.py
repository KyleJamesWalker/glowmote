# rom.lst labels + maskrom_stubs.ld -> "hexaddr name" lines
import re, sys
lst, stubs = sys.argv[1], sys.argv[2]
out, pend = {}, None
for line in open(lst, errors='replace'):
    m = re.match(r'^([A-Za-z_.$][^ ]*):$', line.strip('\n'))
    if m: pend = m.group(1); continue
    m = re.match(r'^\s+([0-9a-f]+):', line)
    if m and pend: out.setdefault(int(m.group(1), 16), pend); pend = None
for line in open(stubs):
    m = re.match(r'\s*(\w+)\s*=\s*ABSOLUTE\((0x[0-9a-f]+)\)', line)
    if m: out.setdefault(int(m.group(2), 16), m.group(1))
for a in sorted(out): print(f'{a:x} {out[a]}')
