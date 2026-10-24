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
pub const VERSION_CHAR_UUID: &str = "d44bc439-abfd-45a2-b575-925416129602";

pub const NAME_PREFIXES: [&str; 2] = ["ISP-", "IDL-"];

const KEY: [u8; 16] = [
    0x34, 0x52, 0x2a, 0x5b, 0x7a, 0x6e, 0x49, 0x2c, 0x08, 0x09, 0x0a, 0x9d, 0x8d, 0x2a, 0x23, 0xf8,
];

/// The vendor app never sends a lit channel below this, even with brightness at 0%.
const MIN_LIT_CHANNEL: u8 = 2;

pub type Packet = [u8; 16];

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

/// Solid colour. Brightness is applied by scaling the channels, as the vendor app does.
pub fn solid_color(color: Rgb, brightness_pct: u8) -> Packet {
    let Rgb { r, g, b } = scale(color, brightness_pct.min(100));
    // mode 0 (solid), direction 0, fixed 100, speed 0x50, colour twice, saturation 0x32
    tagged(b"SGLS", &[0x00, 0x00, 0x64, 0x50, r, g, b, r, g, b, 0x32])
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

/// Parses a decrypted notification. Only the first `len + 1` bytes are meaningful; the rest
/// is stale data from earlier replies.
pub fn parse_ack(plain: &Packet) -> Option<&str> {
    let len = plain[0] as usize;
    let body = plain.get(1..1 + len)?;
    let text = std::str::from_utf8(body).ok()?;
    text.strip_suffix("OK")
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
    fn decrypts_acks() {
        let turn = decrypt(&hex!("e04875cfea0dc30d20e2a01dfb20c6b6"));
        let sgls = decrypt(&hex!("9377d9a5ce41387fbfc001984adabf47"));
        assert_eq!(parse_ack(&turn), Some("TURN"));
        assert_eq!(parse_ack(&sgls), Some("SGLS"));
    }

    #[test]
    fn round_trips() {
        let p = power(true);
        assert_eq!(decrypt(&encrypt(&p)), p);
    }

    #[test]
    fn recognises_names() {
        assert!(is_ideal_name("ISP-50542B"));
        assert!(is_ideal_name("IDL-1234"));
        assert!(!is_ideal_name("MyQ-9C2"));
    }
}
