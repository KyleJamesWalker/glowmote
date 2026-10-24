"""Summarize a decoded capture (tools/parse_snoop.py output) by command tag.

usage: python3 -I tools/summarize_capture.py <decoded.txt> [predict.txt]

Each command tag is marked confirmed (in docs/protocols/ideal-led.md), predicted (its ciphertext
matches a line of predict.txt) or NEW. Plaintext bulk frames are listed by handle and size.
"""
import re
import sys
from collections import Counter, defaultdict

CONFIRMED = {"TURN", "SGLS", "MULT", "DOOD", "DEN", "DSTOP", "TIME", "VE", "CT"}
LINE = re.compile(r"^\S+ conn=(\S+) (->|<-) (\S+)\s+h=(\S+) raw=(\S*)$")


def tag_of(plain: bytes) -> str:
    text = plain[1:].split(b"\0")[0]
    return re.match(rb"[A-Z]*", text).group().decode() or plain[:4].hex()


def load_predictions(path: str) -> dict:
    predicted = {}
    for line in open(path):
        m = re.match(r"^(\S.*?)\s{2,}((?:[0-9a-f]{2} ){15}[0-9a-f]{2})\s+([0-9a-f]{32})$", line.rstrip())
        if m:
            predicted[m.group(3)] = m.group(1)
    return predicted


def main(decoded: str, predictions: str | None) -> None:
    predicted = load_predictions(predictions) if predictions else {}
    lines = open(decoded).read().splitlines()
    commands = defaultdict(Counter)
    replies = Counter()
    frames = Counter()
    for i, line in enumerate(lines):
        m = LINE.match(line)
        if not m:
            continue
        _conn, direction, op, handle, raw = m.groups()
        dec = lines[i + 1].strip() if i + 1 < len(lines) else ""
        if dec.startswith("dec="):
            plain = bytes.fromhex(dec[4:].split("|")[0].replace(" ", ""))
            if direction == "->":
                commands[tag_of(plain)][(plain.hex(" "), raw)] += 1
            else:
                replies[tag_of(plain)] += 1
        elif raw and op.startswith("WRITE"):
            frames[(handle, len(raw) // 2)] += 1

    print("commands:")
    for tag in sorted(commands):
        variants = commands[tag]
        hits = {predicted[raw] for _, raw in variants if raw in predicted}
        status = "confirmed" if tag in CONFIRMED else ("predicted" if hits else "NEW")
        print(f"  {tag:10} {status:9} {sum(variants.values()):4} sent, {len(variants)} distinct")
        if status != "confirmed":
            for (plain, raw), n in variants.most_common():
                note = f"  = {predicted[raw]}" if raw in predicted else ""
                print(f"      {n:3}x {plain}{note}")
    print("replies:")
    for tag, n in sorted(replies.items()):
        print(f"  {tag:10} {n:4}")
    if frames:
        print("plaintext writes:")
        for (handle, size), n in sorted(frames.items()):
            print(f"  h={handle} {size:3} bytes x{n}")


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2] if len(sys.argv) == 3 else None)
