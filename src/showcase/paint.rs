//! Colours, themes, ink shaders and backgrounds, ported from
//! ratatui-showcase's WGSL (`theme.rs`, `theme.wgsl`, `backdrop.wgsl`) to
//! run per sample on the CPU.

use core::f32::consts::{PI, TAU};

use super::fast::{atan2f, cosf, expf, floorf, log2f, powf, sinf, sqrtf};
use ratatui::style::Color;

use super::fractal::Fractal;
use super::{MAX_W, Rng};

// ---------------------------------------------------------------- maths

pub fn wrap(x: f32) -> f32 {
    x - floorf(x)
}

fn sq(x: f32) -> f32 {
    x * x
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xff_ffff) as f32 / 16_777_216.0
}

fn hashf(x: f32, y: f32) -> f32 {
    hash(x.max(0.0) as u32, y.max(0.0) as u32)
}

fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (floorf(x), floorf(y));
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hashf(ix + 4096.0, iy + 4096.0);
    let b = hashf(ix + 4097.0, iy + 4096.0);
    let c = hashf(ix + 4096.0, iy + 4097.0);
    let d = hashf(ix + 4097.0, iy + 4097.0);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Three octaves (the GPU uses four; the badge has less to spare).
fn fbm(mut x: f32, mut y: f32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    for _ in 0..3 {
        sum += amp * value_noise(x, y);
        x = x * 2.03 + 17.0;
        y = y * 2.03 + 9.0;
        amp *= 0.5;
    }
    sum / 0.875
}

// ---------------------------------------------------------------- colour

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

pub const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
pub const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);

impl Rgb {
    pub const fn hex(hex: u32) -> Self {
        Self(
            ((hex >> 16) & 0xff) as f32 / 255.0,
            ((hex >> 8) & 0xff) as f32 / 255.0,
            (hex & 0xff) as f32 / 255.0,
        )
    }

    pub fn lerp(self, o: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb(
            self.0 + (o.0 - self.0) * t,
            self.1 + (o.1 - self.1) * t,
            self.2 + (o.2 - self.2) * t,
        )
    }

    /// Blends in linear light.
    pub fn mix(self, o: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let ch = |a: f32, b: f32| sqrtf((a * a + (b * b - a * a) * t).max(0.0));
        Rgb(ch(self.0, o.0), ch(self.1, o.1), ch(self.2, o.2))
    }

    pub fn scale(self, k: f32) -> Rgb {
        Rgb(self.0 * k, self.1 * k, self.2 * k)
    }

    pub fn add(self, o: Rgb) -> Rgb {
        Rgb(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }

    pub fn dist2(self, o: Rgb) -> f32 {
        let (r, g, b) = (self.0 - o.0, self.1 - o.1, self.2 - o.2);
        r * r + g * g + b * b
    }

    pub fn color(self) -> Color {
        let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Color::Rgb(ch(self.0), ch(self.1), ch(self.2))
    }
}

fn hsl(h: f32, s: f32, l: f32) -> Rgb {
    let a = s * l.min(1.0 - l);
    let ch = |n: f32| {
        let k = (n + wrap(h) * 12.0) % 12.0;
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Rgb(ch(0.0), ch(8.0), ch(4.0))
}

/// A colour from OKLCH, chroma lowered until it fits in sRGB.
fn oklch(l: f32, c: f32, hue: f32) -> Rgb {
    let (sin, cos) = (sinf(hue.to_radians()), cosf(hue.to_radians()));
    let mut chroma = c;
    loop {
        let (a, b) = (chroma * cos, chroma * sin);
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
        let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        let linear = [
            4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
            -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
            -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
        ];
        if linear.iter().all(|v| (-0.001..=1.001).contains(v)) || chroma <= 0.0 {
            let encode = |v: f32| {
                let v = v.clamp(0.0, 1.0);
                if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * powf(v, 1.0 / 2.4) - 0.055
                }
            };
            let [r, g, b] = linear.map(encode);
            return Rgb(r, g, b);
        }
        chroma -= 0.01;
    }
}

// ---------------------------------------------------------------- palettes

struct Theme {
    name: &'static str,
    background: u32,
    stops: &'static [u32],
    accent: u32,
}

const THEMES: &[Theme] = &[
    Theme {
        name: "Classic",
        background: 0x0a0a0c,
        stops: &[0xd0d0d0, 0xffffff, 0xa8a8a8, 0xe4e4e4],
        accent: 0xff3030,
    },
    Theme {
        name: "Aurora",
        background: 0x050a10,
        stops: &[0x00f5a0, 0x00d9f5, 0x7b5cff, 0xff4ecd],
        accent: 0xffe066,
    },
    Theme {
        name: "Synthwave",
        background: 0x0d0518,
        stops: &[0xff2a6d, 0xd300c5, 0x7a04eb, 0x05d9e8],
        accent: 0xf9f871,
    },
    Theme {
        name: "Ember",
        background: 0x100604,
        stops: &[0xff3d00, 0xff9100, 0xffd740, 0xff6e40],
        accent: 0xfff3c4,
    },
    Theme {
        name: "Ocean",
        background: 0x030b16,
        stops: &[0x0077b6, 0x00b4d8, 0x90e0ef, 0x48cae4],
        accent: 0xffd166,
    },
    Theme {
        name: "Candy",
        background: 0x0e0812,
        stops: &[0xff99c8, 0xfcf6bd, 0xd0f4de, 0xa9def9, 0xe4c1f9],
        accent: 0xff595e,
    },
    Theme {
        name: "Phosphor",
        background: 0x020a04,
        stops: &[0x00ff41, 0x14a830, 0x39ff14, 0x0b6e1f],
        accent: 0xd8ffd8,
    },
    Theme {
        name: "Prism",
        background: 0x080808,
        stops: &[0xff595e, 0xffca3a, 0x8ac926, 0x1982c4, 0x6a4c93],
        accent: 0xffffff,
    },
    Theme {
        name: "Sunset",
        background: 0x120714,
        stops: &[0xff7e5f, 0xfeb47b, 0xff6f91, 0x845ec2],
        accent: 0xffe8a8,
    },
    Theme {
        name: "Glacier",
        background: 0x040a12,
        stops: &[0xe0fbfc, 0x98c1d9, 0x5c8fd6, 0xc2dfe3],
        accent: 0x7df9ff,
    },
    Theme {
        name: "Toxic",
        background: 0x060a02,
        stops: &[0xccff00, 0x39ff14, 0x00ffcc, 0xfaff00],
        accent: 0xff00aa,
    },
];

/// A scene's colours: one of the themes, or a harmony made up for it.
#[derive(Clone)]
pub struct Palette {
    pub name: &'static str,
    /// Which theme, or the hue of a generated palette.
    pub detail: Option<u16>,
    pub background: Rgb,
    stops: [Rgb; 5],
    count: usize,
    pub accent: Rgb,
}

impl Palette {
    pub fn theme(index: usize) -> Self {
        const FALLBACK: Theme = Theme {
            name: "Classic",
            background: 0x0a0a0c,
            stops: &[0xffffff],
            accent: 0xff3030,
        };
        let theme = THEMES.get(index % THEMES.len()).unwrap_or(&FALLBACK);
        let mut stops = [BLACK; 5];
        for (s, &h) in stops.iter_mut().zip(theme.stops) {
            *s = Rgb::hex(h);
        }
        Self {
            name: theme.name,
            detail: None,
            background: Rgb::hex(theme.background),
            stops,
            count: theme.stops.len().min(5),
            accent: Rgb::hex(theme.accent),
        }
    }

    pub fn random(rng: &mut Rng) -> Self {
        if rng.f32() < 0.45 {
            Self::generated(rng)
        } else {
            Self::theme(rng.below(THEMES.len()))
        }
    }

    /// The next of the named themes after this one.
    pub fn next_theme(&self) -> Self {
        let i = THEMES
            .iter()
            .position(|t| t.name == self.name)
            .map_or(0, |i| i + 1);
        Self::theme(i)
    }

    fn generated(rng: &mut Rng) -> Self {
        let hue = rng.f32() * 360.0;
        let (name, offsets): (&'static str, [f32; 5]) = match rng.below(6) {
            0 => ("analogous", [-40.0, -20.0, 0.0, 20.0, 40.0]),
            1 => ("complementary", [0.0, 20.0, 180.0, 200.0, -15.0]),
            2 => ("triadic", [0.0, 120.0, 240.0, 30.0, 150.0]),
            3 => ("split", [0.0, 150.0, 210.0, 15.0, 165.0]),
            4 => ("tetradic", [0.0, 90.0, 180.0, 270.0, 45.0]),
            _ => ("tonal", [0.0, 8.0, -8.0, 16.0, -16.0]),
        };
        let mut stops = [BLACK; 5];
        for (i, (s, offset)) in stops.iter_mut().zip(offsets).enumerate() {
            let light = if name == "tonal" {
                0.45 + 0.1 * i as f32
            } else {
                0.62 + rng.f32() * 0.22
            };
            *s = oklch(light, 0.09 + rng.f32() * 0.1, hue + offset);
        }
        Self {
            name,
            detail: Some(hue as u16),
            background: oklch(0.14, 0.03, hue),
            stops,
            count: 5,
            accent: oklch(0.9, 0.12, hue + 180.0),
        }
    }

    /// The looping gradient at `t`.
    pub fn sample(&self, t: f32) -> Rgb {
        let n = self.count.max(1);
        let p = wrap(t) * n as f32;
        let i = (p as usize) % n;
        let at = |k: usize| self.stops.get(k % n).copied().unwrap_or(BLACK);
        let f = wrap(p);
        at(i).lerp(at(i + 1), f * f * (3.0 - 2.0 * f))
    }
}

// ---------------------------------------------------------------- ink shaders

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shader {
    Flow,
    Plasma,
    Pulse,
    Sheen,
    Rainbow,
    Scanline,
    Chrome,
    Holo,
    Fire,
    Stripes,
    Marble,
    Gold,
    Glitter,
}

pub const SHADERS: &[Shader] = &[
    Shader::Flow,
    Shader::Plasma,
    Shader::Pulse,
    Shader::Sheen,
    Shader::Rainbow,
    Shader::Scanline,
    Shader::Chrome,
    Shader::Holo,
    Shader::Fire,
    Shader::Stripes,
    Shader::Marble,
    Shader::Gold,
    Shader::Glitter,
];

impl Shader {
    pub fn name(self) -> &'static str {
        match self {
            Shader::Flow => "flow",
            Shader::Plasma => "plasma",
            Shader::Pulse => "pulse",
            Shader::Sheen => "sheen",
            Shader::Rainbow => "rainbow",
            Shader::Scanline => "scanline",
            Shader::Chrome => "chrome",
            Shader::Holo => "holo",
            Shader::Fire => "fire",
            Shader::Stripes => "stripes",
            Shader::Marble => "marble",
            Shader::Gold => "gold",
            Shader::Glitter => "glitter",
        }
    }

    /// The ink colour at `(u, v)` within the art (0..1 each way).
    pub fn shade(self, p: &Palette, u: f32, v: f32, t: f32, row: u32) -> Rgb {
        let d = sqrtf((u - 0.5) * (u - 0.5) + (v - 0.5) * (v - 0.5));
        match self {
            Shader::Flow => p.sample(0.6 * u + 0.4 * v - 0.08 * t),
            Shader::Plasma => {
                let (px, py) = (u * 2.4, v * 1.5);
                let dx = fbm(px * 1.4 + t * 0.13, py * 1.4 - t * 0.08);
                let dy = fbm(px * 1.4 + 5.2 - t * 0.1, py * 1.4 + 1.3 + t * 0.06);
                let (qx, qy) = (px + (dx - 0.5) * 2.6, py + (dy - 0.5) * 2.6);
                let cx = qx - (1.2 + 0.4 * sinf(t * 0.3));
                let cy = qy - 0.75;
                let w = sinf(qx * 5.0 + t * 0.9)
                    + sinf(qy * 6.0 - t * 1.15)
                    + sinf((qx - qy) * 3.5 + t * 0.6)
                    + sinf(sqrtf(cx * cx + cy * cy) * 8.0 - t * 1.7);
                let n = w * 0.25;
                let band = p.sample(n * 0.55 + dx * 0.8 + t * 0.025);
                let lift = 0.4 + 0.6 * smoothstep(-0.9, 0.7, n);
                let filament = 1.0 - smoothstep(0.0, 0.16, (wrap(w * 0.7 + t * 0.15) - 0.5).abs());
                band.scale(lift)
                    .lerp(WHITE, filament * 0.55 + smoothstep(0.75, 1.0, n) * 0.3)
            }
            Shader::Pulse => p.sample(d * 1.6 - t * 0.15),
            Shader::Sheen => p
                .sample(u * 0.5 + t * 0.03)
                .lerp(WHITE, sweep(u + v * 0.6, t) * 0.8),
            Shader::Rainbow => hsl(u * 0.9 + v * 0.3 - t * 0.1, 0.85, 0.62),
            Shader::Scanline => {
                let line = if row.is_multiple_of(2) { 1.0 } else { 0.72 };
                let roll_at = t * 0.25 - floorf(t * 0.25 / 1.4) * 1.4;
                let roll = expf(-sq((v - roll_at + 0.2) * 9.0));
                let flicker = 0.97 + 0.03 * sinf(t * 13.0 * TAU);
                p.sample(v * 0.8 + t * 0.05)
                    .scale(line * flicker)
                    .lerp(WHITE, roll * 0.35)
            }
            Shader::Chrome => {
                let horizon = 0.55 + 0.03 * sinf(u * 6.0 + t * 0.7);
                let chrome = if v < horizon {
                    Rgb::hex(0x1a2a6c).mix(p.sample(0.55).lerp(WHITE, 0.55), v / horizon)
                } else {
                    let ground = p.sample(0.1).lerp(Rgb::hex(0xffcf70), 0.4);
                    Rgb::hex(0x2b1200).mix(ground, powf((v - horizon) / (1.0 - horizon), 0.7))
                };
                chrome.lerp(WHITE, sweep(u + v * 0.3, t) * 0.9)
            }
            Shader::Holo => {
                let angle = atan2f(v - 0.5, u - 0.5) / TAU;
                let foil = hsl(angle * 2.0 + d * 1.5 - t * 0.12, 0.7, 0.72);
                foil.mix(p.sample(u + t * 0.05), 0.35)
                    .lerp(WHITE, sparkle(u, v, t, 40.0) * 0.9)
            }
            Shader::Fire => {
                let flicker = sinf(u * 23.0 + t * 7.0) * 0.5 + sinf(u * 41.0 - t * 11.0) * 0.25;
                let heat =
                    (1.15 - v + flicker * 0.12 + sinf(t * 3.0 + u * 9.0) * 0.05).clamp(0.0, 1.0);
                let flame = Rgb::hex(0x400000)
                    .mix(Rgb::hex(0xff3000), heat * 1.6)
                    .mix(Rgb::hex(0xffd23f), (heat - 0.5) * 2.0)
                    .mix(WHITE, (heat - 0.85) * 4.0);
                flame.mix(p.sample(heat * 0.5), 0.25)
            }
            Shader::Stripes => {
                let w = wrap((u * 2.0 + v * 1.2) * 4.0 - t * 0.6);
                let band = floorf(w * 2.0) / 2.0;
                p.sample(band * 0.5 + t * 0.03)
                    .lerp(WHITE, if w < 0.06 { 0.4 } else { 0.0 })
            }
            Shader::Marble => {
                let warp = fbm(u * 3.0 + t * 0.05, v * 1.5) * 4.0;
                let vein = sinf((u * 4.0 + v * 2.0 + warp) * PI).abs();
                p.sample(warp * 0.2 + t * 0.02).mix(WHITE.scale(0.95), {
                    let a = 1.0 - vein;
                    sq(a * a * a) * 0.8
                })
            }
            Shader::Gold => {
                let base = Rgb::hex(0x6b4400).mix(
                    Rgb::hex(0xffd66b),
                    0.5 + 0.5 * sinf(v * 9.0 + u * 2.0 - t * 1.5),
                );
                base.mix(p.sample(0.2), 0.15)
                    .lerp(WHITE, sweep(u * 1.2 + v * 0.4, t * 1.3) * 0.9)
            }
            Shader::Glitter => {
                let base = p.sample(u * 0.7 + v * 0.3 + t * 0.04);
                let glint =
                    sparkle(u, v, t * 3.0, 60.0) + sparkle(u + 0.37, v + 0.11, t * 2.3, 40.0);
                base.scale(0.85).lerp(WHITE, glint)
            }
        }
    }
}

fn sweep(position: f32, t: f32) -> f32 {
    let phase = (t * 0.35 - floorf(t * 0.35 / 2.0) * 2.0) * 1.6 - 0.3;
    expf(-sq(position * 0.8 - phase) * 60.0)
}

fn sparkle(u: f32, v: f32, t: f32, density: f32) -> f32 {
    let cell = ((u * density).max(0.0) as u32).wrapping_mul(7919)
        ^ ((v * density * 0.5).max(0.0) as u32).wrapping_mul(104_729);
    let phase = (cell % 997) as f32 / 997.0;
    let s = sinf((t * 0.8 + phase * 13.0) * TAU);
    if s > 0.985 { (s - 0.985) / 0.015 } else { 0.0 }
}

// ---------------------------------------------------------------- backgrounds

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    Vignette,
    Stars,
    Grid,
    Aurora,
    Rays,
    Julia,
    Kaleidoscope,
    Tunnel,
    Warp,
    Metaballs,
    Mandelbrot,
    Ship,
    Newton,
    Apollonian,
    Hyperbolic,
    Domain,
    Glass,
    Mandala,
    Mandelbox,
    Kaliset,
    Quaternion,
    Quasicrystal,
}

pub const BACKDROPS: &[Backdrop] = &[
    Backdrop::Vignette,
    Backdrop::Stars,
    Backdrop::Grid,
    Backdrop::Aurora,
    Backdrop::Rays,
    Backdrop::Julia,
    Backdrop::Kaleidoscope,
    Backdrop::Tunnel,
    Backdrop::Warp,
    Backdrop::Metaballs,
    Backdrop::Mandelbrot,
    Backdrop::Ship,
    Backdrop::Newton,
    Backdrop::Apollonian,
    Backdrop::Hyperbolic,
    Backdrop::Domain,
    Backdrop::Glass,
    Backdrop::Mandala,
    Backdrop::Mandelbox,
    Backdrop::Kaliset,
    Backdrop::Quaternion,
    Backdrop::Quasicrystal,
];

impl Backdrop {
    pub fn name(self) -> &'static str {
        match self {
            Backdrop::Vignette => "vignette",
            Backdrop::Stars => "stars",
            Backdrop::Grid => "synthwave grid",
            Backdrop::Aurora => "aurora",
            Backdrop::Rays => "sun rays",
            Backdrop::Julia => "julia",
            Backdrop::Kaleidoscope => "kaleidoscope",
            Backdrop::Tunnel => "tunnel",
            Backdrop::Warp => "warp",
            Backdrop::Metaballs => "metaballs",
            Backdrop::Mandelbrot => "mandelbrot",
            Backdrop::Ship => "burning ship",
            Backdrop::Newton => "newton",
            Backdrop::Apollonian => "apollonian",
            Backdrop::Hyperbolic => "hyperbolic",
            Backdrop::Domain => "domain colouring",
            Backdrop::Glass => "stained glass",
            Backdrop::Mandala => "mandala",
            Backdrop::Mandelbox => "mandelbox",
            Backdrop::Kaliset => "kaliset",
            Backdrop::Quaternion => "quaternion julia",
            Backdrop::Quasicrystal => "quasicrystal",
        }
    }

    /// What this backdrop needs this frame that is the same for every sample.
    /// `variant` picks among a fractal's forms: a tiling, a scale, a kit.
    pub fn prepare(self, p: &Palette, g: &Geometry, t: f32, variant: u32) -> Prepared {
        let mut prepared = Prepared {
            t,
            fractal: Fractal::new(self, g, t, variant),
            julia: (0.0, 0.0),
            balls: [Ball::default(); 6],
            aurora: [[(0.0, 0.0); 3]; MAX_W],
        };
        match self {
            Backdrop::Julia => prepared.julia = boundary_walk(t),
            Backdrop::Metaballs => {
                for (k, ball) in prepared.balls.iter_mut().enumerate() {
                    let fk = k as f32;
                    *ball = Ball {
                        x: sinf(t * 0.21 * (1.0 + fk * 0.13) + fk * 1.7) * 0.42 * g.aspect,
                        y: cosf(t * 0.17 * (1.0 + fk * 0.11) + fk * 2.3) * 0.36,
                        r2: {
                            let r = 0.09 + 0.035 * sinf(fk * 3.1 + 1.0);
                            r * r
                        },
                        colour: p.sample(fk / 6.0 + t * 0.01),
                    };
                }
            }
            Backdrop::Aurora => {
                // Everything but the height above the hem depends on the column alone.
                for (px, column) in prepared.aurora.iter_mut().enumerate().take(g.w as usize) {
                    let u = (px as f32 + 0.5) / g.w as f32 * g.aspect;
                    *column = core::array::from_fn(|k| {
                        let fk = k as f32;
                        let fold = fbm(u * 1.2 + fk * 3.1, t * 0.04 + fk) - 0.5;
                        let hem = 0.42
                            + fk * 0.1
                            + fold * 0.25
                            + 0.03 * sinf(u * 4.0 + t * 0.3 + fk * 2.0);
                        let noise = value_noise(u * 38.0 + fold * 30.0, t * 0.35 + fk * 7.0);
                        (hem, 0.45 + 0.55 * noise * sqrtf(noise))
                    });
                }
            }
            Backdrop::Vignette
            | Backdrop::Stars
            | Backdrop::Grid
            | Backdrop::Rays
            | Backdrop::Kaleidoscope
            | Backdrop::Tunnel
            | Backdrop::Warp
            | Backdrop::Mandelbrot
            | Backdrop::Ship
            | Backdrop::Newton
            | Backdrop::Apollonian
            | Backdrop::Hyperbolic
            | Backdrop::Domain
            | Backdrop::Glass
            | Backdrop::Mandala
            | Backdrop::Mandelbox
            | Backdrop::Kaliset
            | Backdrop::Quaternion
            | Backdrop::Quasicrystal => {}
        }
        prepared
    }

    /// The background at sample `pt`.
    pub fn paint(self, p: &Palette, pt: &Point, f: &Prepared) -> Rgb {
        let t = f.t;
        let y = pt.y;
        let base = p.background.scale(pt.vignette);
        match self {
            Backdrop::Vignette => base,
            Backdrop::Stars => {
                let seed = pt.stars;
                if seed > 0.02 {
                    return base;
                }
                let twinkle = sinf((t * (0.5 + seed * 90.0) + seed * 300.0) * TAU) * 0.5 + 0.5;
                base.add(
                    WHITE
                        .lerp(p.sample(seed * 30.0), 0.4)
                        .scale(twinkle * twinkle * twinkle * 0.75),
                )
            }
            Backdrop::Grid => {
                let horizon = 0.58;
                let h = (y - horizon) * 14.0;
                let glow = p.sample(0.15).scale(0.35 * expf(-h * h));
                if y < horizon {
                    let k = y / horizon;
                    return base.add(p.sample(0.75).scale(0.12 * k * k)).add(glow);
                }
                let depth = 1.0 / (y - horizon + 0.02);
                let z = depth * 0.6 + t * 1.2;
                let lane = pt.qx * depth * 0.9;
                let fade = powf((y - horizon) / (1.0 - horizon), 0.6);
                // Wider lines than the GPU's: a sample here is a quarter cell.
                let grid = line(z, 0.3).max(line(lane, 0.2 * depth.min(10.0) / 4.0));
                base.add(p.sample(0.0).scale(grid * 0.45 * fade)).add(glow)
            }
            Backdrop::Aurora => {
                let columns = f.aurora.get(pt.px as usize).copied().unwrap_or_default();
                base.scale(0.6).add(aurora(p, &columns, y, t))
            }
            Backdrop::Rays => {
                let beam = (wrap(pt.rays * 14.0 + t * 0.04) - 0.5).abs() * 2.0;
                let falloff = (1.0 - pt.d * 1.4).max(0.0);
                base.add(p.sample(0.3).scale(beam * beam * falloff * 0.12))
            }
            Backdrop::Julia => julia(p, pt, f.julia, t).scale(0.7),
            Backdrop::Mandelbrot
            | Backdrop::Ship
            | Backdrop::Newton
            | Backdrop::Apollonian
            | Backdrop::Hyperbolic
            | Backdrop::Domain
            | Backdrop::Glass
            | Backdrop::Mandala
            | Backdrop::Mandelbox
            | Backdrop::Kaliset
            | Backdrop::Quaternion
            | Backdrop::Quasicrystal => f.fractal.paint(p, pt, t),
            Backdrop::Kaleidoscope => kaleidoscope(p, pt, t).scale(0.65),
            Backdrop::Tunnel => {
                let r = pt.r.max(1e-3);
                let u = pt.angle / PI + t * 0.05;
                let v = 0.3 / r + t * 0.6;
                let checker = if ((floorf(u * 8.0) + floorf(v * 4.0)) as i32) & 1 == 0 {
                    1.0
                } else {
                    0.65
                };
                let n = fbm(u * 6.0, v * 2.0);
                p.sample(v * 0.15 + n * 0.3)
                    .scale(checker * smoothstep(0.0, 0.35, r) * 0.9 * 0.5)
            }
            Backdrop::Warp => {
                let (r, turn) = (pt.r, pt.angle / TAU + 0.5);
                let mut light = p.background;
                for layer in 0..3u32 {
                    let sectors = 60.0 + layer as f32 * 35.0;
                    let at = turn * sectors;
                    let sector = floorf(at);
                    let seed = hash(sector as u32 + layer * 977, 3);
                    let depth = wrap(seed * 7.0 + t * (0.25 + seed * 0.5));
                    let star = depth * depth * 0.9;
                    let trail = 0.02 + depth * 0.08;
                    let along = 1.0 - smoothstep(0.0, trail, (r - star).abs());
                    if along <= 0.0 {
                        continue;
                    }
                    let across = (at - sector - 0.5).abs();
                    let core = 1.0 - smoothstep(0.05, 0.5, across);
                    light = light.add(p.sample(seed).lerp(WHITE, 0.5).scale(along * core * depth));
                }
                light
            }
            Backdrop::Metaballs => {
                let mut field = 0.0;
                let mut tint = BLACK;
                for ball in &f.balls {
                    let (ex, ey) = (pt.qx - ball.x, pt.qy - ball.y);
                    let k = ball.r2 / (ex * ex + ey * ey).max(1e-5);
                    field += k;
                    tint = tint.add(ball.colour.scale(k));
                }
                let tint = tint.scale(1.0 / field.max(1e-4));
                let inside = smoothstep(0.9, 1.1, field);
                let e = (field - 1.0) * 4.0;
                let rim = expf(-e * e);
                p.background
                    .scale(0.7)
                    .add(tint.scale(inside * 0.45 + rim * 0.7 + field.min(1.0) * 0.08))
                    .scale(0.75)
            }
        }
    }
}

/// A sample's place on the screen, and what follows from it alone.
#[derive(Clone, Copy, Default)]
pub struct Point {
    /// 0..1 across the screen from the top left.
    pub x: f32,
    pub y: f32,
    /// The sample.
    pub px: u32,
    pub py: u32,
    /// From the centre, in screen heights.
    pub qx: f32,
    pub qy: f32,
    /// `|(qx, qy)|` and its angle.
    pub r: f32,
    pub angle: f32,
    /// Distance from the centre in screen fractions, and the angle the sun
    /// rays use, in turns.
    pub d: f32,
    pub rays: f32,
    pub stars: f32,
    pub vignette: f32,
}

/// What is costly to work out for a sample and never changes.
#[derive(Clone, Copy, Default)]
pub struct Polar {
    r: f32,
    angle: f32,
    d: f32,
    rays: f32,
}

impl Polar {
    pub const ZERO: Polar = Polar {
        r: 0.0,
        angle: 0.0,
        d: 0.0,
        rays: 0.0,
    };
}

/// The fixed geometry of one grid size, over a table the caller keeps.
pub struct Geometry<'a> {
    pub w: u32,
    pub h: u32,
    /// The screen's physical width over its height.
    pub aspect: f32,
    polar: &'a [Polar],
}

impl<'a> Geometry<'a> {
    /// Fills `polar` for a `w` x `h` grid, row by row.
    pub fn fill(polar: &mut [Polar], w: u32, h: u32, aspect: f32) {
        let cells = (0..h).flat_map(|py| (0..w).map(move |px| (px, py)));
        for (slot, (px, py)) in polar.iter_mut().zip(cells) {
            let (dx, dy) = (
                (px as f32 + 0.5) / w as f32 - 0.5,
                (py as f32 + 0.5) / h as f32 - 0.5,
            );
            let (qx, qy) = (dx * aspect, dy);
            *slot = Polar {
                r: sqrtf(qx * qx + qy * qy),
                angle: atan2f(qy, qx),
                d: sqrtf(dx * dx + dy * dy),
                rays: atan2f(dy / aspect.max(0.1), dx) / TAU,
            };
        }
    }

    /// The geometry of a grid whose table `fill` filled.
    pub fn new(polar: &'a [Polar], w: u32, h: u32, aspect: f32) -> Self {
        Self {
            w,
            h,
            aspect,
            polar,
        }
    }

    /// Sample `(px, py)`.
    pub fn point(&self, px: u32, py: u32) -> Point {
        let polar = self
            .polar
            .get((py * self.w + px) as usize)
            .copied()
            .unwrap_or_default();
        let (x, y) = (
            (px as f32 + 0.5) / self.w as f32,
            (py as f32 + 0.5) / self.h as f32,
        );
        let (dx, dy) = (x - 0.5, y - 0.5);
        Point {
            x,
            y,
            px,
            py,
            qx: dx * self.aspect,
            qy: dy,
            r: polar.r,
            angle: polar.angle,
            d: polar.d,
            rays: polar.rays,
            stars: hash(px, py),
            vignette: 1.6 - 1.8 * (dx * dx + dy * dy),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Ball {
    x: f32,
    y: f32,
    r2: f32,
    colour: Rgb,
}

/// A backdrop's per-frame constants.
pub struct Prepared {
    pub t: f32,
    fractal: Fractal,
    julia: (f32, f32),
    balls: [Ball; 6],
    /// Per column: each curtain's hem and ray brightness.
    aurora: [[(f32, f32); 3]; MAX_W],
}

fn line(v: f32, w: f32) -> f32 {
    (1.0 - (wrap(v) - 0.5).abs() * 2.0 / w).max(0.0)
}

fn aurora(p: &Palette, columns: &[(f32, f32); 3], y: f32, t: f32) -> Rgb {
    let mut light = BLACK;
    for (k, &(hem, rays)) in columns.iter().enumerate() {
        let fk = k as f32;
        let above = hem - y;
        if above < -0.02 {
            continue;
        }
        let height = 0.22 + 0.1 * fk;
        let curtain = smoothstep(-0.02, 0.01, above) * expf(-above.max(0.0) / height);
        let colour = p.sample(0.15 + fk * 0.18 + above.max(0.0) * 1.4 + t * 0.01);
        light = light.add(colour.scale(curtain * rays * (0.55 - fk * 0.12)));
    }
    light
}

/// A point walking the Mandelbrot set's boundary, so the Julia sets for it
/// stay connected and change continuously.
fn boundary_walk(t: f32) -> (f32, f32) {
    let lap = wrap(t / 160.0) * 4.0;
    if (1.5..2.5).contains(&lap) {
        let phi = (lap - 1.5) * TAU;
        let seam = expf(-sq(phi.min(TAU - phi)) * 20.0);
        let r = 0.25 * (0.95 + 0.05 * seam);
        return (-1.0 + cosf(phi) * r, sinf(phi) * r);
    }
    let theta = (if lap >= 2.5 { lap - 1.0 } else { lap }) / 3.0 * TAU;
    let (wx, wy) = (cosf(theta) * 0.5, sinf(theta) * 0.5);
    let seam = expf(-sq(theta - PI) * 20.0);
    let k = 0.985 + 0.015 * seam;
    // w - w²
    ((wx - (wx * wx - wy * wy)) * k, (wy - 2.0 * wx * wy) * k)
}

fn julia(p: &Palette, pt: &Point, (cx, cy): (f32, f32), t: f32) -> Rgb {
    const LIMIT: f32 = 48.0;
    let (mut zx, mut zy) = (pt.qx * 2.1, pt.qy * 2.1);
    let mut i = 0.0;
    while i < LIMIT {
        if zx * zx + zy * zy > 16.0 {
            break;
        }
        (zx, zy) = (zx * zx - zy * zy + cx, 2.0 * zx * zy + cy);
        i += 1.0;
    }
    if i >= LIMIT {
        return p.background.scale(0.6);
    }
    let mu = i + 1.0 - log2f(log2f((zx * zx + zy * zy).max(1.0001)) * 0.5 + 1e-6);
    let shade = (log2f(mu + 1.0) / log2f(LIMIT)).clamp(0.0, 1.0);
    p.sample(log2f(mu + 1.0) * 0.3 + t * 0.03)
        .scale(0.12 + 0.88 * shade)
}

fn kaleidoscope(p: &Palette, pt: &Point, t: f32) -> Rgb {
    let segments = 8.0;
    let r = pt.r;
    let wedge = TAU / segments;
    let a = pt.angle + t * 0.05;
    let a = (a - floorf(a / wedge) * wedge - wedge * 0.5).abs();
    let k = r * 3.2 - t * 0.12;
    let (sx, sy) = (cosf(a) * k, sinf(a) * k);
    let n = fbm(sx + t * 0.03, sy + 1.7) + 0.35 * fbm(sx * 2.1 - t * 0.05, sy * 2.1);
    let bands = n * 6.0;
    let piece = floorf(bands);
    let leading = smoothstep(0.0, 0.12, (wrap(bands) - 0.5).abs() * 2.0 - 0.75);
    let glass = p
        .sample(piece * 0.17 + t * 0.015)
        .scale(0.55 + 0.45 * wrap(piece * 0.618));
    let mirror = expf(-(a - wedge * 0.5).abs() * r * 140.0) * 0.5;
    glass
        .scale((1.0 - leading * 0.85) * (1.15 - r * 0.9))
        .add(p.sample(0.5).scale(mirror))
}
