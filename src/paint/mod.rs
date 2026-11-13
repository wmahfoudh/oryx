mod band;
pub mod gutter;
pub mod painter;
pub mod preview;
pub mod scroll;

pub use band::{band, band_numbered, paper};

/// Packs a screen pixel for the frame buffer: `0xFFRRGGBB`. The buffer
/// is `XRGB8888`, whose top byte the protocol leaves unused and
/// softbuffer documents as zero, but KWin 6.8 reads it as the pixel's
/// opacity: a zero there draws the window transparent. Keep it at 0xFF.
pub fn pixel(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
