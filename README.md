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

## HTTP proxy

`glowmote-proxy` holds one connection and shares it over HTTP: open the desktop app's UI in any
browser, or script the lights with `curl`.

```sh
cargo run -p glowmote-proxy -- --device ISP-50542B   # then browse to http://127.0.0.1:8484
curl -H 'content-type: application/json' -d '{"rgb":"ff0000","brightness":50}' http://127.0.0.1:8484/api/color
```

`--device` connects on start to the light whose name starts with it; without it, scan and
connect from the UI or the API. `--scan-secs` sets the scan time (default 5).

**`--listen 0.0.0.0:8484` lets anyone on your network control the lights. There is no
authentication.** The default, `127.0.0.1:8484`, accepts this machine only.

Every `POST` takes a JSON body (`{}` when there are no arguments) and needs
`Content-Type: application/json`. Responses are `{"ok": value}`, or `{"error": "..."}` with a 4xx
or 5xx status: 400 bad argument, 404 unknown light or command, 409 not connected, 502 the light
failed or did not acknowledge.

| Endpoint | Body | `ok` |
|---|---|---|
| `GET /api/status` | | connected light's name, or `null` |
| `POST /api/scan_lights` | `{}` | light names |
| `POST /api/connect` | `{"name"}` | `null` |
| `POST /api/disconnect` | `{}` | `null` |
| `POST /api/info` | `{}` | `{"name", "model", "firmware": [v0, v1]}` |
| `POST /api/power` | `{"on"}` | acknowledged tag |
| `POST /api/color` | `{"rgb", "brightness"}` | acknowledged tag |
| `POST /api/scene` | `{"first", "second", "speed", "brightness", "reverse"}` | `null` |
| `POST /api/diy` | `{"on"}` | `null` |
| `POST /api/diy_pixel` | `{"index", "leds", "rgb", "brightness"}` | acknowledged tag |
| `POST /api/diy_fill` | `{"leds", "rgb", "brightness"}` | acknowledged tag |

Colors are `RRGGBB` hex, with or without `#`; brightness and speed are 0-100. Commands from all
clients run one at a time in arrival order, so send DIY pixels sequentially to keep their order.


## Android

The desktop app also builds for Android. Bluetooth does not work there yet: btleplug needs an
Android backend.

```sh
source tools/android-env.sh   # Android SDK and NDK in ~/Library/Android/sdk, JDK 17
cd crates/glowmote-app && cargo tauri android build --debug --apk --target aarch64
```

## Layout

| Path | Contents |
|---|---|
| `crates/glowmote-core` | Protocol encoders, pure bytes, no I/O. Tests use captured packets. |
| `crates/glowmote-ble` | BLE transport via [btleplug](https://github.com/deviceplug/btleplug): scanning and a connection that stays open across commands. |
| `crates/glowmote-control` | `Controller`: the shared light connection and the command set the app and proxy expose. |
| `crates/glowmote-app` | Tauri 2 desktop app; the UI is plain HTML/JS in `ui/`, no npm. |
| `crates/glowmote-proxy` | `glowmote-proxy` binary: the commands over HTTP, plus the app's UI for browsers. |
| `crates/glowmote-cli` | `glowmote` binary: one-shot commands, `shell` and `probe`. |
| `docs/protocols/` | One file per protocol: GATT layout, encryption, confirmed commands. |
| `docs/captures/` | Decoded vendor-app captures backing the confirmed commands. |
| `tools/capture.sh` | Pulls the phone's snoop log over adb and decodes it into `docs/captures/`. |
| `tools/parse_snoop.py` | Decodes an Android `btsnoop_hci.log` into ATT writes and notifications. |
| `tools/ufw_tool.py` | Lists and decrypts JieLi `.ufw` firmware update images. |

## Safety

Some controllers can be bricked by unexpected bytes. The encoders only emit byte patterns
captured from the vendor apps; each protocol doc lists what is confirmed and what is not.
