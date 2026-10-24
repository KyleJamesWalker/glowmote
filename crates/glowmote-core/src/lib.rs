//! Protocol encoders for vendor LED controllers. Pure byte-level code, no I/O.

pub mod color;
pub mod ideal;

pub use color::Rgb;
