# iDeal LED / Winkison BLE protocol

Covers BLE controllers that advertise as `ISP-*` or `IDL-*` and pair with the Android app
`com.winkison.ledlight` ("LED Light", Winkison). Prior art: [8none1/idealLED](https://github.com/8none1/idealLED)
(Home Assistant integration, MIT) and its [`att_protocol.md`](https://github.com/8none1/idealLED/blob/main/att_protocol.md).

> **Warning.** The idealLED author bricked a set of lights by sending invalid data
> ([write-up](https://www.whizzy.org/2023-12-14-bricked-xmas/)). Only send byte patterns observed
> from the vendor app. Add new commands to the encoder only after capturing them.

## Reference device

| Field | Value |
|---|---|
| Advertised name | `ISP-50542B` |
| Firmware (version characteristic) | `TR2411R003-07` |
| Vendor app | `com.winkison.ledlight` 3.1.7 (versionCode 317) |
| Chip | JieLi (OTA via `ae00` service, `libjl_ota_auth.so`) |

## GATT

| Role | UUID | Handle on reference device |
|---|---|---|
| Control service | `0000fff0-0000-1000-8000-00805f9b34fb` | |
| Write (commands, write without response) | `d44bc439-abfd-45a2-b575-925416129600` | `0x0006` |
| Model string (user description `2901` on `…9600`), e.g. `TR2411R003-07` | | `0x0007` |
| Notify (acks) | `d44bc439-abfd-45a2-b575-925416129601` | `0x000d`, CCCD `0x000e` |
| Bulk write (plaintext frames, see `MULT`) | `d44bc439-abfd-45a2-b575-92541612960a` | `0x0009` |
| Extra write channel | `…960b` | |
| Unknown service / write / write+notify | `0000fd00` / `0000fd01` / `0000fd02` | |
| OTA service / write / notify | `0000ae00` / `0000ae01` / `0000ae02` | `0x0082` / `0x0084`, CCCD `0x0085` |

The vendor app references a `…9602` read characteristic, but `ISP-50542B` does not expose one.

The app also references `0000fee9` as an alternative service UUID for other product lines.

## Encryption

Every command and notification is one 16-byte block, AES-128-ECB, no padding.

- Key: `34522a5b7a6e492c08090a9d8d2a23f8`
- Source: hardcoded in `libAES.so` (JNI class `csh.tiro.cc.aes`, loaded via `keyExpansionDefault()`).
  The same key is published by idealLED.

## Packet layout (plaintext)

`<len> <ASCII tag> <args…> <zero padding to 16 bytes>`, where `len` counts the tag and args
but not itself. Exceptions: `SGLS` uses `0x0f` and fills all 16 bytes, `TIME` uses `0x15`, and
`DSTOP` uses `0x05`, which excludes its argument.

## Confirmed commands

Captured on 2026-10-24 from the reference device; see
[`../captures/2026-10-24-isp-50542b-power-color-brightness.txt`](../captures/2026-10-24-isp-50542b-power-color-brightness.txt)
and [`../captures/2026-10-24-isp-50542b-scenes-diy.txt`](../captures/2026-10-24-isp-50542b-scenes-diy.txt).

| Action | Plaintext | Ciphertext |
|---|---|---|
| Power off | `05 54 55 52 4e 00` + zeros (`TURN 00`) | `79d1dba40919c246a8580ae7d11b7884` |
| Power on | `05 54 55 52 4e 01` + zeros (`TURN 01`) | `84dd5042374150897ac82f39110968a8` |
| Solid red | `0f 'SGLS' 00 00 64 50 ff 00 00 ff 00 00 32` | `73baa3e311ef375b0c2965911e7f5357` |
| Solid green | `0f 'SGLS' 00 00 64 50 00 ff 00 00 ff 00 32` | `a06566adefd31c59d5c85979620ce9ef` |
| Solid blue | `0f 'SGLS' 00 00 64 50 00 00 ff 00 00 ff 32` | `c7f95a08c3583941208cca2d3e6e91c0` |
| Scene, speed 5, brightness 29 | `0a 'MULT' 07 00 64 05 02 1d` | `612074c06b15b5b0616f2569e669062c` |
| Scene, speed 100, brightness 100 | `0a 'MULT' 07 00 64 64 02 64` | `29d3a784888e6dcd9baaccee9809a18c` |
| Scene, reversed | `0a 'MULT' 07 01 64 64 02 64` | `c8d3bced6b94ddee42f40213ea1866ce` |
| DIY pixel 0, `a51515`, brightness 65 | `0d 'DOOD' 01 00 00 02 64 a5 15 15 41` | `89875e93862614aca7f70d38702bff7b` |
| DIY pixel 179, `a50000`, brightness 50 | `0d 'DOOD' 01 00 b3 02 64 a5 00 00 32` | `746a837fe5d1e12408d9e11f34df95a2` |
| DIY fill 180 LEDs, `56a542`, brightness 50 | `0d 'DOOD' 00 00 b4 02 64 56 a5 42 32` | `dcdd7fe7cbf6235242153c019b839bb9` |
| Enter DIY | `04 'DEN' 00` | `cbe9305dbde55cf3f8f13873f0920f2f` |
| Leave DIY | `05 'DSTOP' 01` | `5eb9c3c21f0457240f0c39e0d0953039` |
| Leave DIY | `05 'DSTOP' 00` | `5724de0f03efb16acbe078544cbc4ce1` |

### `SGLS` (color / effect)

Built by `BleProtocol.sendSingleColor` (`com/winkison/smartlights/ble/BleProtocol.java`):

| Byte | Field | Observed |
|---|---|---|
| 0 | length | `0f` |
| 1–4 | tag | `SGLS` |
| 5 | mode (effect index) | `00` = solid |
| 6 | direction (inverted "reverse" flag) | `00` |
| 7 | constant | `64` (100) |
| 8 | speed | `50` |
| 9–11 | color 1 RGB | |
| 12–14 | color 2 RGB (same as color 1 for solid) | |
| 15 | saturation | `32` |

From the app code, not yet captured:

- On the main color screen, effects and speed are carried by `SGLS`; scenes use `MULT` instead.
  This product line never sends `SPEED` or `LIGHT`.
- Mode: 0–9 on the main color screen, each with its own default color pair
  (`LightColorCore.setLightsColorById`).
- Speed: slider 0–100; the app's default is 80 (`0x50`).
- Direction: the app defaults "reverse" to on and sends its inverse, hence `00`.
- Saturation: slider; the app's reset value is 100.
- The color screen sends at most one `SGLS` per 50 ms (`ColorFragment.sendSingleColor`).

### `MULT` (scenes)

Built by `MutilColorAgreement.sendMutilColor`. All three scenes captured (Christmas tree, reef,
fence) sent `07` in byte 5.

| Byte | Field | Observed |
|---|---|---|
| 0 | length | `0a` |
| 1–4 | tag | `MULT` |
| 5 | effect (`modelIndex − 10`) | `07` |
| 6 | direction (inverted "reverse" flag) | `00`, `01` |
| 7 | constant | `64` |
| 8 | speed, 0–100 | `00`–`64` |
| 9 | color count | `02` |
| 10 | brightness, 0–100 (`saturation` in the app) | `05`–`64` |

The colors follow in a second step:

1. Write `MULT` to `0x0006`; the light replies `MULTOK`.
2. Write the colors **unencrypted** to `0x0009` (`…960a`) in frames of
   `<data len + 1> <frame index> <data>`, padded to 98 bytes (96 data bytes per frame once the
   MTU is raised, 18 otherwise). Data is `R G B` per color, each channel scaled by brightness
   and truncated (`ff` at 29% → `49`); no minimum.
3. The light replies `MULTREOK` for each frame except the last, then `MULTCPOK`.

Example, 2 colors `ff00ff` and `1400cb` at brightness 29: `07 00 49 00 49 05 00 3a` + zeros.

### `DOOD` (DIY pixels)

Built by `BleProtocol.graffiti`. Acked with `DOODOK`.

| Byte | Field | Observed |
|---|---|---|
| 0 | length | `0d` |
| 1–4 | tag | `DOOD` |
| 5 | target | `01` one pixel, `00` fill all |
| 6–7 | pixel index, big-endian; LED count when filling | `0000`–`00b3`; fill `00b4` |
| 8 | mode | `02` |
| 9 | speed | `64` |
| 10–12 | RGB | |
| 13 | DIY brightness, 0–100 | `41`, `32` |
| 14–15 | zero | |

The DIY brightness slider sends nothing by itself; the value rides on the next `DOOD`. The app
enters DIY with `DEN 00` and leaves with `DSTOP`; neither is acked.

**Brightness has no command.** The app scales the RGB channels: red at 100% → `ff`, ~50% →
`7f`–`89`, 0% → `02`. A lit channel never drops below `02`.

## Acknowledgements

Each command gets a notification on `0x000d`, decrypted as `<len> <TAG>OK …`. Only the first
`len + 1` bytes are meaningful; the rest is stale buffer data from earlier replies.

| Reply to | Plaintext |
|---|---|
| `TURN` | `06 'TURNOK' 00 06 'SGLSOK' 00` |
| `SGLS` | `06 'SGLSOK' 00 06 'MULTOK' 00` |

## Connection sequence (vendor app)

1. Enable notifications: write `0100` to `0x000e` (control) and `0x0085` (OTA).
2. Read handle `0x0007`, the user description on `…9600` → `TR2411R003-07`.
3. JieLi OTA auth handshake on `ae01`/`ae02` (see [OTA service](#ota-service-ae00)). Not needed
   for control commands: glowmote drove `ISP-50542B` without it on 2026-10-24.
4. On a fresh app start, also on `0x0006`:
   - `15 'TIME' <weekday> <h> <m> <s> a5 5a a5 01 <app version digits>`, e.g.
     `06 08 04 26 … 03 01 07` for Saturday 08:04:38 from app 3.1.7. No ack.
   - `03 'VE' 00` → `V024.11.03.7`, `03 'VE' 01` → `V124.11.66.1`.
   - `0e 'CT' 01 01 00 <LED count u16> <per-strip LED counts u16 ×4>` (`sendCNTSort`, multi-strip
     grouping) → `CONNECTOK`. The reference device reports 180 LEDs (`00b4`).
5. Commands on `0x0006`.

Opening a BLE connection makes the lights blink 3 times, then return to their previous state; the
vendor app and glowmote both trigger it. Subscribing, sending commands and disconnecting cause no
blink (glowmote `probe`, 2026-10-24).

## Commands seen in the app but not yet captured

Do not send these until a capture confirms the exact bytes.

| Tag | Purpose | Builder |
|---|---|---|
| `LIGHT` | brightness, `06 'LIGHT' <n>` (other product lines) | `sendLight` |
| `SPEED` | effect speed, `06 'SPEED' <n>` (other product lines) | `sendSpeed` |
| `TR\0W` | color/mode (other product lines) | `sendColor` |
| `SCHL` / `SCH0` | timers | `sendTimer`, `getTimerLightSwitch` |
| `CORFIRM` / `CHECKLINE` | RGB channel order | `sendCheckLine*` |
| `SHVEW` | enter DIY mode | `Agreement.getEnterDiyCommand` |
| `DOOT`, `DOOTCP` | bulk DIY upload | `BleProtocol.graffiti(List)`, `sendDootCp` |
| `DEN 01` | enter DIY, variant | `enterDiyMode1` |

## OTA service (`ae00`)

The `ae00` service is the stock JieLi SDK's RCSP OTA channel (`apps/spp_and_le` in
[fw-AC63_BT_SDK](https://github.com/Jieli-Tech/fw-AC63_BT_SDK)). Before it accepts update commands,
both sides authenticate with Bluetooth Classic's E1 (SAFER+) function, keyed with the SDK's
default link key and address, which the vendor left unchanged:

- link key `06775f87918dd423005df1d8cf0c142b`, address `112233332211`
- `00 <16-byte challenge>` → `01 <E1(key, challenge, address)>`; `02 'pass'` confirms.

| Step | Direction | Bytes |
|---|---|---|
| 1 | app → light (`ae01`) | `00 67c6697351ff4aec29cdbaabf2fbe346` |
| 2 | light → app (`ae02`) | `01 023a02126fa284879236e9da27ba0b8d` |
| 3 | app → light | `02 'pass'` |
| 4 | light → app | `00 f14f07aeaa049fb1e314ff67c629fecf` |
| 5 | app → light | `01 367094e00eb997eaccfbee8d295e3f4f` |
| 6 | light → app | `02 'pass'` |

Both responses were reproduced with E1 and the key above. Passing the handshake only opens the
OTA channel; the on-chip loader still checks the chip model, CRCs and the image encryption key.

## Firmware

The vendor app bundles OTA images in `assets/ota/*.ufw` for 14 other product lines; none is for
the reference device's `TR2411R003-07`.

| Images | Chip (from the decrypted header) |
|---|---|
| `TR2107R012-4` … `TR2208R009-04` (12 files) | JieLi AC636N |
| `TR2210R004-08`, `TR2309R002-02` | JieLi AC632N |

The images use JieLi's CRC-16 stream cipher: XOR with the low byte of a 16-bit state that
steps as `state = (state << 1) ^ (0x1021 if state & 0x8000 else 0)`. All 14 decrypt fully, and
every file inside passes its stored CRC-16:

| Region | Keystream restarts | Seed |
|---|---|---|
| `.ufw` header (0x40 B) and entry records (0x50 B each) | per record | `0xffff` |
| Small `.ufw` entries (`isd_config.ini`, `script.ver`, …) | every 32 B | `key ^ ((ufw_offset >> 2) & 0xffff)` |
| `ota.bin` (JieLi's stock OTA loader) | not encrypted | |
| Flash header, flash file entries, `uboot.boot` | per record | `0xffff` |
| App area (`app.bin`, `cfg_tool.bin`, …) | every 32 B | `key ^ ((offset_from_area_start >> 2) & 0xffff)` |

`key` is the vendor's 16-bit chip key, `0xdb62` in every image, found by brute force and
confirmed by the `app.bin` CRC. `tools/ufw_tool.py list|extract` implements all of this. `app.bin` loads at `0x01e00120` and is not compressed. JieLi
publishes an Apache-2.0 SDK for both chips:
[Jieli-Tech/fw-AC63_BT_SDK](https://github.com/Jieli-Tech/fw-AC63_BT_SDK).

The images fall into two protocol families. `TR2107`/`TR2110` (`IDL-` names) use a different
command set (`LEDON`, `DATS`, `PLAY`, `MANY`). `TR2202` onward (`ISP-` names) speak this
protocol; the closest to the reference device is `TR2309R002-02` (AC632N, `ISP-`, versions
`V023.9.2.2` / `V122.3.137.8`). Its reply table, in firmware order:

`TURNOK SGLSOK MULTOK SPLAYOK SCHDOK LAMPTOK LAMPNOK DOOTCPOK DOOTOK CONNECTOK DIRECTOK DOODOK
SPLACPOK MULTCPOK SGLSCPOK`, plus `LAMPQ`, `DOOTREOK` and `SPLAREOK` elsewhere in `.data`.
`TR2208` and `TR2210` also have `LAMPNMAX`.

Command tags never appear as strings, so the commands behind the uncaptured replies are
inferred from their names only: `SPLAY` (a third bulk upload with `…REOK`/`…CPOK` phases),
`SCHD` (schedules), `LAMPT`, `LAMPN` (LED count, with an upper limit where `LAMPNMAX` exists),
`LAMPQ` (query), `DIRECT`, and a bulk-completion step for `SGLS` (`SGLSCPOK`). Effect counts and
parameter limits live in code; reading them needs a pi32v2 disassembler such as
[ghidra-jieli](https://github.com/virtualabs/ghidra-jieli).

## Firmware limits (`TR2309R002-02`)

Read from the decrypted `app.bin` of the closest sibling image in Ghidra, with the q32s module
extended until it matched JieLi's own `rom.lst` disassembly. The reference device's firmware is
newer and may differ, so treat these as the safe envelope, not as its exact behavior.
Addresses are load addresses (`app.bin` offset + `0x01e00120`).

The command parser (`0x01e055c8`) decrypts each packet into fixed globals and matches tags byte
by byte. Three paths index arrays with packet bytes **without any bounds check**; the staged
data is saved to flash and reloaded at boot, so a bad value can survive a power cycle:

| Command | Field | Valid | Unchecked write |
|---|---|---|---|
| `SGLS` | byte 5, mode | 0–9 | `slot * 9` into a 10-slot array at RAM `0x4ef0` (`0x01e059c6`, `0x01e059ce`); also `1 << mode` into a bitmask |
| `MULT` | byte 5, effect | 0–11 | `slot * 0x19` into a 12-slot array at RAM `0x592c` (`0x01e05adc`, `0x01e05aee`) |
| `DOOD` (one pixel) | bytes 6–7, index | below 400 | five 400-byte channel buffers from RAM `0x306a` (`0x01e06462`–`0x01e06484`) |

The renderers are guarded: 10 `SGLS` effects (jump table at `0x01e06cf6`) and 12 `MULT` effects
(`0x01e075cc`); out-of-range effects are skipped, and the final pixel output checks the LED count.
`CT` and `LAMPN` only clamp an LED count of 0 to 11; there is no maximum on this image. The
capability record behind `LAMPQ` advertises 480, 160, 100 and 140; 480 looks like the maximum
LED count. Speed, brightness and saturation are stored as-is and bounded only by being bytes.

glowmote sends only `SGLS` mode 0 and `MULT` effect 7, and refuses `DOOD` indexes and fill
counts above 400 (`ideal::MAX_LEDS`).

## Capturing more

1. Phone: Developer options → **Enable Bluetooth HCI snoop log → Enabled** (not Filtered), then
   toggle Bluetooth off and on.
2. `tools/capture.sh check` confirms the phone is on adb and the log mode is `full`.
3. Perform one action at a time in the vendor app, noting the order.
4. `tools/capture.sh pull <name>` collects the bug report, keeps the raw log in the git-ignored
   `research/captures/`, and writes the decoded dump to `docs/captures/<date>-<name>.txt`
   (needs `cryptography`). It lists packets per connection; drop other devices' lines before
   committing.
5. Turn the snoop log off afterwards.

Don't commit raw `btsnoop_hci.log` files: they hold every device on the phone. Commit the
decoded dump instead.

A debug build of the vendor app would log every plaintext command with a label: flip
`com.winkison.baselib.utils.LogUtil.ISLOG` to `true` and re-sign (needs an uninstall, which
wipes the app's saved devices).
