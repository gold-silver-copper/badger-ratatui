//! Fractal backdrops, as 2D fields cheap enough for the badge. From
//! ratatui-showcase's `backdrop.wgsl`: the Mandelbrot zoom, the Burning
//! Ship, Newton's method, the Apollonian gasket, a hyperbolic tiling, domain
//! colouring and stained glass. From hyper-surf's sky (`sky.wgsl`) and world
//! fields (`sdf.wgsl`, `mega/`): the mandala, and slices through the
//! Mandelbox, the Kaliset, a quaternion Julia set and a quasicrystal.
//!
//! Whatever is the same for every sample is worked out once a frame, in
//! `Fractal::new`; `paint` is the per-sample part.

use core::f32::consts::{LN_2, PI, TAU};

use super::fast::{atan2f, cosf, expf, floorf, log2f, roundf, sinf, sqrtf};
use super::paint::{Backdrop, Geometry, Palette, Point, Rgb, hash, smoothstep, wrap};

/// Where the Mandelbrot zoom heads: seahorse valley.
const SEAHORSE: (f32, f32) = (-0.743_643_9, 0.131_825_9);
/// The deepest the zoom goes, in e-folds: past this the escape counts near
/// the set outgrow the iteration budget.
const MANDELBROT_DEPTH: f32 = 5.0;
/// Hyperbolic {p, q} tilings, as in the showcase.
const TILINGS: [(f32, f32); 5] = [(7.0, 3.0), (5.0, 4.0), (4.0, 5.0), (8.0, 3.0), (6.0, 4.0)];
/// Mandelbox scales: hyper-surf's Halls (positive) and Citadel (negative).
const MANDELBOX: [f32; 5] = [2.8, -2.5, 3.2, -2.8, 2.6];
/// hyper-surf's Julia Reef constants.
const QUATERNION: [[f32; 4]; 6] = [
    [-0.2, 0.6, 0.2, 0.2],
    [-0.45, 0.45, 0.2, 0.0],
    [-0.137, -0.63, -0.338, -0.18],
    [-0.291, -0.399, 0.339, 0.437],
    [-0.2, 0.4, -0.4, -0.4],
    [0.185, 0.478, 0.125, -0.392],
];
/// The stained glass's sites move by a table of this many phases.
const PHASES: usize = 32;
const WAVES: usize = 7;

/// hyper-surf's Kaliset kits: dunes, strands and badlands.
#[derive(Clone, Copy)]
pub struct Kit {
    scale: f32,
    shift: [f32; 3],
    least: f32,
    iterations: u32,
    remains: Remains,
}

/// What is left of the fold's last point, whose distance draws the surface.
#[derive(Clone, Copy)]
enum Remains {
    Sheets,
    Filaments,
    Masses,
}

const KITS: [Kit; 3] = [
    Kit {
        scale: 2.0,
        shift: [0.62, 0.38, 0.5],
        least: 0.25,
        iterations: 7,
        remains: Remains::Sheets,
    },
    Kit {
        scale: 1.3,
        shift: [0.5, 0.6, 0.3],
        least: 0.2,
        iterations: 9,
        remains: Remains::Filaments,
    },
    Kit {
        scale: 1.9,
        shift: [0.7, 0.45, 0.55],
        least: 0.3,
        iterations: 6,
        remains: Remains::Masses,
    },
];

/// A turn, as its cosine and sine.
#[derive(Clone, Copy, Default)]
pub struct Turn(f32, f32);

impl Turn {
    fn by(angle: f32) -> Self {
        Turn(cosf(angle), sinf(angle))
    }

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (self.0 * x - self.1 * y, self.1 * x + self.0 * y)
    }
}

/// A fractal backdrop's per-frame constants.
#[derive(Clone, Copy)]
pub enum Fractal {
    None,
    Mandelbrot {
        scale: f32,
        limit: u32,
        pixel: f32,
    },
    Ship {
        scale: f32,
    },
    Newton {
        turn: Turn,
        relax: f32,
    },
    Apollonian {
        view: f32,
        k: f32,
        pixel: f32,
    },
    Hyperbolic {
        mirror: (f32, f32),
        centre: f32,
        r2: f32,
        glide: (f32, f32),
        turn: Turn,
    },
    Domain {
        c: (f32, f32),
    },
    Glass {
        phases: [f32; PHASES],
    },
    Mandala {
        zoom: f32,
        sector: f32,
        drift: f32,
        offset: (f32, f32),
        turn: Turn,
        grow: f32,
        per_unit: f32,
    },
    Mandelbox {
        s: f32,
        view: f32,
        z: f32,
        pixel: f32,
    },
    Kaliset {
        kit: Kit,
        view: f32,
        at: (f32, f32),
        twist: Turn,
        pixel: f32,
    },
    Quaternion {
        c: [f32; 4],
        view: f32,
        w: f32,
        turn: Turn,
        pixel: f32,
    },
    Quasicrystal {
        waves: [(f32, f32); WAVES],
        phase: f32,
    },
}

impl Fractal {
    pub fn new(backdrop: Backdrop, g: &Geometry, t: f32, variant: u32) -> Self {
        let rows = g.h.max(1) as f32;
        let pick = |k: u32| hash(variant, k);
        match backdrop {
            Backdrop::Mandelbrot => {
                let depth = MANDELBROT_DEPTH * (0.5 - 0.5 * cosf(t * TAU / 120.0));
                let scale = 3.0 * expf(-depth);
                Fractal::Mandelbrot {
                    scale,
                    limit: 48 + (12.0 * depth) as u32,
                    pixel: scale / rows,
                }
            }
            Backdrop::Ship => Fractal::Ship {
                scale: 0.16 * expf(-wrap(t / 30.0) * 30.0 * 0.06),
            },
            Backdrop::Newton => Fractal::Newton {
                turn: Turn::by(t * 0.07),
                relax: 1.0 + 0.35 * sinf(t * 0.25),
            },
            Backdrop::Apollonian => {
                let zoom = expf(-(0.5 - 0.5 * cosf(t * TAU / 90.0)) * 2.5);
                let view = 1.2 * zoom;
                Fractal::Apollonian {
                    view,
                    k: 1.15 + 0.06 * sinf(t * 0.11),
                    pixel: view / rows,
                }
            }
            Backdrop::Hyperbolic => {
                let (p, q) = TILINGS
                    .get(variant as usize % TILINGS.len())
                    .copied()
                    .unwrap_or((7.0, 3.0));
                let (alpha, beta) = (PI / p, PI / q);
                let (sa, cb) = (sinf(alpha), cosf(beta));
                let r = sa / sqrtf((cb * cb - sa * sa).max(1e-6));
                let reach = 0.55 * sinf(t * 0.07);
                Fractal::Hyperbolic {
                    mirror: (-sa, cosf(alpha)),
                    centre: sqrtf(1.0 + r * r),
                    r2: r * r,
                    glide: (cosf(t * 0.03) * reach, sinf(t * 0.03) * reach),
                    turn: Turn::by(t * 0.02),
                }
            }
            Backdrop::Domain => Fractal::Domain {
                c: (cosf(t * 0.11) * 1.3, sinf(t * 0.11) * 1.3),
            },
            Backdrop::Glass => Fractal::Glass {
                phases: core::array::from_fn(|k| sinf(t * 0.25 + k as f32 * TAU / PHASES as f32)),
            },
            Backdrop::Mandala => {
                let s = [pick(1), pick(2), pick(3), pick(4)];
                let [sx, sy, sz, sw] = s;
                let zoom = 1.1 + 0.8 * sx;
                let wobble = (cosf(t * 0.08), sinf(t * 0.08));
                Fractal::Mandala {
                    zoom,
                    sector: TAU / floorf(5.0 + sy * 7.0),
                    drift: t * 0.04,
                    offset: (
                        0.5 + 0.25 * sz + 0.04 * wobble.0,
                        0.25 + 0.2 * sw + 0.04 * wobble.1,
                    ),
                    turn: Turn::by(0.25 + 0.9 * sw + 0.1 * wobble.1),
                    grow: 1.35 + 0.2 * sx,
                    per_unit: rows / zoom,
                }
            }
            Backdrop::Mandelbox => {
                let view = 5.0;
                Fractal::Mandelbox {
                    s: MANDELBOX
                        .get(variant as usize % MANDELBOX.len())
                        .copied()
                        .unwrap_or(2.8),
                    view,
                    z: 1.4 * sinf(t * 0.05),
                    pixel: view / rows,
                }
            }
            Backdrop::Kaliset => {
                let view = 0.4;
                Fractal::Kaliset {
                    kit: KITS
                        .get(variant as usize % KITS.len())
                        .copied()
                        .unwrap_or(KITS[0]),
                    view,
                    at: (t * 0.015, 0.3 * sinf(t * 0.04)),
                    twist: Turn::by(0.12),
                    pixel: view / rows,
                }
            }
            Backdrop::Quaternion => {
                let view = 2.0;
                Fractal::Quaternion {
                    c: QUATERNION
                        .get(variant as usize % QUATERNION.len())
                        .copied()
                        .unwrap_or(QUATERNION[0]),
                    view,
                    w: 0.6 * sinf(t * 0.06),
                    turn: Turn::by(t * 0.02),
                    pixel: view / rows,
                }
            }
            Backdrop::Quasicrystal => {
                let k = 0.6 * rows * (1.0 + 0.3 * sinf(t * 0.05));
                Fractal::Quasicrystal {
                    waves: core::array::from_fn(|n| {
                        let a = n as f32 * PI / WAVES as f32;
                        (cosf(a) * k, sinf(a) * k)
                    }),
                    phase: t * 0.8,
                }
            }
            _ => Fractal::None,
        }
    }

    /// The backdrop at sample `pt`.
    /// Kept out of line: inlined into `Backdrop::paint`, the backdrops' code
    /// outgrew the 16 KiB flash cache, and some fractals ran three times
    /// slower for evicting the shading.
    #[inline(never)]
    pub fn paint(&self, p: &Palette, pt: &Point, t: f32) -> Rgb {
        let (qx, qy) = (pt.qx, pt.qy);
        match *self {
            Fractal::None => p.background,
            Fractal::Mandelbrot {
                scale,
                limit,
                pixel,
            } => mandelbrot(
                p,
                SEAHORSE.0 + qx * scale,
                SEAHORSE.1 + qy * scale,
                limit,
                pixel,
                t,
            ),
            Fractal::Ship { scale } => {
                let (cx, cy) = (-1.762 + qx * scale, 0.028 + qy * scale);
                let (mut zx, mut zy, mut m2) = (0.0f32, 0.0f32, 0.0f32);
                let limit = 64;
                let mut i = 0;
                while i < limit {
                    if m2 > 16.0 {
                        break;
                    }
                    let (ax, ay) = (zx.abs(), zy.abs());
                    (zx, zy) = (ax * ax - ay * ay + cx, -2.0 * ax * ay + cy);
                    m2 = zx * zx + zy * zy;
                    i += 1;
                }
                if i >= limit && m2 <= 16.0 {
                    return p.background.scale(0.6);
                }
                escape_colour(p, smooth_escape(i as f32, m2), limit as f32, t)
            }
            Fractal::Newton { turn, relax } => {
                const LIMIT: f32 = 24.0;
                let (mut zx, mut zy) = turn.apply(qx * 2.4, qy * 2.4);
                let mut n = 0.0;
                while n < LIMIT {
                    // z -= relax (z^3 - 1) / (3 z^2)
                    let (z2x, z2y) = (zx * zx - zy * zy, 2.0 * zx * zy);
                    let (fx, fy) = (z2x * zx - z2y * zy - 1.0, z2x * zy + z2y * zx);
                    let (dx, dy) = (3.0 * z2x, 3.0 * z2y);
                    let inv = relax / (dx * dx + dy * dy).max(1e-9);
                    zx -= (fx * dx + fy * dy) * inv;
                    zy -= (fy * dx - fx * dy) * inv;
                    if fx * fx + fy * fy < 1e-6 {
                        break;
                    }
                    n += 1.0;
                }
                let root = (atan2f(zy, zx) / TAU + 1.0) * 3.0;
                let shade = 1.0 - n / LIMIT;
                p.sample(floorf(root + 0.5) / 3.0 + t * 0.01)
                    .scale((0.2 + 0.8 * shade * sqrtf(shade)) * 0.7)
            }
            Fractal::Apollonian { view, k, pixel } => {
                let (mut x, mut y) = (0.3 + qx * view, 0.2 + qy * view);
                let mut scale = 1.0;
                for _ in 0..10 {
                    x = 2.0 * wrap(0.5 * x + 0.5) - 1.0;
                    y = 2.0 * wrap(0.5 * y + 0.5) - 1.0;
                    let f = k / (x * x + y * y).max(1e-5);
                    x *= f;
                    y *= f;
                    scale *= f;
                }
                let edge = expf(-0.25 * y.abs() / scale / pixel * 0.9);
                p.background
                    .scale(0.6)
                    .add(
                        p.sample(log2f(scale) * 0.07 + t * 0.015)
                            .scale(edge * 0.95 + 0.12),
                    )
                    .scale(0.7)
            }
            Fractal::Hyperbolic {
                mirror,
                centre,
                r2,
                glide,
                turn,
            } => hyperbolic(p, (qx * 1.3, qy * 1.3), mirror, centre, r2, glide, turn, t),
            Fractal::Domain { c } => {
                let (zx, zy) = (qx * 3.2, qy * 3.2);
                let (z2x, z2y) = (zx * zx - zy * zy, 2.0 * zx * zy);
                // (z^3 - c) / (z^2 + 1)
                let (nx, ny) = (z2x * zx - z2y * zy - c.0, z2x * zy + z2y * zx - c.1);
                let (dx, dy) = (z2x + 1.0, z2y);
                let inv = 1.0 / (dx * dx + dy * dy).max(1e-9);
                let (fx, fy) = ((nx * dx + ny * dy) * inv, (ny * dx - nx * dy) * inv);
                let phase = atan2f(fy, fx);
                let size = 0.5 * log2f((fx * fx + fy * fy).max(1e-12));
                let rings = 0.6 + 0.4 * smoothstep(0.0, 1.0, wrap(size));
                let x8 = {
                    let x = cosf(phase * 6.0).abs();
                    let x2 = x * x;
                    let x4 = x2 * x2;
                    x4 * x4
                };
                let x16 = x8 * x8;
                let rays = 1.0 - 0.35 * x16 * x16 * x8;
                p.sample(phase / TAU + t * 0.01)
                    .scale(rings * rays * 0.7 * 0.7)
            }
            Fractal::Glass { ref phases } => glass(p, qx * 7.0 + 40.0, qy * 7.0 + 40.0, phases, t),
            Fractal::Mandala {
                zoom,
                sector,
                drift,
                offset,
                turn,
                grow,
                per_unit,
            } => {
                let r = pt.r * zoom;
                let angle = pt.angle + drift;
                let folded = (angle - sector * roundf(angle / sector)).abs();
                let (mut x, mut y) = (r * cosf(folded), r * sinf(folded));
                let (mut scale, mut trap, mut glow, mut weight) = (1.0, 1e3f32, 0.0, 1.0);
                for _ in 0..5 {
                    (x, y) = turn.apply(x.abs() - offset.0, y.abs() - offset.1);
                    x *= grow;
                    y *= grow;
                    scale *= grow;
                    // Each fold's mirror lines, about a sample wide.
                    let line = x.abs().min(y.abs()) / scale * per_unit;
                    glow += expf(-line * 1.5) * weight;
                    weight *= 0.62;
                    trap = trap.min((sqrtf(x * x + y * y) - 0.7).abs() / scale);
                }
                let ring = expf(-trap * per_unit * 0.85);
                // The GPU also steps the tone at log rings, too fine for the badge.
                let tone = 0.9 * sqrtf(trap * 4.0);
                ink(
                    p,
                    tone + t / 75.0,
                    glow * 0.6 + ring * 0.5,
                    0.35 + 0.65 * smoothstep(0.0, 0.25, trap),
                )
            }
            Fractal::Mandelbox { s, view, z, pixel } => {
                mandelbox(p, [qx * view, qy * view, z], s, pixel, t)
            }
            Fractal::Kaliset {
                kit,
                view,
                at,
                twist,
                pixel,
            } => kaliset(
                p,
                [qx * view + at.0, qy * view, at.1],
                &kit,
                twist,
                pixel,
                t,
            ),
            Fractal::Quaternion {
                c,
                view,
                w,
                turn,
                pixel,
            } => {
                let (x, y) = turn.apply(qx * view, qy * view);
                quaternion(p, [x, y, 0.0, w], c, pixel, t)
            }
            Fractal::Quasicrystal { ref waves, phase } => {
                let sum: f32 = waves
                    .iter()
                    .map(|&(kx, ky)| cosf(kx * qx + ky * qy + phase))
                    .sum();
                let tone = 0.5 + 0.5 * sum / WAVES as f32;
                p.sample(tone * 0.8 + t * 0.02)
                    .scale((0.15 + 0.85 * tone * tone) * 0.7)
            }
        }
    }
}

/// hyper-surf's sky colouring: the body in the palette, folds and edges in
/// its complement, burning past it only when bright.
fn ink(p: &Palette, tone: f32, glow: f32, body: f32) -> Rgb {
    let base = p.sample(tone).scale(body);
    let accent = p.sample(tone + 0.5).scale(1.4);
    base.lerp(accent, glow)
        .add(accent.scale((glow - 1.0).max(0.0) * 0.4))
        .scale(0.7)
}

/// A slice through a solid: its inside by the orbit trap's tone, the empty
/// space dark but for a glow along the surface.
fn slice(p: &Palette, inside: bool, distance: f32, tone: f32, glow: f32, pixel: f32) -> Rgb {
    if inside {
        return ink(p, tone, glow, 0.85);
    }
    let rim = expf(-distance / pixel * 0.7);
    ink(p, tone, rim * 0.9, 0.1 + 0.5 * rim)
}

fn smooth_escape(i: f32, m2: f32) -> f32 {
    i + 1.0 - log2f(log2f(m2.max(1.0001)) * 0.5 + 1e-6)
}

fn escape_colour(p: &Palette, mu: f32, limit: f32, t: f32) -> Rgb {
    if mu >= limit {
        return p.background.scale(0.6);
    }
    // Most of the plane escapes in a few steps: a log scale lights the
    // bands near the set instead of leaving them dark.
    let shade = (log2f(mu + 1.0) / log2f(limit.min(48.0))).clamp(0.0, 1.0);
    p.sample(log2f(mu + 1.0) * 0.3 + t * 0.03)
        .scale((0.12 + 0.88 * shade) * 0.7)
}

/// Lit by how close the set is, in samples: its filaments glow at any depth.
fn mandelbrot(p: &Palette, cx: f32, cy: f32, limit: u32, pixel: f32, t: f32) -> Rgb {
    let inside = p.background.scale(0.6 * 0.7);
    // The main cardioid and the period-2 bulb never escape.
    let xq = cx - 0.25;
    let q = xq * xq + cy * cy;
    if q * (q + xq) < 0.25 * cy * cy || (cx + 1.0) * (cx + 1.0) + cy * cy < 0.0625 {
        return inside;
    }
    let (mut zx, mut zy, mut dx, mut dy, mut m2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut i = 0;
    while i < limit {
        // The derivative by c, for the distance to the set; held finite.
        (dx, dy) = (
            (2.0 * (zx * dx - zy * dy) + 1.0).clamp(-1e18, 1e18),
            (2.0 * (zx * dy + zy * dx)).clamp(-1e18, 1e18),
        );
        (zx, zy) = (zx * zx - zy * zy + cx, 2.0 * zx * zy + cy);
        m2 = zx * zx + zy * zy;
        if m2 > 16.0 {
            break;
        }
        i += 1;
    }
    if i >= limit {
        return inside;
    }
    let ln = 0.5 * log2f(m2) * LN_2;
    let distance = sqrtf(m2 / (dx * dx + dy * dy).max(1e-30)) * ln;
    let edge = expf(-sqrtf((distance / pixel).max(0.0)) * 0.7);
    let mu = smooth_escape(i as f32, m2);
    p.sample(log2f(mu + 1.0) * 0.3 + t * 0.03)
        .scale((0.07 + 0.93 * edge) * 0.7)
}

/// A {p, q} tiling of the Poincaré disk: each point reflected into the
/// fundamental triangle, the reflections counted, while a Möbius map glides
/// the plane along.
#[allow(clippy::too_many_arguments)]
fn hyperbolic(
    p: &Palette,
    (zx, zy): (f32, f32),
    (nx, ny): (f32, f32),
    centre: f32,
    r2: f32,
    (ax, ay): (f32, f32),
    turn: Turn,
    t: f32,
) -> Rgb {
    if zx * zx + zy * zy >= 1.0 {
        return p.background.scale(0.5 * 0.7);
    }
    // z -> (z - a) / (1 - conj(a) z)
    let (mx, my) = (zx - ax, zy - ay);
    let (dx, dy) = (1.0 - (ax * zx + ay * zy), -(ax * zy - ay * zx));
    let inv = 1.0 / (dx * dx + dy * dy).max(1e-9);
    let (mut x, mut y) = turn.apply((mx * dx + my * dy) * inv, (my * dx - mx * dy) * inv);
    let mut flips = 0u32;
    for _ in 0..40 {
        let mut moved = false;
        if y < 0.0 {
            y = -y;
            flips += 1;
            moved = true;
        }
        let d = x * nx + y * ny;
        if d > 0.0 {
            x -= 2.0 * d * nx;
            y -= 2.0 * d * ny;
            flips += 1;
            moved = true;
        }
        let (qx, qy) = (x - centre, y);
        let q2 = qx * qx + qy * qy;
        if q2 < r2 {
            let k = r2 / q2.max(1e-9);
            (x, y) = (centre + qx * k, qy * k);
            flips += 1;
            moved = true;
        }
        if !moved {
            break;
        }
    }
    let rim = (sqrtf((x - centre) * (x - centre) + y * y) - sqrtf(r2)).abs();
    let edge = y.abs().min((x * nx + y * ny).abs()).min(rim);
    let even = flips.is_multiple_of(2);
    let tile = p
        .sample(if even { 0.1 } else { 0.6 } + t * 0.01)
        .scale(if even { 0.75 } else { 0.35 });
    // Wider than the GPU's lines: a sample here is a quarter cell.
    let line = expf(-edge * 30.0);
    tile.mix(p.accent, line * 0.7)
        .scale((1.0 - 0.3 * (x * x + y * y)) * 0.7)
}

/// Cells around drifting points, each its own colour, with dark leading.
fn glass(p: &Palette, x: f32, y: f32, phases: &[f32; PHASES], t: f32) -> Rgb {
    let (gx, gy) = (floorf(x), floorf(y));
    let (fx, fy) = (x - gx, y - gy);
    let (gx, gy) = (gx.max(1.0) as u32, gy.max(1.0) as u32);
    let phase = |h: f32| {
        phases
            .get((h * PHASES as f32) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let (mut d1, mut d2, mut id) = (9.0f32, 9.0f32, (0, 0));
    for j in 0..3u32 {
        for i in 0..3u32 {
            let cell = (gx + i - 1, gy + j - 1);
            let (h1, h2) = (hash(cell.0, cell.1), hash(cell.0 + 31, cell.1 + 17));
            let sx = i as f32 - 0.5 + 0.38 * phase(h1) - fx;
            let sy = j as f32 - 0.5 + 0.38 * phase(h2) - fy;
            let d = sx * sx + sy * sy;
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = cell;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    let (d1, d2) = (sqrtf(d1), sqrtf(d2));
    let glass = p
        .sample(hash(id.0, id.1) + t * 0.01)
        .scale((0.45 + 0.5 * (1.0 - d1)) * 0.8);
    p.background
        .scale(0.25)
        .mix(glass, smoothstep(0.05, 0.2, d2 - d1))
        .scale(0.7)
}

/// hyper-surf's Mandelbox (`sdf::mandelbox`), cut at depth `z[2]`.
fn mandelbox(p: &Palette, c: [f32; 3], s: f32, pixel: f32, t: f32) -> Rgb {
    let mut z = c;
    let (mut dr, mut trap) = (1.0f32, f32::MAX);
    let mut escaped = None;
    for _ in 0..10 {
        for v in &mut z {
            *v = v.clamp(-1.0, 1.0) * 2.0 - *v;
        }
        let r2 = z.iter().map(|v| v * v).sum::<f32>();
        trap = trap.min(r2);
        let k = if r2 < 0.25 {
            4.0
        } else if r2 < 1.0 {
            1.0 / r2
        } else {
            1.0
        };
        for (v, c) in z.iter_mut().zip(c) {
            *v = *v * k * s + c;
        }
        dr = dr * k * s.abs() + 1.0;
        let m2 = z.iter().map(|v| v * v).sum::<f32>();
        if m2 > 1e4 {
            escaped = Some(sqrtf(m2) / dr.abs() * 0.5);
            break;
        }
    }
    let trap = sqrtf(trap);
    let tone = 0.2 + 0.75 * (trap * 0.5).min(1.0) + t * 0.01;
    // Hot where the orbit came closest: hyper-surf's furnace.
    let glow = (1.0 - trap).clamp(0.0, 1.0);
    slice(
        p,
        escaped.is_none(),
        escaped.unwrap_or(0.0),
        tone,
        glow,
        pixel,
    )
}

/// hyper-surf's Kaliset folds (`mega/kali.wgsl`), cut at depth `z[2]`.
fn kaliset(p: &Palette, mut z: [f32; 3], kit: &Kit, twist: Turn, pixel: f32, t: f32) -> Rgb {
    let (mut magnified, mut trap) = (1.0f32, f32::MAX);
    for _ in 0..kit.iterations {
        let [x, y, w] = z;
        let [a, b, c] = kit.shift;
        let (x, y) = twist.apply(x.abs() - a, y.abs() - b);
        let r2 = x * x + y * y + (w.abs() - c) * (w.abs() - c);
        trap = trap.min(r2);
        let k = kit.scale / r2.max(kit.least);
        z = [x * k, y * k, (w.abs() - c) * k];
        magnified *= k;
    }
    let [x, y, w] = z;
    let d = match kit.remains {
        Remains::Sheets => y.abs(),
        Remains::Filaments => sqrtf(x * x + w * w),
        Remains::Masses => sqrtf(x * x + y * y + w * w) - 2.0,
    };
    let distance = (d / magnified * 0.5).max(0.0);
    let tone = 0.2 + 0.75 * sqrtf(trap).clamp(0.0, 1.0) + t * 0.01;
    let rim = expf(-distance / pixel * 0.6);
    ink(p, tone, rim * 0.8, 0.15 + 0.7 * rim)
}

/// hyper-surf's Julia Reef (`sdf::julia4`): a quaternion Julia set, cut by
/// the plane through `z`.
fn quaternion(p: &Palette, mut z: [f32; 4], c: [f32; 4], pixel: f32, t: f32) -> Rgb {
    let mut dz2 = 1.0f32;
    let mut m2 = z.iter().map(|v| v * v).sum::<f32>();
    let mut trap = f32::MAX;
    for _ in 0..10 {
        dz2 *= 4.0 * m2;
        let [a, b, cz, d] = z;
        z = [
            a * a - b * b - cz * cz - d * d + c[0],
            2.0 * a * b + c[1],
            2.0 * a * cz + c[2],
            2.0 * a * d + c[3],
        ];
        m2 = z.iter().map(|v| v * v).sum::<f32>();
        trap = trap.min(m2);
        if m2 > 1024.0 {
            break;
        }
    }
    let trap = sqrtf(trap).min(1.0);
    let tone = 0.35 + 0.6 * trap + t * 0.01;
    let inside = m2 <= 4.0;
    let distance = 0.25 * sqrtf(m2 / dz2.max(1e-30)) * log2f(m2.max(1.0)) * LN_2;
    // The tips glow where the orbit came close.
    slice(
        p,
        inside,
        distance,
        tone,
        (1.0 - trap / 0.6).clamp(0.0, 1.0),
        pixel,
    )
}
