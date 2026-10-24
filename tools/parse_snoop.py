"""Dump ATT writes/notifications from a btsnoop log, AES-decrypting 16-byte payloads."""
import struct
import sys
from datetime import datetime

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

KEY = bytes.fromhex("34522a5b7a6e492c08090a9d8d2a23f8")
EPOCH_DELTA_US = 0x00DCDDB30F2F8000
ATT_OPS = {0x12: "WRITE_REQ", 0x52: "WRITE_CMD", 0x1B: "NOTIFY", 0x1D: "INDICATE", 0x0A: "READ_REQ", 0x0B: "READ_RSP"}


def decrypt(block: bytes) -> bytes:
    d = Cipher(algorithms.AES(KEY), modes.ECB()).decryptor()
    return d.update(block) + d.finalize()


def printable(b: bytes) -> str:
    return "".join(chr(c) if 32 <= c < 127 else "." for c in b)


def main(path: str) -> None:
    with open(path, "rb") as f:
        data = f.read()
    off = 16
    while off + 24 <= len(data):
        _orig, incl, flags, _drops, ts = struct.unpack(">IIIIq", data[off : off + 24])
        pkt = data[off + 24 : off + 24 + incl]
        off += 24 + incl
        if not pkt or pkt[0] != 0x02 or len(pkt) < 10:
            continue
        hdl_flags, _acl_len = struct.unpack("<HH", pkt[1:5])
        if (hdl_flags >> 12) & 0x3 == 0x1:
            continue
        _l2_len, cid = struct.unpack("<HH", pkt[5:9])
        if cid != 4:
            continue
        att = pkt[9:]
        op = att[0]
        if op not in ATT_OPS or len(att) < 3:
            continue
        att_handle = struct.unpack("<H", att[1:3])[0]
        value = att[3:]
        when = datetime.fromtimestamp((ts - EPOCH_DELTA_US) / 1e6).strftime("%H:%M:%S.%f")[:-3]
        direction = "<-" if flags & 1 else "->"
        conn = hdl_flags & 0x0FFF
        line = f"{when} conn={conn:#05x} {direction} {ATT_OPS[op]:9} h={att_handle:#06x} raw={value.hex()}"
        if len(value) == 16:
            plain = decrypt(value)
            line += f"\n{'':33}dec={plain.hex(' ')}  |{printable(plain)}|"
        print(line)


if __name__ == "__main__":
    main(sys.argv[1])
