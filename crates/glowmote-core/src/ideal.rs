//! iDeal LED / Winkison (`ISP-*`, `IDL-*`) BLE protocol. See `docs/protocols/ideal-led.md`.
//!
//! Only byte patterns observed from the vendor app are emitted: these controllers can be
//! bricked by unexpected values.

use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};

use crate::Rgb;

pub const SERVICE_UUID: &str = "0000fff0-0000-1000-8000-00805f9b34fb";
pub const WRITE_CHAR_UUID: &str = "d44bc439-abfd-45a2-b575-925416129600";
pub const NOTIFY_CHAR_UUID: &str = "d44bc439-abfd-45a2-b575-925416129601";
/// Takes plaintext data frames, such as scene colors.
pub const BULK_WRITE_CHAR_UUID: &str = "d44bc439-abfd-45a2-b575-92541612960a";
/// Takes encrypted music rhythm levels; see [`rhythm`].
pub const RHYTHM_CHAR_UUID: &str = "d44bc439-abfd-45a2-b575-92541612960b";

pub const NAME_PREFIXES: [&str; 2] = ["ISP-", "IDL-"];

const KEY: [u8; 16] = [
    0x34, 0x52, 0x2a, 0x5b, 0x7a, 0x6e, 0x49, 0x2c, 0x08, 0x09, 0x0a, 0x9d, 0x8d, 0x2a, 0x23, 0xf8,
];

/// The vendor app never sends a lit channel below this, even with brightness at 0%.
const MIN_LIT_CHANNEL: u8 = 2;

pub type Packet = [u8; 16];

/// Size of the firmware's per-channel pixel buffers. `DOOD` indexes them without a bounds check,
/// so a larger index or LED count corrupts the controller's RAM.
pub const MAX_LEDS: u16 = 400;

#[derive(Debug, thiserror::Error)]
#[error("{0} is beyond the firmware's {MAX_LEDS}-LED buffer")]
pub struct LedOutOfRange(pub u16);

/// A field outside the range the firmware handles safely.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{field} {value} is outside {min}..={max}")]
pub struct OutOfRange {
    pub field: &'static str,
    pub value: u16,
    pub min: u16,
    pub max: u16,
}

fn check<T: Into<u16> + Copy>(
    field: &'static str,
    value: T,
    min: u16,
    max: u16,
) -> Result<T, OutOfRange> {
    let v = value.into();
    if (min..=max).contains(&v) {
        Ok(value)
    } else {
        Err(OutOfRange {
            field,
            value: v,
            min,
            max,
        })
    }
}

pub fn is_ideal_name(name: &str) -> bool {
    NAME_PREFIXES.iter().any(|p| name.starts_with(p))
}

pub fn encrypt(plain: &Packet) -> Packet {
    let mut block = GenericArray::from(*plain);
    Aes128::new(&KEY.into()).encrypt_block(&mut block);
    block.into()
}

pub fn decrypt(cipher: &Packet) -> Packet {
    let mut block = GenericArray::from(*cipher);
    Aes128::new(&KEY.into()).decrypt_block(&mut block);
    block.into()
}

fn tagged(tag: &[u8], args: &[u8]) -> Packet {
    let mut p = [0u8; 16];
    p[0] = (tag.len() + args.len()) as u8;
    p[1..1 + tag.len()].copy_from_slice(tag);
    p[1 + tag.len()..1 + tag.len() + args.len()].copy_from_slice(args);
    p
}

pub fn power(on: bool) -> Packet {
    tagged(b"TURN", &[on as u8])
}

/// Solid color. Brightness is applied by scaling the channels, as the vendor app does.
pub fn solid_color(color: Rgb, brightness_pct: u8) -> Packet {
    let c = scale(color, brightness_pct.min(100));
    sgls(0, false, 0x50, [c, c])
}

/// Number of `SGLS` effects. The firmware indexes a 10-slot array with the mode unchecked.
pub const EFFECT_COUNT: u8 = 10;

/// An `SGLS` effect (mode 0 is solid): `colors` are foreground and background. `reversed` sets
/// byte 6, which the app leaves at `00` by default. Brightness scales the channels.
pub fn effect(
    mode: u8,
    reversed: bool,
    speed_pct: u8,
    colors: [Rgb; 2],
    brightness_pct: u8,
) -> Result<Packet, OutOfRange> {
    check("effect mode", mode, 0, EFFECT_COUNT as u16 - 1)?;
    check("speed", speed_pct, 0, 100)?;
    check("brightness", brightness_pct, 0, 100)?;
    Ok(sgls(
        mode,
        reversed,
        speed_pct,
        colors.map(|c| scale(c, brightness_pct)),
    ))
}

fn sgls(mode: u8, reversed: bool, speed: u8, [c1, c2]: [Rgb; 2]) -> Packet {
    // fixed 100 in byte 7 and saturation 0x32, as captured
    tagged(
        b"SGLS",
        &[
            mode,
            reversed as u8,
            0x64,
            speed,
            c1.r,
            c1.g,
            c1.b,
            c2.r,
            c2.g,
            c2.b,
            0x32,
        ],
    )
}

#[cfg(test)]
mod effect_tests {
    use super::*;
    use hex_literal::hex;

    const RED: Rgb = Rgb::new(0xff, 0, 0);
    const GREEN: Rgb = Rgb::new(0, 0xff, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 0xff);

    // Captured 2026-10-24 from the vendor app's scene pages.
    #[test]
    fn effects_match_capture() {
        assert_eq!(
            encrypt(&effect(1, false, 0x50, [GREEN, BLUE], 50).unwrap()),
            hex!("abc14c153ede82e54b7e050132c329d8")
        );
        assert_eq!(
            encrypt(&effect(0, true, 0x50, [RED, RED], 37).unwrap()),
            hex!("d31cf01de4024b9952ec894d32ae4694")
        );
    }

    #[test]
    fn solid_effect_matches_solid_color_capture() {
        assert_eq!(
            encrypt(&effect(0, false, 0x50, [RED, RED], 100).unwrap()),
            hex!("73baa3e311ef375b0c2965911e7f5357")
        );
    }

    #[test]
    fn effect_rejects_out_of_range_fields() {
        assert!(effect(EFFECT_COUNT - 1, false, 100, [RED, BLUE], 100).is_ok());
        let err = effect(EFFECT_COUNT, false, 50, [RED, BLUE], 100).unwrap_err();
        assert_eq!(err.field, "effect mode");
        assert!(effect(0, false, 101, [RED, BLUE], 100).is_err());
        assert!(effect(0, false, 50, [RED, BLUE], 101).is_err());
    }
}

/// Enters the DIY pixel editor, as the app does when its DIY page opens.
pub fn diy_enter() -> Packet {
    tagged(b"DEN", &[0])
}

/// Leaves DIY mode: `keep_drawing` leaves the drawing showing (`DSTOP 01`), otherwise the lights
/// return to their previous mode and the drawing is discarded (`DSTOP 00`).
pub fn diy_exit(keep_drawing: bool) -> Packet {
    let mut p = tagged(b"DSTOP", &[keep_drawing as u8]);
    p[0] = 5; // the length byte excludes the argument
    p
}

/// Sets one LED (0-based) in DIY mode. Brightness is sent as-is, not by scaling the channels.
pub fn diy_pixel(index: u16, color: Rgb, brightness_pct: u8) -> Result<Packet, LedOutOfRange> {
    if index >= MAX_LEDS {
        return Err(LedOutOfRange(index));
    }
    Ok(dood(1, index, color, brightness_pct))
}

/// Sets every LED in DIY mode; the app sends the strip's LED count in place of a pixel index.
pub fn diy_fill(led_count: u16, color: Rgb, brightness_pct: u8) -> Result<Packet, LedOutOfRange> {
    if led_count > MAX_LEDS {
        return Err(LedOutOfRange(led_count));
    }
    Ok(dood(0, led_count, color, brightness_pct))
}

fn dood(target: u8, index: u16, Rgb { r, g, b }: Rgb, brightness_pct: u8) -> Packet {
    let [hi, lo] = index.to_be_bytes();
    // mode 2 and speed 100 are the only values captured
    let mut p = tagged(
        b"DOOD",
        &[
            target,
            hi,
            lo,
            0x02,
            0x64,
            r,
            g,
            b,
            brightness_pct.min(100),
            0,
            0,
        ],
    );
    p[0] = 0x0d; // the length byte excludes the two trailing zeros
    p
}

/// DIY pixel animation, as stored per LED by the firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiyMode {
    Flash = 0,
    Breathe = 1,
    Static = 2,
}

/// `count` consecutive LEDs with the same color, mode and speed (1–100, 100 fastest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiyRun {
    pub count: u16,
    pub color: Rgb,
    pub mode: DiyMode,
    pub speed: u8,
}

/// Collapses a per-LED color list into the fewest runs.
pub fn diy_runs(pixels: &[Rgb], mode: DiyMode, speed: u8) -> Vec<DiyRun> {
    let mut runs: Vec<DiyRun> = Vec::new();
    for &color in pixels {
        match runs.last_mut() {
            Some(run) if run.color == color && run.count < u16::MAX => run.count += 1,
            _ => runs.push(DiyRun {
                count: 1,
                color,
                mode,
                speed,
            }),
        }
    }
    runs
}

pub const DIY_FRAME_LEN: usize = 100;
const DIY_RECORD_LEN: usize = 7;
const DIY_RECORDS_PER_FRAME: usize = 14;

/// A bulk DIY drawing: send `header` and wait for `DOOTOK`, write each frame unencrypted to
/// [`BULK_WRITE_CHAR_UUID`] and wait for its `DOOTREOK` (see [`parse_frame_ack`]), then send
/// [`diy_upload_done`] (acked with `DOOTCPOK`) and [`diy_upload_finish`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiyUpload {
    pub header: Packet,
    pub frames: Vec<[u8; DIY_FRAME_LEN]>,
}

/// Builds a bulk DIY upload (`DOOT`). The firmware writes the runs back to back from LED 0 with
/// no bounds check, so their total must fit `led_count`, itself at most [`MAX_LEDS`].
pub fn diy_upload(runs: &[DiyRun], led_count: u16) -> Result<DiyUpload, OutOfRange> {
    check("LED count", led_count, 1, MAX_LEDS)?;
    let mut total: u16 = 0;
    for run in runs {
        check("run length", run.count, 1, led_count)?;
        check("DIY speed", run.speed, 1, 100)?;
        total = total.saturating_add(run.count);
    }
    check("LEDs drawn", total, 1, led_count)?;

    let frames: Vec<_> = runs
        .chunks(DIY_RECORDS_PER_FRAME)
        .enumerate()
        .map(|(i, chunk)| {
            let mut frame = [0u8; DIY_FRAME_LEN];
            frame[0] = (chunk.len() * DIY_RECORD_LEN + 1) as u8; // data length + 1
            frame[1] = i as u8;
            for (j, run) in chunk.iter().enumerate() {
                let [hi, lo] = run.count.to_be_bytes();
                let Rgb { r, g, b } = run.color;
                let at = 2 + j * DIY_RECORD_LEN;
                frame[at..at + DIY_RECORD_LEN].copy_from_slice(&[
                    hi,
                    lo,
                    r,
                    g,
                    b,
                    run.mode as u8,
                    run.speed,
                ]);
            }
            frame
        })
        .collect();
    let mut header = tagged(b"DOOT", &[frames.len() as u8]);
    header[0] = 4; // the length byte excludes the frame count
    Ok(DiyUpload { header, frames })
}

/// Closes a [`diy_upload`] (`DOOTCP`). The firmware keeps the drawing only if the last frame
/// index it received matches the header's frame count.
pub fn diy_upload_done() -> Packet {
    tagged(b"DOOTCP", &[])
}

/// Sent after `DOOTCPOK`: `show` keeps the uploaded drawing, otherwise the lights return to their
/// previous mode. Not acknowledged.
pub fn diy_upload_finish(show: bool) -> Packet {
    tagged(b"DTARTCY", &[show as u8])
}

/// Parses a per-frame acknowledgement such as `DOOTREOK`, which carries the frame index after
/// `OK`: returns the tag (`DOOTRE`) and the index.
pub fn parse_frame_ack(plain: &Packet) -> Option<(&str, u8)> {
    let len = plain[0] as usize;
    let index = *plain.get(len)?;
    let text = std::str::from_utf8(plain.get(1..len)?).ok()?;
    let tag = text.strip_suffix("OK").filter(|t| t.ends_with("RE"))?;
    Some((tag, index))
}

#[cfg(test)]
mod diy_upload_tests {
    use super::*;
    use hex_literal::hex;

    const RED: Rgb = Rgb::new(0xff, 0, 0);
    const GREEN: Rgb = Rgb::new(0, 0xff, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 0xff);

    fn runs(n: usize) -> Vec<DiyRun> {
        let colors = [RED, GREEN];
        let pixels: Vec<_> = (0..n).map(|i| colors[i % 2]).collect();
        diy_runs(&pixels, DiyMode::Static, 100)
    }

    // Captured 2026-10-24: the vendor app applying a saved DIY favorite on 180 LEDs.
    const FAVORITE: [[u8; DIY_FRAME_LEN]; 3] = [
        hex!(
            "63000005001d800264000180002802640005001d800264000280002802640002001d800264000480002802640002001d800264000180002802640001001d800264000169008002640003001d800264000180002802640005001d80026400018000280264"
        ),
        hex!(
            "6301000169008002640001001d800264000280002802640001001d800264000280002802640003001d800264000169008002640002001d800264000169008002640005001d800264000180002802640001001d800264000269008002640002001d800264"
        ),
        hex!(
            "6302000180002802640001001d80026400018000280264000b001d800264000180002802640005001d800264000169008002640003001d800264000180002802640001001d800264000480002802640009001d800264000169008002640051001d800264"
        ),
    ];

    #[test]
    fn diy_upload_matches_capture() {
        let runs: Vec<_> = FAVORITE
            .iter()
            .flat_map(|f| f[2..].chunks(DIY_RECORD_LEN))
            .map(|r| {
                assert_eq!(r[5], DiyMode::Static as u8);
                DiyRun {
                    count: u16::from_be_bytes([r[0], r[1]]),
                    color: Rgb::new(r[2], r[3], r[4]),
                    mode: DiyMode::Static,
                    speed: r[6],
                }
            })
            .collect();
        let upload = diy_upload(&runs, 180).unwrap();
        assert_eq!(
            encrypt(&upload.header),
            hex!("4d125879450737e4b6fa4e85062cfac1")
        );
        assert_eq!(upload.frames, FAVORITE);
        assert_eq!(
            encrypt(&diy_upload_done()),
            hex!("49ca6210248aac8b1c1fc7fc55dd3f94")
        );
        assert_eq!(
            encrypt(&diy_upload_finish(true)),
            hex!("e761c13e901167bd39bf66db76b9eb6f")
        );
    }

    #[test]
    fn diy_frames_hold_14_records() {
        let upload = diy_upload(&runs(15), 180).unwrap();
        assert_eq!(upload.frames.len(), 2);
        let [first, last] = [&upload.frames[0], &upload.frames[1]];
        assert_eq!(&first[..16], &hex!("6300 0001ff0000 0264 0001 00ff00 0264"));
        assert_eq!(&first[93..], &hex!("000100ff000264"));
        assert_eq!(&last[..9], &hex!("0801 0001ff0000 0264"));
        assert!(last[9..].iter().all(|&b| b == 0));
    }

    #[test]
    fn diy_runs_merge_neighbors() {
        let pixels = [RED, RED, RED, BLUE, RED, RED];
        let runs = diy_runs(&pixels, DiyMode::Breathe, 40);
        let counts: Vec<_> = runs.iter().map(|r| (r.count, r.color)).collect();
        assert_eq!(counts, [(3, RED), (1, BLUE), (2, RED)]);
        assert!(
            runs.iter()
                .all(|r| r.mode == DiyMode::Breathe && r.speed == 40)
        );
        let frame = diy_upload(&runs, 180).unwrap().frames[0];
        assert_eq!(&frame[..9], &hex!("1600 0003ff0000 0128"));
        assert!(diy_runs(&[], DiyMode::Static, 100).is_empty());
    }

    #[test]
    fn diy_upload_keeps_runs_inside_the_strip() {
        let all = diy_runs(&[RED; 180], DiyMode::Static, 100);
        assert!(diy_upload(&all, 180).is_ok());
        assert_eq!(diy_upload(&all, 179).unwrap_err().field, "run length");
        let split = [all[0], all[0]];
        assert_eq!(diy_upload(&split, 300).unwrap_err().field, "LEDs drawn");
        let full = diy_runs(&[RED; MAX_LEDS as usize], DiyMode::Static, 100);
        assert!(diy_upload(&full, MAX_LEDS).is_ok());
        assert!(diy_upload(&full, MAX_LEDS + 1).is_err());
        assert!(diy_upload(&[], 180).is_err());
        assert!(diy_upload(&runs(400), MAX_LEDS).is_ok());
        let zero = DiyRun { count: 0, ..all[0] };
        assert!(diy_upload(&[zero, all[0]], 180).is_err());
    }

    #[test]
    fn diy_upload_rejects_speeds_outside_1_to_100() {
        for (speed, ok) in [(0, false), (1, true), (100, true), (101, false)] {
            let runs = diy_runs(&[RED; 10], DiyMode::Flash, speed);
            assert_eq!(diy_upload(&runs, 180).is_ok(), ok, "{speed}");
        }
    }

    #[test]
    fn parses_diy_frame_acks() {
        let reok = decrypt(&hex!("3f6220826483b8ca6e5cc87e657c7ef2"));
        assert_eq!(parse_frame_ack(&reok), Some(("DOOTRE", 0)));
        assert_eq!(parse_ack(&reok), None);
        let cp = decrypt(&hex!("3af00d94d213c084bc9df70a59db4b4e"));
        assert_eq!(parse_ack(&cp), Some("DOOTCP"));
        assert_eq!(parse_frame_ack(&cp), None);
        let ok = decrypt(&hex!("886b02308c82c98f6d267a5d917c6685"));
        assert_eq!(parse_ack(&ok), Some("DOOT"));
    }
}

/// Every scene captured used this effect byte.
const SCENE_EFFECT: u8 = 0x07;
const SCENE_FRAME_LEN: usize = 98;

/// Scene header (`MULT`) for a two-color effect. Send it, wait for `MULTOK`, then write
/// [`scene_frame`] unencrypted to [`BULK_WRITE_CHAR_UUID`]; the light finishes with `MULTCPOK`.
pub fn scene(reversed: bool, speed_pct: u8, brightness_pct: u8) -> Packet {
    mult(
        SCENE_EFFECT,
        reversed,
        speed_pct.min(100),
        2,
        brightness_pct.min(100),
    )
}

/// The color frame that follows [`scene`]: each channel scaled by brightness and truncated.
pub fn scene_frame(colors: [Rgb; 2], brightness_pct: u8) -> [u8; SCENE_FRAME_LEN] {
    mult_frame(&colors, brightness_pct.min(100))
}

/// Number of `MULT` effects. The firmware indexes a 12-slot array with the effect unchecked.
pub const SCENE_EFFECT_COUNT: u8 = 12;
/// More colors overflow the firmware's color slots; zero divides by zero.
pub const MAX_SCENE_COLORS: usize = 7;

/// A `MULT` scene with any effect and 1 to [`MAX_SCENE_COLORS`] colors: the header and the
/// color frame, sent as for [`scene`].
pub fn multi_color(
    effect: u8,
    reversed: bool,
    speed_pct: u8,
    colors: &[Rgb],
    brightness_pct: u8,
) -> Result<(Packet, [u8; SCENE_FRAME_LEN]), OutOfRange> {
    check("scene effect", effect, 0, SCENE_EFFECT_COUNT as u16 - 1)?;
    check("speed", speed_pct, 0, 100)?;
    check("brightness", brightness_pct, 0, 100)?;
    let count = colors.len().min(u8::MAX.into()) as u8;
    check("color count", count, 1, MAX_SCENE_COLORS as u16)?;
    Ok((
        mult(effect, reversed, speed_pct, count, brightness_pct),
        mult_frame(colors, brightness_pct),
    ))
}

fn mult(effect: u8, reversed: bool, speed: u8, count: u8, brightness: u8) -> Packet {
    tagged(
        b"MULT",
        &[effect, reversed as u8, 0x64, speed, count, brightness],
    )
}

fn mult_frame(colors: &[Rgb], pct: u8) -> [u8; SCENE_FRAME_LEN] {
    let ch = |v: u8| (v as u16 * pct as u16 / 100) as u8;
    let mut frame = [0u8; SCENE_FRAME_LEN];
    frame[0] = (colors.len() * 3 + 1) as u8; // data length + 1
    for (i, &Rgb { r, g, b }) in colors.iter().enumerate() {
        frame[2 + i * 3..5 + i * 3].copy_from_slice(&[ch(r), ch(g), ch(b)]);
    }
    frame
}

#[cfg(test)]
mod multi_color_tests {
    use super::*;
    use hex_literal::hex;

    const RED: Rgb = Rgb::new(0xff, 0, 0);
    const GREEN: Rgb = Rgb::new(0, 0xff, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 0xff);
    const WHITE: Rgb = Rgb::new(0xff, 0xff, 0xff);
    const SEVEN: [Rgb; 7] = [RED, GREEN, BLUE, WHITE, RED, GREEN, BLUE];

    /// Doubles each channel of a captured 50% frame, so scaling by 50 reproduces it.
    fn colors_at_half(frame: &[u8]) -> Vec<Rgb> {
        let data = &frame[2..2 + frame[0] as usize - 1];
        data.chunks(3)
            .map(|c| Rgb::new(c[0] * 2, c[1] * 2, c[2] * 2))
            .collect()
    }

    // Captured 2026-10-24: the rainbow scene's 7 colors at brightness 50 under each effect.
    #[test]
    fn multi_color_matches_capture() {
        let rainbow = hex!("16007f00007f51007f7f00007f0000007f7f007f7f7f7f00");
        let blinking = hex!("16007f00007f00007f7f00007f0000007f7f007f7f7f7f00");
        for (effect, speed, frame, header) in [
            (
                0,
                0x50,
                &rainbow[..],
                hex!("78a285cda85bf710f8bcbb1eae183bd8"),
            ),
            (
                1,
                0x09,
                &blinking[..],
                hex!("7a0e16c5439b21ad54cf9a2e6745f1aa"),
            ),
            (
                8,
                0x50,
                &rainbow[..],
                hex!("173a9afca7ae6d41a8c794c4fdbb5c08"),
            ),
            (
                9,
                0x50,
                &rainbow[..],
                hex!("6912a67bf156ae6fbc63c7b36e5bfbcd"),
            ),
        ] {
            let (h, f) = multi_color(effect, false, speed, &colors_at_half(frame), 50).unwrap();
            assert_eq!(encrypt(&h), header, "effect {effect}");
            assert_eq!(&f[..frame.len()], frame, "effect {effect}");
            assert!(f[frame.len()..].iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn multi_color_frames_carry_every_color() {
        let (_, frame) = multi_color(0, false, 50, &[RED, GREEN, BLUE], 100).unwrap();
        assert_eq!(&frame[..11], &hex!("0a00ff000000ff000000ff"));
        assert!(frame[11..].iter().all(|&b| b == 0));
        let (_, frame) = multi_color(11, true, 100, &SEVEN, 50).unwrap();
        assert_eq!(frame[0], 22);
        assert_eq!(&frame[11..14], &hex!("7f7f7f"));
        assert_eq!(&frame[21..23], &hex!("007f"));
        assert!(frame[23..].iter().all(|&b| b == 0));
    }

    #[test]
    fn two_color_effect_7_matches_scene_capture() {
        let colors = [Rgb::new(0xff, 0, 0xff), Rgb::new(0x14, 0, 0xcb)];
        let (header, frame) = multi_color(7, false, 100, &colors, 100).unwrap();
        assert_eq!(encrypt(&header), hex!("29d3a784888e6dcd9baaccee9809a18c"));
        assert_eq!(frame, scene_frame(colors, 100));
    }

    #[test]
    fn multi_color_rejects_out_of_range_fields() {
        let max = [RED; MAX_SCENE_COLORS];
        assert!(multi_color(SCENE_EFFECT_COUNT - 1, false, 100, &max, 100).is_ok());
        assert!(multi_color(SCENE_EFFECT_COUNT, false, 50, &[RED], 100).is_err());
        assert!(multi_color(0, false, 50, &[], 100).is_err());
        assert!(multi_color(0, false, 50, &[RED; MAX_SCENE_COLORS + 1], 100).is_err());
        assert!(multi_color(0, false, 50, &[RED; 300], 100).is_err());
        assert!(multi_color(0, false, 101, &[RED], 100).is_err());
        assert!(multi_color(0, false, 50, &[RED], 101).is_err());
    }
}

fn scale(c: Rgb, pct: u8) -> Rgb {
    let ch = |v: u8| {
        if v == 0 {
            0
        } else {
            ((v as u16 * pct as u16 / 100) as u8).max(MIN_LIT_CHANNEL)
        }
    };
    Rgb::new(ch(c.r), ch(c.g), ch(c.b))
}

/// Sets the light's clock (`TIME`), as the app does on connecting; timers fire against it.
/// `weekday` is 1 (Monday) to 7 (Sunday); `app_version` is the vendor app's version digits,
/// e.g. `[3, 1, 7]`. Not acknowledged.
pub fn clock(
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
    app_version: [u8; 3],
) -> Result<Packet, OutOfRange> {
    check("weekday", weekday, 1, 7)?;
    check("hour", hour, 0, 23)?;
    check("minute", minute, 0, 59)?;
    check("second", second, 0, 59)?;
    for digit in app_version {
        check("app version digit", digit, 0, 9)?;
    }
    let [a, b, c] = app_version;
    let mut p = tagged(
        b"TIME",
        &[
            weekday, hour, minute, second, 0xa5, 0x5a, 0xa5, 0x01, a, b, c,
        ],
    );
    p[0] = 0x15; // the app sends 0x15, not the packet length
    Ok(p)
}

/// One daily timer. `days` has bit 0 for Monday through bit 6 for Sunday; 0 fires once, at the
/// next occurrence by the light's [`clock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timer {
    pub hour: u8,
    pub minute: u8,
    pub days: u8,
}

/// Sets both power timers (`SCHD`); `None` disables one. Acknowledged with `SCHDOK`.
pub fn timers(on: Option<Timer>, off: Option<Timer>) -> Result<Packet, OutOfRange> {
    let mut args = [0u8; 8];
    for (slot, timer) in args.chunks_mut(4).zip([on, off]) {
        if let Some(Timer { hour, minute, days }) = timer {
            check("hour", hour, 0, 23)?;
            check("minute", minute, 0, 59)?;
            check("days", days, 0, 0x7f)?;
            slot.copy_from_slice(&[1, days, hour, minute]);
        }
    }
    Ok(tagged(b"SCHD", &args))
}

#[cfg(test)]
mod timer_tests {
    use super::*;
    use hex_literal::hex;

    // Captured 2026-10-24: on at 19:00 and off at 23:00, every day.
    #[test]
    fn timers_match_capture() {
        let on = Timer {
            hour: 19,
            minute: 0,
            days: 0x7f,
        };
        let off = Timer { hour: 23, ..on };
        assert_eq!(
            encrypt(&timers(Some(on), Some(off)).unwrap()),
            hex!("d978305bd8db034ae6b09a010e25c994")
        );
        let ack = decrypt(&hex!("761610f8ae440c3f82023e61cdb75339"));
        assert_eq!(parse_ack(&ack), Some("SCHD"));
    }

    #[test]
    fn off_timer_fills_the_second_half() {
        let off = Timer {
            hour: 23,
            minute: 5,
            days: 0,
        };
        let p = timers(None, Some(off)).unwrap();
        assert_eq!(&p[..16], &hex!("0c534348440000000001001705000000"));
    }

    #[test]
    fn timers_reject_impossible_times() {
        let ok = Timer {
            hour: 23,
            minute: 59,
            days: 0x7f,
        };
        assert!(timers(Some(ok), Some(ok)).is_ok());
        for bad in [
            Timer { hour: 24, ..ok },
            Timer { minute: 60, ..ok },
            Timer { days: 0x80, ..ok },
        ] {
            assert!(timers(Some(bad), None).is_err());
            assert!(timers(None, Some(bad)).is_err());
        }
    }
}

/// Asks for one of the two firmware version strings (`VE 00` and `VE 01`); see [`parse_version`].
pub fn version_query(second: bool) -> Packet {
    tagged(b"VE", &[second as u8])
}

/// Parses a `VE` reply such as `V024.11.03.7`.
pub fn parse_version(plain: &Packet) -> Option<&str> {
    let len = plain[0] as usize;
    let text = std::str::from_utf8(plain.get(1..1 + len)?).ok()?;
    let text = text.trim_end_matches('\0');
    let mut chars = text.chars();
    (chars.next() == Some('V') && chars.next().is_some_and(|c| c.is_ascii_digit())).then_some(text)
}

/// Parses a decrypted notification. Only the first `len + 1` bytes are meaningful; the rest
/// is stale data from earlier replies.
pub fn parse_ack(plain: &Packet) -> Option<&str> {
    let len = plain[0] as usize;
    let body = plain.get(1..1 + len)?;
    let text = std::str::from_utf8(body).ok()?;
    text.strip_suffix("OK")
}

/// Number of music rhythm effects the app offers this product line.
pub const RHYTHM_EFFECT_COUNT: u8 = 10;

/// One music level sample for [`RHYTHM_CHAR_UUID`], at most one per 50 ms; the first switches the
/// light to music mode. `level` is 1–100. Not acknowledged.
pub fn rhythm(effect: u8, level: u8) -> Result<Packet, OutOfRange> {
    check("rhythm effect", effect, 0, RHYTHM_EFFECT_COUNT as u16 - 1)?;
    check("rhythm level", level, 1, 100)?;
    let mut p = [0u8; 16];
    p[..3].copy_from_slice(&[0x02, effect, level]);
    Ok(p)
}

/// Leaves music mode (`RSTOPR`) and restores the previous mode. Not acknowledged.
pub fn rhythm_stop() -> Packet {
    tagged(b"RSTOPR", &[])
}

/// Switches music mode to the light's own microphone (`MIC 01 64 00`), or back off (`MIC 00`),
/// as the app's microphone toggle does. Not acknowledged.
pub fn light_microphone(on: bool) -> Packet {
    if on {
        tagged(b"MIC", &[1, 100, 0])
    } else {
        tagged(b"MIC", &[0])
    }
}

#[cfg(test)]
mod rhythm_tests {
    use super::*;
    use hex_literal::hex;

    // Captured 2026-10-24 from the vendor app's music page, with the phone's microphone.
    #[test]
    fn rhythm_matches_capture() {
        for (effect, level, cipher) in [
            (0, 1, hex!("3e1142824947852021989cf5b3c35921")),
            (1, 0x17, hex!("771f81a56837572cfc6afa7384f427c9")),
            (2, 0x27, hex!("c2850100635a9070cbe9a44387aebb92")),
            (2, 0x4e, hex!("ec15bbf6137336f04254a3045768a369")),
        ] {
            assert_eq!(encrypt(&rhythm(effect, level).unwrap()), cipher);
        }
        assert_eq!(
            encrypt(&rhythm_stop()),
            hex!("b418a9994f9adab07d78a24008161c3f")
        );
    }

    #[test]
    fn light_microphone_matches_capture() {
        assert_eq!(
            encrypt(&light_microphone(true)),
            hex!("726e1c8c4da1f77727ee35abcb448fa7")
        );
        assert_eq!(
            encrypt(&light_microphone(false)),
            hex!("a6207a5cb9b6336ca99cf1cb5c304752")
        );
    }

    #[test]
    fn rhythm_rejects_out_of_range_fields() {
        assert!(rhythm(RHYTHM_EFFECT_COUNT, 50).is_err());
        assert!(rhythm(0, 0).is_err());
        assert!(rhythm(0, 101).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    // Ciphertexts captured from the vendor app talking to an ISP-50542B (firmware TR2411R003-07).
    #[test]
    fn power_matches_capture() {
        assert_eq!(
            encrypt(&power(false)),
            hex!("79d1dba40919c246a8580ae7d11b7884")
        );
        assert_eq!(
            encrypt(&power(true)),
            hex!("84dd5042374150897ac82f39110968a8")
        );
    }

    #[test]
    fn solid_colors_match_capture() {
        let cases = [
            (
                Rgb::new(0xff, 0, 0),
                hex!("73baa3e311ef375b0c2965911e7f5357"),
            ),
            (
                Rgb::new(0, 0xff, 0),
                hex!("a06566adefd31c59d5c85979620ce9ef"),
            ),
            (
                Rgb::new(0, 0, 0xff),
                hex!("c7f95a08c3583941208cca2d3e6e91c0"),
            ),
        ];
        for (color, expected) in cases {
            assert_eq!(encrypt(&solid_color(color, 100)), expected, "{color}");
        }
    }

    #[test]
    fn zero_brightness_bottoms_out_like_the_app() {
        assert_eq!(
            encrypt(&solid_color(Rgb::new(0xff, 0, 0), 0)),
            hex!("5a2097e40890b857a99a6b5f53668285")
        );
    }

    #[test]
    fn half_brightness_scales_channels() {
        let p = solid_color(Rgb::new(0xff, 0, 0), 50);
        assert_eq!(&p[9..15], &[0x7f, 0, 0, 0x7f, 0, 0]);
    }

    #[test]
    fn diy_pixels_match_capture() {
        assert_eq!(
            encrypt(&diy_pixel(0, Rgb::new(0xa5, 0x15, 0x15), 0x41).unwrap()),
            hex!("89875e93862614aca7f70d38702bff7b")
        );
        assert_eq!(
            encrypt(&diy_pixel(179, Rgb::new(0xa5, 0, 0), 50).unwrap()),
            hex!("746a837fe5d1e12408d9e11f34df95a2")
        );
    }

    #[test]
    fn diy_rejects_indexes_past_the_firmware_buffer() {
        let red = Rgb::new(0xff, 0, 0);
        assert!(diy_pixel(MAX_LEDS - 1, red, 50).is_ok());
        assert!(diy_pixel(MAX_LEDS, red, 50).is_err());
        assert!(diy_pixel(u16::MAX, red, 50).is_err());
        assert!(diy_fill(MAX_LEDS, red, 50).is_ok());
        assert!(diy_fill(MAX_LEDS + 1, red, 50).is_err());
    }

    #[test]
    fn diy_fill_matches_capture() {
        assert_eq!(
            encrypt(&diy_fill(180, Rgb::new(0x56, 0xa5, 0x42), 50).unwrap()),
            hex!("dcdd7fe7cbf6235242153c019b839bb9")
        );
    }

    #[test]
    fn diy_enter_and_exit_match_capture() {
        assert_eq!(
            encrypt(&diy_enter()),
            hex!("cbe9305dbde55cf3f8f13873f0920f2f")
        );
        assert_eq!(
            encrypt(&diy_exit(true)),
            hex!("5eb9c3c21f0457240f0c39e0d0953039")
        );
        assert_eq!(
            encrypt(&diy_exit(false)),
            hex!("5724de0f03efb16acbe078544cbc4ce1")
        );
    }

    #[test]
    fn scenes_match_capture() {
        assert_eq!(
            encrypt(&scene(false, 5, 0x1d)),
            hex!("612074c06b15b5b0616f2569e669062c")
        );
        assert_eq!(
            encrypt(&scene(false, 100, 100)),
            hex!("29d3a784888e6dcd9baaccee9809a18c")
        );
        assert_eq!(
            encrypt(&scene(true, 100, 100)),
            hex!("c8d3bced6b94ddee42f40213ea1866ce")
        );
    }

    #[test]
    fn scene_frames_match_capture() {
        let colors = [Rgb::new(0xff, 0, 0xff), Rgb::new(0x14, 0, 0xcb)];
        let dim = scene_frame(colors, 0x1d);
        assert_eq!(&dim[..8], &hex!("070049004905003a"));
        assert!(dim[8..].iter().all(|&b| b == 0));
        assert_eq!(&scene_frame(colors, 5)[..8], &hex!("07000c000c01000a"));
        assert_eq!(&scene_frame(colors, 100)[..8], &hex!("0700ff00ff1400cb"));
    }

    #[test]
    fn decrypts_scene_acks() {
        let mult = decrypt(&hex!("c68ed93a891110cab5c8067f843d04e6"));
        let done = decrypt(&hex!("33e943ec5a7cfc5975cdaae497bf25f2"));
        assert_eq!(parse_ack(&mult), Some("MULT"));
        assert_eq!(parse_ack(&done), Some("MULTCP"));
    }

    #[test]
    fn version_queries_match_capture() {
        assert_eq!(
            encrypt(&version_query(false)),
            hex!("263a91cc5f1bf003ce437b733478ada2")
        );
        assert_eq!(
            encrypt(&version_query(true)),
            hex!("d12e61ad95a00bd3c56bee6f9cb545fe")
        );
    }

    #[test]
    fn clock_matches_capture() {
        assert_eq!(
            encrypt(&clock(6, 8, 4, 38, [3, 1, 7]).unwrap()),
            hex!("b7f8ad85115c2ad8d8a7692cae99270e")
        );
    }

    #[test]
    fn clock_rejects_impossible_times() {
        assert!(clock(7, 23, 59, 59, [9, 9, 9]).is_ok());
        assert!(clock(0, 8, 4, 38, [3, 1, 7]).is_err());
        assert!(clock(8, 8, 4, 38, [3, 1, 7]).is_err());
        assert!(clock(6, 24, 4, 38, [3, 1, 7]).is_err());
        assert!(clock(6, 8, 60, 38, [3, 1, 7]).is_err());
        assert!(clock(6, 8, 4, 60, [3, 1, 7]).is_err());
        assert!(clock(6, 8, 4, 38, [3, 10, 7]).is_err());
    }

    #[test]
    fn parses_version_replies() {
        let first = decrypt(&hex!("46ab4f31aab2c94a189d69be3420fabc"));
        let second = decrypt(&hex!("28666480d835f1b014ca8ea867a606ab"));
        assert_eq!(parse_version(&first), Some("V024.11.03.7"));
        assert_eq!(parse_version(&second), Some("V124.11.66.1"));
        let ack = decrypt(&hex!("e04875cfea0dc30d20e2a01dfb20c6b6"));
        assert_eq!(parse_version(&ack), None);
    }

    #[test]
    fn decrypts_acks() {
        let turn = decrypt(&hex!("e04875cfea0dc30d20e2a01dfb20c6b6"));
        let sgls = decrypt(&hex!("9377d9a5ce41387fbfc001984adabf47"));
        let dood = decrypt(&hex!("f6104e6c6768a68fb3be40feeb5791d0"));
        assert_eq!(parse_ack(&turn), Some("TURN"));
        assert_eq!(parse_ack(&sgls), Some("SGLS"));
        assert_eq!(parse_ack(&dood), Some("DOOD"));
    }

    #[test]
    fn round_trips() {
        let p = power(true);
        assert_eq!(decrypt(&encrypt(&p)), p);
    }

    #[test]
    fn recognizes_names() {
        assert!(is_ideal_name("ISP-50542B"));
        assert!(is_ideal_name("IDL-1234"));
        assert!(!is_ideal_name("MyQ-9C2"));
    }
}
