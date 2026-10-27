//! 6x13 glyphs as row bitmasks, bit `x` set where column `x` is ink.
//!
//! Block elements and braille are drawn here, so they fill the cell exactly
//! and tile without seams; everything else comes from the X11 6x13 fonts in
//! `embedded-graphics-unicodefonts`, rasterized once per character on first
//! use and cached.

use alloc::vec::Vec;

use embedded_graphics::Drawable;
use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Point, Size};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics_unicodefonts::{MONO_6X13, MONO_6X13_BOLD};

pub const W: usize = 6;
pub const H: usize = 13;

pub type Glyph = [u8; H];

const FULL_ROW: u8 = 0b11_1111;
/// The first row of a cell's lower half; the upper half is a row taller.
const MID: usize = 7;
const PRINTABLE: core::ops::RangeInclusive<char> = ' '..='~';

pub struct Glyphs {
    ascii: [[Glyph; 95]; 2],
    /// Other characters seen so far, sorted by `key`.
    cached: Vec<(u32, Glyph)>,
}

impl Glyphs {
    pub fn new() -> Self {
        let mut ascii = [[[0; H]; 95]; 2];
        for (bold, set) in ascii.iter_mut().enumerate() {
            for (glyph, c) in set.iter_mut().zip(PRINTABLE) {
                *glyph = rasterize(c, bold == 1);
            }
        }
        Self {
            ascii,
            cached: Vec::new(),
        }
    }

    pub fn get(&mut self, c: char, bold: bool) -> Glyph {
        if PRINTABLE.contains(&c) {
            let index = c as usize - ' ' as usize;
            return self
                .ascii
                .get(usize::from(bold))
                .and_then(|set| set.get(index))
                .copied()
                .unwrap_or([0; H]);
        }
        if let Some(glyph) = drawn(c) {
            return glyph;
        }
        let key = (u32::from(c) << 1) | u32::from(bold);
        match self.cached.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(i) => self.cached.get(i).map_or([0; H], |&(_, g)| g),
            Err(i) => {
                let glyph = rasterize(c, bold);
                self.cached.insert(i, (key, glyph));
                glyph
            }
        }
    }
}

impl Default for Glyphs {
    fn default() -> Self {
        Self::new()
    }
}

fn rasterize(c: char, bold: bool) -> Glyph {
    let font: &MonoFont = if bold { &MONO_6X13_BOLD } else { &MONO_6X13 };
    let mut capture = Capture([0; H]);
    let mut utf8 = [0u8; 4];
    let style = MonoTextStyle::new(font, BinaryColor::On);
    let _ = Text::with_baseline(
        c.encode_utf8(&mut utf8),
        Point::zero(),
        style,
        Baseline::Top,
    )
    .draw(&mut capture);
    capture.0
}

struct Capture(Glyph);

impl OriginDimensions for Capture {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Capture {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<BinaryColor>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, color) in pixels {
            if color.is_on()
                && (0..W as i32).contains(&p.x)
                && let Some(row) = usize::try_from(p.y).ok().and_then(|y| self.0.get_mut(y))
            {
                *row |= 1 << p.x;
            }
        }
        Ok(())
    }
}

/// Rows `top..bottom` filled across columns `left..right`.
fn rect(left: usize, right: usize, top: usize, bottom: usize) -> Glyph {
    let mask = (FULL_ROW >> (W - right.min(W))) & !((1u8 << left.min(W)) - 1);
    let mut g = [0; H];
    for row in g.iter_mut().take(bottom.min(H)).skip(top) {
        *row = mask;
    }
    g
}

fn or(a: Glyph, b: Glyph) -> Glyph {
    let mut g = a;
    for (x, y) in g.iter_mut().zip(b) {
        *x |= y;
    }
    g
}

/// Eighths of the cell height or width, rounded.
fn eighths(n: usize, of: usize) -> usize {
    (n * of + 4) / 8
}

/// Block elements (U+2580..U+259F) and braille (U+2800..U+28FF).
fn drawn(c: char) -> Option<Glyph> {
    let code = u32::from(c);
    if (0x2800..=0x28FF).contains(&code) {
        return Some(braille(code - 0x2800));
    }
    let quadrant = |tl: bool, tr: bool, bl: bool, br: bool| {
        let parts = [
            (tl, rect(0, 3, 0, MID)),
            (tr, rect(3, 6, 0, MID)),
            (bl, rect(0, 3, MID, H)),
            (br, rect(3, 6, MID, H)),
        ];
        parts
            .into_iter()
            .filter(|&(on, _)| on)
            .fold([0; H], |g, (_, part)| or(g, part))
    };
    Some(match c {
        '▀' => rect(0, W, 0, MID),
        '▄' => rect(0, W, MID, H),
        '▁'..='▇' => rect(0, W, H - eighths(code as usize - 0x2580, H), H),
        '█' => rect(0, W, 0, H),
        '▉'..='▏' => rect(0, eighths(0x2590 - code as usize, W), 0, H),
        '▐' => rect(3, W, 0, H),
        '░' | '▒' | '▓' => shade(c),
        '▔' => rect(0, W, 0, eighths(1, H)),
        '▕' => rect(W - 1, W, 0, H),
        '▖' => quadrant(false, false, true, false),
        '▗' => quadrant(false, false, false, true),
        '▘' => quadrant(true, false, false, false),
        '▙' => quadrant(true, false, true, true),
        '▚' => quadrant(true, false, false, true),
        '▛' => quadrant(true, true, true, false),
        '▜' => quadrant(true, true, false, true),
        '▝' => quadrant(false, true, false, false),
        '▞' => quadrant(false, true, true, false),
        '▟' => quadrant(false, true, true, true),
        _ => return None,
    })
}

fn shade(c: char) -> Glyph {
    let mut g = [0; H];
    for (y, row) in g.iter_mut().enumerate() {
        *row = match c {
            '░' => {
                if y % 2 == 0 {
                    0b01_0001 << (y / 2 % 2)
                } else {
                    0
                }
            }
            '▒' => {
                if y % 2 == 0 {
                    0b01_0101
                } else {
                    0b10_1010
                }
            }
            _ => {
                if y % 2 == 0 {
                    !0b01_0001u8 & FULL_ROW
                } else {
                    FULL_ROW
                }
            }
        };
    }
    g
}

/// Dots of 2x2 pixels in two columns and four rows.
fn braille(bits: u32) -> Glyph {
    // Unicode's dot numbering: (bit, column, row).
    const DOTS: [(u32, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 1),
        (2, 0, 2),
        (3, 1, 0),
        (4, 1, 1),
        (5, 1, 2),
        (6, 0, 3),
        (7, 1, 3),
    ];
    DOTS.iter()
        .filter(|&&(bit, _, _)| bits & (1 << bit) != 0)
        .fold([0; H], |g, &(_, col, row)| {
            let left = 1 + col * 3;
            let top = 1 + row * 3;
            or(g, rect(left, left + 2, top, top + 2))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(g: &Glyph) -> u32 {
        g.iter().map(|r| r.count_ones()).sum()
    }

    #[test]
    fn complementary_blocks_tile_the_cell_exactly() {
        let pairs = [('▀', '▄'), ('▌', '▐'), ('▘', '▟'), ('▚', '▞'), ('▛', '▗')];
        for (a, b) in pairs {
            let (a, b) = (drawn(a).expect("block"), drawn(b).expect("block"));
            for (x, y) in a.iter().zip(b) {
                assert_eq!(x & y, 0);
                assert_eq!(x | y, FULL_ROW);
            }
        }
        assert_eq!(ink(&drawn('█').expect("full")), (W * H) as u32);
    }

    #[test]
    fn braille_has_one_dot_per_bit() {
        assert_eq!(ink(&braille(0)), 0);
        assert_eq!(ink(&braille(0xFF)), 8 * 4);
        assert_eq!(ink(&braille(1 << 6)), 4);
    }

    #[test]
    fn fonts_rasterize_and_cache() {
        let mut glyphs = Glyphs::new();
        assert!(ink(&glyphs.get('A', false)) > 0);
        assert_ne!(glyphs.get('A', false), glyphs.get('A', true));
        let rounded = glyphs.get('╭', false);
        assert!(ink(&rounded) > 0);
        assert_eq!(glyphs.get('╭', false), rounded);
        assert_eq!(glyphs.cached.len(), 1);
        assert_eq!(ink(&glyphs.get(' ', false)), 0);
    }
}
