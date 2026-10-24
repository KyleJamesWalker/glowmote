# Parse JieLi rom.lst (llvm-objdump q32s) into rom.bin + instruction table (addr, hex, text)
import re, sys, json
src, outdir = sys.argv[1], sys.argv[2]
rx = re.compile(r'^\s+([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*\t(.*)$')
ins, mem = [], {}
for line in open(src, errors='replace'):
    m = rx.match(line.rstrip('\n'))
    if not m: continue
    a = int(m.group(1), 16); bs = bytes.fromhex(m.group(2).replace(' ', ''))
    text = m.group(3).split('##')[0].strip()
    for i, b in enumerate(bs): mem[a+i] = b
    ins.append((a, bs.hex(), re.sub(r'\s+', ' ', text)))
lo, hi = min(mem), max(mem)+1
buf = bytearray(b'\xff'*(hi-lo))
for a, b in mem.items(): buf[a-lo] = b
open(f'{outdir}/rom.bin', 'wb').write(buf)
with open(f'{outdir}/rom_ins.tsv', 'w') as f:
    for a, h, t in ins: f.write(f'{a:x}\t{h}\t{t}\n')
print(hex(lo), hex(hi), len(ins))
