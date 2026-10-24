# glowmote

- Only encode commands whose bytes were captured from the vendor app. Unconfirmed commands stay
  listed under "not yet captured" in `docs/protocols/<protocol>.md` until a capture lands.
- Every confirmed command gets a test that asserts the captured ciphertext.
- Record new findings (UUIDs, firmware versions, command bytes) in `docs/protocols/` as they are
  discovered, and the device inventory in `docs/devices.md`.
- Never commit raw `btsnoop_hci.log` or bug reports; commit the decoded output of
  `tools/parse_snoop.py` under `docs/captures/`.
- Reference device for iDeal: `ISP-50542B` on a Pixel 9 Pro XL over adb. Decompile vendor APKs
  with `jadx` into a scratch directory, never into the repo.
