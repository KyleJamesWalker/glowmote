# glowmote

One remote for all your LED lights. A Rust replacement for the pile of vendor apps that
each control one brand of LED strip, targeting Windows, macOS, Linux and Android.

Supported today: iDeal LED / Winkison BLE controllers (`ISP-*`, `IDL-*`): power, solid color
and brightness. See [docs/devices.md](docs/devices.md) for what's planned.

## Usage

Close the vendor app first; these controllers accept one connection at a time.

```sh
cargo run -p glowmote -- scan
cargo run -p glowmote -- on
cargo run -p glowmote -- color ff0000 --brightness 50
cargo run -p glowmote -- --device ISP-50542B off
```

On macOS the first run asks for Bluetooth permission for your terminal.

## Layout

| Path | Contents |
|---|---|
| `crates/glowmote-core` | Protocol encoders, pure bytes, no I/O. Tests use captured packets. |
| `crates/glowmote-cli` | `glowmote` binary: BLE scanning and sending via [btleplug](https://github.com/deviceplug/btleplug). |
| `docs/protocols/` | One file per protocol: GATT layout, encryption, confirmed commands. |
| `docs/captures/` | Decoded vendor-app captures backing the confirmed commands. |
| `tools/parse_snoop.py` | Decodes an Android `btsnoop_hci.log` into ATT writes and notifications. |

## Safety

Some controllers can be bricked by unexpected bytes. The encoders only emit byte patterns
captured from the vendor apps; each protocol doc lists what is confirmed and what is not.
