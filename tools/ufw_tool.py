#!/usr/bin/env python3
"""Decode JieLi .ufw OTA images (AC632N / AC636N) and decrypt the flash images inside.

Usage:
    python3 -I tools/ufw_tool.py list <file.ufw>
    python3 -I tools/ufw_tool.py extract <file.ufw> <outdir>

Layers (all use JieLi's CRC-16 stream cipher, see `jl_crypt`):
  .ufw container  header 0x40 B, then 0x50-B entries; each record encrypted alone with key 0xffff.
  flash*.bin      JieLi flash image: 0x20-B header and 0x20-B file entries, each encrypted alone
                  with 0xffff. uboot.boot: 16-B header + body, each with 0xffff.
  small entries   isd_config.ini, script.ver, farg.cfg, blimit.bin: 32-B blocks at absolute .ufw
                  offset `o` encrypted with `chip_key ^ ((o >> 2) & 0xffff)`.
  ota.bin, tail   plaintext.
  app area        starts at the `app_dir_head` offset. Every 32-B block at offset `o` from the
                  area start is encrypted with `chip_key ^ ((o >> 2) & 0xffff)`. The chip key is
                  per flash image and recovered from the known `app_area_head` entry name.
"""
import os
import struct
import sys


def keystream(key, n):
    out = bytearray(n)
    for i in range(n):
        out[i] = key & 0xFF
        key = ((key << 1) ^ (0x1021 if key & 0x8000 else 0)) & 0xFFFF
    return bytes(out)


def jl_crypt(data, key):
    return bytes(a ^ b for a, b in zip(data, keystream(key, len(data))))


def crc16(data, crc=0):
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def cstr(b):
    return b.split(b"\0")[0].split(b"\xff")[0].decode("latin-1")


def safe_name(name):
    """Entry names come from the image; keep them inside the output directory."""
    return os.path.basename(name.replace("\\", "/")) or "unnamed"


# ---------------------------------------------------------------- .ufw container

def parse_ufw(d):
    h = jl_crypt(d[:0x40], 0xFFFF)
    hdr = {
        "hdr_crc": struct.unpack_from("<H", h, 0)[0],
        "hdr_crc_ok": struct.unpack_from("<H", h, 0)[0] == crc16(h[2:0x40]),
        "list_crc": struct.unpack_from("<H", h, 2)[0],
        "file_size": struct.unpack_from("<I", h, 4)[0],
        "count": struct.unpack_from("<H", h, 8)[0],
        "unk_0a": struct.unpack_from("<H", h, 10)[0],
        "unk_0c": struct.unpack_from("<H", h, 12)[0],
        "chip": cstr(h[0x10:0x40]),
    }
    entries = []
    for i in range(hdr["count"]):
        off = 0x40 + i * 0x50
        r = jl_crypt(d[off:off + 0x50], 0xFFFF)
        typ, res, idx, crc, _pad, foff, size, alloc, unk18, unk1c = struct.unpack_from("<BBHHHIIIII", r, 0)
        e = dict(index=idx, type=typ, res=res, crc=crc, offset=foff, size=size, alloc=alloc,
                 unk18=unk18, unk1c=unk1c, name=cstr(r[0x40:0x50]), rec_off=off)
        e["crc_ok"] = crc16(d[foff:foff + size]) == crc if size else crc == 0
        entries.append(e)
    return hdr, entries


# ---------------------------------------------------------------- JieLi flash image

def parse_flash_entry(r):
    hcrc, dcrc, off, size, attr, res, idx = struct.unpack_from("<HHIIBBH", r, 0)
    return dict(hdr_crc=hcrc, hdr_crc_ok=hcrc == crc16(r[2:32]), data_crc=dcrc, offset=off,
                size=size, attr=attr, res=res, idx=idx, name=cstr(r[16:32]))


def find_chip_key(area_block0):
    want = b"app_area_head\0"
    for k in range(0x10000):
        if jl_crypt(area_block0[:30], k)[16:30] == want:
            return k
    return None


def decrypt_area(buf, start, end, key):
    """Decrypt [start, end) of an app area that begins at `start` (32-B blocks, address-keyed)."""
    out = bytearray()
    for o in range(0, end - start, 32):
        out += jl_crypt(buf[start + o:min(start + o + 32, end)], key ^ ((o >> 2) & 0xFFFF))
    return bytes(out)


def parse_flash(fb):
    raw_hdr = fb[:0x20]
    h = jl_crypt(raw_hdr, 0xFFFF)
    info = {
        "hdr_crc": struct.unpack_from("<H", h, 0)[0],
        "hdr_crc_ok": struct.unpack_from("<H", h, 0)[0] == crc16(h[2:32]),
        "hdr_plain": h[:16].hex(),
        # bytes 0x10..0x1f are stored so that the raw flash shows the firmware version in ASCII
        "version": cstr(raw_hdr[0x10:0x20]),
    }
    files = []
    o = 0x20
    while o + 32 <= len(fb):
        e = parse_flash_entry(jl_crypt(fb[o:o + 32], 0xFFFF))
        if not e["hdr_crc_ok"]:
            break
        files.append(e)
        o += 32
    info["files"] = files

    dec = bytearray(fb)
    dec[:0x20] = h
    dec[0x20:o] = b"".join(jl_crypt(fb[i:i + 32], 0xFFFF) for i in range(0x20, o, 32))
    blobs = {}
    for f in files:
        name, off, size = f["name"], f["offset"], f["size"]
        if name == "uboot.boot":
            p = jl_crypt(fb[off:off + 16], 0xFFFF) + jl_crypt(fb[off + 16:off + size], 0xFFFF)
            dec[off:off + size] = p
            f["data_crc_ok"] = crc16(p) == f["data_crc"]
            blobs[name] = p
        elif name == "isd_config.ini":
            # 0x24-byte binary prefix (same bytes as the .ufw tail.bin) followed by plaintext TLVs
            p = fb[off:off + size]
            f["data_crc_ok"] = crc16(p) == f["data_crc"]
            blobs[name] = p
        elif name == "app_dir_head":
            info["area_start"] = off
        elif size and off + size <= len(fb) and size != 0xFFFFFFFF:
            blobs[name] = fb[off:off + size]

    area = []
    if "area_start" in info:
        a = info["area_start"]
        key = find_chip_key(fb[a:a + 32])
        info["chip_key"] = key
        if key is not None:
            plain = decrypt_area(fb, a, len(fb), key)
            dec[a:] = plain
            o = 0
            while o + 32 <= len(plain):
                e = parse_flash_entry(plain[o:o + 32])
                if not e["hdr_crc_ok"]:
                    break
                area.append(e)
                o += 32
            for e in area:
                if e["name"] == "app_area_head":
                    e["load_addr"] = e["offset"]
                    continue
                if e["data_crc"] == 0xFFFF or e["offset"] + e["size"] > len(plain):
                    continue
                p = plain[e["offset"]:e["offset"] + e["size"]]
                e["data_crc_ok"] = crc16(p) == e["data_crc"]
                blobs["area/" + e["name"]] = p
    info["area"] = area
    return info, bytes(dec), blobs


# ---------------------------------------------------------------- JieLi ota.bin (plaintext)

def parse_ota(d):
    out = []
    o = 0
    while o + 32 <= len(d):
        hcrc, dcrc, off, size, typ, idx = struct.unpack_from("<HHIIHH", d, o)
        name = cstr(d[o + 16:o + 32])
        if not name or hcrc != crc16(d[o + 2:o + 32]) or (out and o >= min(f["offset"] for f in out)):
            break
        out.append(dict(name=name, offset=off, size=size, type=typ, idx=idx, data_crc=dcrc,
                        data_crc_ok=crc16(d[off:off + size]) == dcrc if size < 0x1000000 else None))
        o += 32
    return out


def decrypt_ufw_blocks(d, off, size, key):
    out = bytearray()
    for o in range(off, off + size, 32):
        out += jl_crypt(d[o:min(o + 32, off + size)], key ^ ((o >> 2) & 0xFFFF))
    return bytes(out)


PLAIN_ENTRIES = {"ota.bin", "tail.bin", "info.log"}


def ufw_chip_key(d, entries):
    for e in entries:
        if e["name"].startswith("flash") and e["name"].endswith(".bin"):
            info, _, _ = parse_flash(d[e["offset"]:e["offset"] + e["size"]])
            if info.get("chip_key") is not None:
                return info["chip_key"]
    return None


def entry_plain(d, e, key):
    """Plaintext of a small (non-flash) .ufw entry and whether its CRC verifies."""
    if e["name"] in PLAIN_ENTRIES or key is None:
        return d[e["offset"]:e["offset"] + e["size"]], None
    p = decrypt_ufw_blocks(d, e["offset"], e["size"], key)
    return p, crc16(p) == e["crc"]


# ---------------------------------------------------------------- CLI

def cmd_list(path):
    d = open(path, "rb").read()
    hdr, entries = parse_ufw(d)
    key = ufw_chip_key(d, entries)
    for e in entries:
        if not e["crc_ok"] and not e["name"].startswith("flash"):
            e["crc_ok"] = entry_plain(d, e, key)[1]
    print(f"{os.path.basename(path)}: chip {hdr['chip']}, size {hdr['file_size']:#x} "
          f"(actual {len(d):#x}), {hdr['count']} entries, hdr_crc {hdr['hdr_crc']:#06x} "
          f"{'ok' if hdr['hdr_crc_ok'] else 'BAD'}")
    print(f"  {'idx':>3} {'type':>4} {'offset':>8} {'size':>8} {'alloc':>8} {'crc':>6}  name")
    for e in entries:
        print(f"  {e['index']:>3} {e['type']:#04x} {e['offset']:#8x} {e['size']:#8x} {e['alloc']:#8x} "
              f"{e['crc']:#06x}{' ok ' if e['crc_ok'] else ' BAD'} {e['name']}")
        blob = d[e["offset"]:e["offset"] + e["size"]]
        if e["name"].startswith("flash") and e["name"].endswith(".bin"):
            info, _, _ = parse_flash(blob)
            key = info.get("chip_key")
            print(f"        flash image: version {info['version']!r}, chip key "
                  f"{key:#06x}" if key is not None else "        flash image: chip key NOT FOUND")
            for f in info["files"]:
                ok = {True: " ok", False: " BAD", None: ""}[f.get("data_crc_ok")]
                print(f"        {f['offset']:#8x} {f['size']:#10x} crc {f['data_crc']:#06x}{ok} {f['name']}")
            for f in info["area"]:
                ok = {True: " ok", False: " BAD", None: ""}[f.get("data_crc_ok")]
                print(f"          area +{f['offset']:#9x} {f['size']:#8x} attr {f['attr']:#04x} "
                      f"crc {f['data_crc']:#06x}{ok} {f['name']}")
        elif e["name"] == "ota.bin":
            for f in parse_ota(blob):
                ok = {True: " ok", False: " BAD", None: ""}[f["data_crc_ok"]]
                print(f"        {f['offset']:#8x} {f['size']:#8x} crc {f['data_crc']:#06x}{ok} {f['name']}")


def cmd_extract(path, outdir):
    d = open(path, "rb").read()
    hdr, entries = parse_ufw(d)
    key = ufw_chip_key(d, entries)
    os.makedirs(outdir, exist_ok=True)
    for e in entries:
        blob = d[e["offset"]:e["offset"] + e["size"]]
        if not blob:
            continue
        if not (e["name"].startswith("flash") and e["name"].endswith(".bin")):
            blob = entry_plain(d, e, key)[0]
        open(os.path.join(outdir, safe_name(e["name"])), "wb").write(blob)
        if e["name"].startswith("flash") and e["name"].endswith(".bin"):
            info, dec, blobs = parse_flash(blob)
            sub = os.path.join(outdir, safe_name(e["name"])[:-4])
            os.makedirs(os.path.join(sub, "area"), exist_ok=True)
            open(os.path.join(sub, "flash_decrypted.bin"), "wb").write(dec)
            for name, b in blobs.items():
                folder, _, leaf = name.rpartition("/")
                open(os.path.join(sub, folder, safe_name(leaf)), "wb").write(b)
        elif e["name"] == "ota.bin":
            sub = os.path.join(outdir, "ota")
            os.makedirs(sub, exist_ok=True)
            for f in parse_ota(blob):
                open(os.path.join(sub, safe_name(f["name"])), "wb").write(blob[f["offset"]:f["offset"] + f["size"]])
    print(f"extracted {len(entries)} entries to {outdir}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "list":
        cmd_list(sys.argv[2])
    elif len(sys.argv) == 4 and sys.argv[1] == "extract":
        cmd_extract(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)
