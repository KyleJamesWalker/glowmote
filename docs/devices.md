# Devices and vendor apps to replace

| Vendor app (Android package) | Link | Status | Reference implementations |
|---|---|---|---|
| LED Light (`com.winkison.ledlight`) | BLE, AES | Power, color, brightness, scenes (`MULT`), DIY pixels (`DOOD`) captured; power, color, brightness in glowmote; [protocol](protocols/ideal-led.md) | [8none1/idealLED](https://github.com/8none1/idealLED) |
| iPixel Color (`com.wifiled.ipixels`) | BLE (LED matrix) | Not started | [lucagoc/pypixelcolor](https://github.com/lucagoc/pypixelcolor), [go-ipxl](https://yewolf.fr/projects/go-ipxl) |
| Magic Home (`com.zennge.magichome2`) | Wi-Fi (TCP 5577) or BLE | Not started | [lightinglibs/flux_led](https://github.com/lightinglibs/flux_led), [Bluetooth-Devices/led-ble](https://github.com/Bluetooth-Devices/led-ble) |
| Tuya / Smart Life (`com.tuya.smart`, `com.tuya.smartlife`) | Usually Wi-Fi, needs per-device local key | Not started | [jasonacox/tinytuya](https://github.com/jasonacox/tinytuya) |

Reference implementations are protocol specs to port, not dependencies.
