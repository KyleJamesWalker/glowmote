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
| Notify (acks) | `d44bc439-abfd-45a2-b575-925416129601` | `0x000d`, CCCD `0x000e` |
| Read (firmware version) | `d44bc439-abfd-45a2-b575-925416129602` | `0x0007` |
| Extra write channels | `…960a`, `…960b` | |
| OTA service / write / notify | `0000ae00` / `0000ae01` / `0000ae02` | `0x0082` / `0x0084`, CCCD `0x0085` |

The app also references `0000fee9` as an alternative service UUID for other product lines.

## Encryption

Every command and notification is one 16-byte block, AES-128-ECB, no padding.

- Key: `34522a5b7a6e492c08090a9d8d2a23f8`
- Source: hardcoded in `libAES.so` (JNI class `csh.tiro.cc.aes`, loaded via `keyExpansionDefault()`).
  The same key is published by idealLED.

## Packet layout (plaintext)

`<len> <ASCII tag> <args…> <zero padding to 16 bytes>`, where `len` counts the tag and args
but not itself. Exception: `SGLS` uses `0x0f` and fills all 16 bytes.

## Confirmed commands

Captured on 2026-10-24 from the reference device; see
[`../captures/2026-10-24-isp-50542b-power-color-brightness.txt`](../captures/2026-10-24-isp-50542b-power-color-brightness.txt).

| Action | Plaintext | Ciphertext |
|---|---|---|
| Power off | `05 54 55 52 4e 00` + zeros (`TURN 00`) | `79d1dba40919c246a8580ae7d11b7884` |
| Power on | `05 54 55 52 4e 01` + zeros (`TURN 01`) | `84dd5042374150897ac82f39110968a8` |
| Solid red | `0f 'SGLS' 00 00 64 50 ff 00 00 ff 00 00 32` | `73baa3e311ef375b0c2965911e7f5357` |
| Solid green | `0f 'SGLS' 00 00 64 50 00 ff 00 00 ff 00 32` | `a06566adefd31c59d5c85979620ce9ef` |
| Solid blue | `0f 'SGLS' 00 00 64 50 00 00 ff 00 00 ff 32` | `c7f95a08c3583941208cca2d3e6e91c0` |

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
2. Read the version characteristic → `TR2411R003-07`.
3. JieLi OTA auth handshake on `ae01`/`ae02` (random challenge, then response). Not needed for
   control commands: glowmote drove `ISP-50542B` without it on 2026-10-24.
4. Commands on `0x0006`.

Opening a BLE connection makes the lights blink 3 times, then return to their previous state; the
vendor app and glowmote both trigger it. Subscribing, sending commands and disconnecting cause no
blink (glowmote `probe`, 2026-10-24).

## Commands seen in the app but not yet captured

Do not send these until a capture confirms the exact bytes.

| Tag | Purpose | Builder |
|---|---|---|
| `LIGHT` | brightness (other product lines) | `sendLight` |
| `SPEED` | effect speed | `sendSpeed` |
| `TR\0W` | color/mode (other product lines) | `sendColor` |
| `TIME` | set clock | `getTimeCommand` |
| `SCHL` / `SCH0` | timers | `sendTimer`, `getTimerLightSwitch` |
| `CORFIRM` / `CHECKLINE` | RGB channel order | `sendCheckLine*` |
| `SHVEW` | enter DIY mode | `Agreement.getEnterDiyCommand` |
| `DOOTCP`, `STOP` | graffiti mode | `sendDootCp`, `outMode` |
| `MULT` | multi-color | `sendMutilColor` |

## Capturing more

1. Phone: Developer options → **Enable Bluetooth HCI snoop log → Enabled** (not Filtered), then
   toggle Bluetooth off and on.
2. Perform one action at a time in the vendor app, noting the order.
3. `adb bugreport bug.zip`, then
   `unzip -j bug.zip FS/data/misc/bluetooth/logs/btsnoop_hci.log`.
4. `python3 -I tools/parse_snoop.py btsnoop_hci.log` (needs `cryptography`).
5. Turn the snoop log off afterwards.

Don't commit raw `btsnoop_hci.log` files: they hold every device on the phone. Commit the
decoded dump instead.

A debug build of the vendor app would log every plaintext command with a label: flip
`com.winkison.baselib.utils.LogUtil.ISLOG` to `true` and re-sign (needs an uninstall, which
wipes the app's saved devices).
