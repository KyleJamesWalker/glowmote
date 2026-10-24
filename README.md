# glowmote

One remote for all your LED lights. A Rust replacement for the pile of vendor apps that
each control one brand of LED strip, targeting Windows, macOS, Linux and Android.

Supported today: iDeal LED / Winkison BLE controllers (`ISP-*`, `IDL-*`): power, solid color,
brightness, the two-color scene effect, and DIY per-LED color and fill. See [docs/devices.md](docs/devices.md) for what's planned.

## Usage

Close the vendor app first; these controllers accept one connection at a time.

```sh
cargo run -p glowmote -- scan
cargo run -p glowmote -- on
cargo run -p glowmote -- color ff0000 --brightness 50
cargo run -p glowmote -- --device ISP-50542B off
```

Each command above opens its own connection, and the lights blink 3 times on every connection.
`shell` connects once and reads commands from stdin, one per line, typed or piped:

```sh
cargo run -p glowmote -- shell
printf 'on\ncolor 00ff00\nquit\n' | cargo run -p glowmote -- shell
```

DIY commands mirror the vendor app's DIY page: enter DIY mode, then paint.

```sh
printf 'diy\nfill 00ff00\npixel 0 ff0000\npixel 179 ff0000 -b 80\nquit\n' | cargo run -p glowmote -- shell
```

`scene ff3b30 34c759 --speed 60` plays the two-color effect. `pixel` and `fill` default to 50% brightness and a 180-LED strip (`--leds`).

On macOS the first run asks for Bluetooth permission for your terminal.

## Desktop app

A [Tauri 2](https://v2.tauri.app/) app with scan, connect, power, color, brightness, scenes, DIY painting and white balance. It holds
one connection while open.

```sh
cargo run -p glowmote-app
```

For a bundled app, install the CLI (`cargo install tauri-cli --version "^2" --locked`) and run
`cargo tauri build` in `crates/glowmote-app`.

## Layout

| Path | Contents |
|---|---|
| `crates/glowmote-core` | Protocol encoders, pure bytes, no I/O. Tests use captured packets. |
| `crates/glowmote-ble` | BLE transport via [btleplug](https://github.com/deviceplug/btleplug): scanning and a connection that stays open across commands. |
| `crates/glowmote-app` | Tauri 2 desktop app; the UI is plain HTML/JS in `ui/`, no npm. |
| `crates/glowmote-cli` | `glowmote` binary: one-shot commands, `shell` and `probe`. |
| `docs/protocols/` | One file per protocol: GATT layout, encryption, confirmed commands. |
| `docs/captures/` | Decoded vendor-app captures backing the confirmed commands. |
| `tools/parse_snoop.py` | Decodes an Android `btsnoop_hci.log` into ATT writes and notifications. |

## Safety

Some controllers can be bricked by unexpected bytes. The encoders only emit byte patterns
captured from the vendor apps; each protocol doc lists what is confirmed and what is not.
