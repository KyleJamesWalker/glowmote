use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected a color as RRGGBB hex, got {0:?}")]
pub struct ParseRgbError(String);

impl FromStr for Rgb {
    type Err = ParseRgbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.trim_start_matches('#');
        let err = || ParseRgbError(s.to_owned());
        if hex.len() != 6 {
            return Err(err());
        }
        let n = u32::from_str_radix(hex, 16).map_err(|_| err())?;
        Ok(Self::new((n >> 16) as u8, (n >> 8) as u8, n as u8))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_hash() {
        assert_eq!("ff8000".parse::<Rgb>().unwrap(), Rgb::new(0xff, 0x80, 0));
        assert_eq!("#00ff7f".parse::<Rgb>().unwrap(), Rgb::new(0, 0xff, 0x7f));
    }

    #[test]
    fn rejects_bad_hex() {
        assert!("fff".parse::<Rgb>().is_err());
        assert!("gg0000".parse::<Rgb>().is_err());
    }
}
