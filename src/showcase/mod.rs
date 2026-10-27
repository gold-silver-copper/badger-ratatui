//! A small port of ratatui-showcase (github.com/gold-silver-copper/ratatui-showcase)
//! to the badge's CPU: the ratatui art over animated backgrounds, painted by
//! ink shaders in the showcase's themes, cut to the tempo with sub-cell
//! wipes, and encoded into quadrant block glyphs, two colours per cell.

mod art;
mod fast;
mod paint;

use core::f32::consts::TAU;
use core::fmt::Write as _;

use self::fast::{floorf, roundf, sinf, sqrtf};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use self::art::{Bitmap, Material, QUADRANTS, SUBJECTS, Subject};
use self::paint::{
    BACKDROPS, BLACK, Backdrop, Geometry, Palette, Point, Polar, Prepared, Rgb, SHADERS, Shader,
    WHITE, hash, smoothstep, wrap,
};
use crate::input::Reading;

/// The largest grid drawn: the badge's 53x18 cells, each 2x2 samples.
/// A larger area is drawn in its top-left corner.
pub const MAX_COLS: u16 = 53;
pub const MAX_ROWS: u16 = 18;
pub const MAX_W: usize = MAX_COLS as usize * 2;
pub const MAX_H: usize = MAX_ROWS as usize * 2;
const SAMPLES: usize = MAX_W * MAX_H;

/// The showcase's working memory, kept out of the heap: on the badge a
/// static, so frame after frame nothing big is allocated or freed.
pub struct Buffers {
    samples: [Rgb; SAMPLES],
    polar: [Polar; SAMPLES],
    /// When the wipe reaches each sample.
    wipe_at: [f32; SAMPLES],
}

impl Buffers {
    pub const fn new() -> Self {
        Self {
            samples: [BLACK; SAMPLES],
            polar: [Polar::ZERO; SAMPLES],
            wipe_at: [0.0; SAMPLES],
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

/// A scene's caption, held inline.
struct Caption {
    bytes: [u8; 96],
    len: usize,
}

impl Caption {
    fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

impl core::fmt::Write for Caption {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // Whole characters only, so the text stays valid UTF-8.
        let room = self.bytes.len() - self.len;
        if s.len() > room {
            return Err(core::fmt::Error);
        }
        if let Some(dst) = self.bytes.get_mut(self.len..self.len + s.len()) {
            dst.copy_from_slice(s.as_bytes());
            self.len += s.len();
        }
        Ok(())
    }
}

/// A sample is a quarter of a 6x13 px cell: 3 px wide, 6.5 px tall.
const SAMPLE_ASPECT: f32 = 6.5 / 3.0;
/// 120 bpm, four bars a scene, as the showcase holds by default.
const BEAT: f32 = 0.5;
const HOLD: f32 = 16.0 * BEAT;
const TRANSITION: f32 = 2.0 * BEAT;
const CAPTION: f32 = 3.0;

/// wyrand: small, fast, good enough for picking scenes.
pub struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0xa076_1d64_78bd_642f);
        let t = u128::from(self.0) * u128::from(self.0 ^ 0xe703_7ed1_a0b4_28db);
        (t as u64) ^ ((t >> 64) as u64)
    }

    pub fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn pick<T: Copy>(&mut self, items: &[T], fallback: T) -> T {
        items
            .get(self.below(items.len()))
            .copied()
            .unwrap_or(fallback)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Solid,
    Shadow,
    Outline,
    Neon,
}

const STYLES: &[Style] = &[
    Style::Solid,
    Style::Solid,
    Style::Shadow,
    Style::Outline,
    Style::Neon,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Still,
    Float,
    Wave,
    Breathe,
    Sway,
}

const MOTIONS: &[Motion] = &[
    Motion::Still,
    Motion::Float,
    Motion::Wave,
    Motion::Breathe,
    Motion::Sway,
];

#[derive(Clone, Copy)]
enum Wipe {
    Radial,
    Diamond,
    Clock,
    Diagonal,
    Blinds,
    Checker,
    Dissolve,
    Spiral,
}

const WIPES: &[Wipe] = &[
    Wipe::Radial,
    Wipe::Diamond,
    Wipe::Clock,
    Wipe::Diagonal,
    Wipe::Blinds,
    Wipe::Checker,
    Wipe::Dissolve,
    Wipe::Spiral,
];

impl Wipe {
    /// When, from 0 to 1 through the transition, the new scene reaches `pt`.
    fn at(self, g: &Geometry, pt: &Point) -> f32 {
        // From the centre, as a fraction of the way to a corner.
        let r = pt.r / (0.5 * sqrtf(g.aspect * g.aspect + 1.0));
        match self {
            Wipe::Radial => r,
            Wipe::Diamond => (pt.qx.abs() + pt.qy.abs()) / (0.5 * g.aspect + 0.5),
            Wipe::Clock => wrap(pt.angle / TAU + 0.25),
            Wipe::Diagonal => (pt.x + pt.y) * 0.5,
            Wipe::Blinds => wrap(pt.x * 6.0),
            Wipe::Checker => {
                let odd = ((pt.x * 8.0) as u32 + (pt.y * 4.0) as u32) % 2;
                odd as f32 * 0.5 + pt.x * 0.5
            }
            Wipe::Dissolve => hash(pt.px, pt.py),
            Wipe::Spiral => wrap(r * 1.5 + pt.angle / TAU) * 0.8 + r * 0.2,
        }
    }
}

#[derive(Clone)]
struct Scene {
    palette: Palette,
    backdrop: Backdrop,
    shader: Shader,
    subject: Subject,
    style: Style,
    motion: Motion,
}

impl Scene {
    fn random(rng: &mut Rng) -> Self {
        Self {
            palette: Palette::random(rng),
            backdrop: rng.pick(BACKDROPS, Backdrop::Vignette),
            shader: rng.pick(SHADERS, Shader::Flow),
            subject: rng.pick(SUBJECTS, Subject::Mascot),
            style: rng.pick(STYLES, Style::Solid),
            motion: rng.pick(MOTIONS, Motion::Still),
        }
    }

    /// The next cut: one or two layers change, as in the showcase's runs.
    fn varied(&self, rng: &mut Rng) -> Self {
        let mut next = self.clone();
        for _ in 0..1 + rng.below(2) {
            match rng.below(6) {
                0 => next.palette = Palette::random(rng),
                1 => next.backdrop = rng.pick(BACKDROPS, Backdrop::Vignette),
                2 => next.shader = rng.pick(SHADERS, Shader::Flow),
                3 => next.subject = rng.pick(SUBJECTS, Subject::Mascot),
                4 => next.style = rng.pick(STYLES, Style::Solid),
                _ => next.motion = rng.pick(MOTIONS, Motion::Still),
            }
        }
        next
    }

    fn caption(&self) -> Caption {
        let mut c = Caption {
            bytes: [0; 96],
            len: 0,
        };
        let _ = match self.palette.detail {
            Some(hue) => write!(c, " {} {hue}°", self.palette.name),
            None => write!(c, " {}", self.palette.name),
        };
        let _ = write!(
            c,
            " · {} · {} · {} ",
            self.backdrop.name(),
            self.shader.name(),
            self.subject.name()
        );
        c
    }
}

/// A scene's art placed on the grid for this frame.
struct Placed<'a> {
    art: &'a Bitmap,
    cx: f32,
    cy: f32,
    sx: f32,
    sy: f32,
    /// The wave motion's vertical offset for each sample column, from one
    /// left of the screen to one right of it; all 0 when the art doesn't wave.
    lift: [f32; MAX_W + 2],
}

impl Placed<'_> {
    fn new<'a>(art: &'a Bitmap, motion: Motion, g: &Geometry, t: f32) -> Placed<'a> {
        let (w, h) = (art.width.max(1) as f32, art.height.max(1) as f32);
        let ratio = art.aspect / SAMPLE_ASPECT;
        let (box_w, box_h) = (g.w as f32 * 0.88, g.h as f32 * 0.86);
        let fit = (box_w / w).min(box_h / (h * ratio));
        // Whole-number scales keep every pixel of the art the same size.
        let (mut sx, mut sy) = if fit >= 2.0 {
            let sx = floorf(fit);
            (sx, roundf(sx * ratio).max(1.0))
        } else {
            (fit, fit * ratio)
        };
        let (mut cx, mut cy) = (g.w as f32 * 0.5, g.h as f32 * 0.5);
        let mut wave = 0.0;
        let bar = 4.0 * BEAT;
        match motion {
            Motion::Still => {}
            Motion::Float => cy += g.h as f32 * 0.05 * sinf(t * TAU / (2.0 * bar)),
            Motion::Sway => cx += g.w as f32 * 0.04 * sinf(t * TAU / (2.0 * bar)),
            Motion::Breathe => {
                let k = 1.0 + 0.05 * sinf(t * TAU / bar);
                sx *= k;
                sy *= k;
            }
            Motion::Wave => wave = 1.2,
        }
        let mut lift = [0.0; MAX_W + 2];
        if wave != 0.0 {
            for (slot, x) in lift.iter_mut().zip(-1..=g.w as i32) {
                let ax = (x as f32 + 0.5 - cx) / sx + w * 0.5;
                *slot = wave * sinf(ax * 0.35 - t * TAU / (4.0 * BEAT));
            }
        }
        Placed {
            art,
            cx,
            cy,
            sx,
            sy,
            lift,
        }
    }

    /// The art pixel under sample `(x, y)`, and where it lies in the art (0..1).
    fn at(&self, x: i32, y: i32) -> (Material, f32, f32) {
        let (w, h) = (self.art.width as f32, self.art.height as f32);
        let ax = (x as f32 + 0.5 - self.cx) / self.sx + w * 0.5;
        let lift = usize::try_from(x + 1)
            .ok()
            .and_then(|i| self.lift.get(i))
            .copied()
            .unwrap_or(0.0);
        let ay = (y as f32 + 0.5 - self.cy - lift) / self.sy + h * 0.5;
        if ax < 0.0 || ay < 0.0 {
            return (Material::Empty, 0.0, 0.0);
        }
        (self.art.get(ax as i32, ay as i32), ax / w, ay / h)
    }

    fn ink(&self, x: i32, y: i32) -> bool {
        self.at(x, y).0 != Material::Empty
    }

    fn rim(&self, x: i32, y: i32) -> bool {
        !(self.ink(x - 1, y) && self.ink(x + 1, y) && self.ink(x, y - 1) && self.ink(x, y + 1))
    }

    fn near(&self, x: i32, y: i32) -> bool {
        self.ink(x - 1, y) || self.ink(x + 1, y) || self.ink(x, y - 1) || self.ink(x, y + 1)
    }
}

pub struct Showcase {
    rng: Rng,
    t: f32,
    scene: Scene,
    art: Bitmap,
    old: Option<(Scene, Bitmap)>,
    wipe: Wipe,
    cut_at: f32,
    caption: Caption,
    buffers: &'static mut Buffers,
    /// The grid size and aspect `buffers.polar` holds the geometry of.
    grid: Option<(u32, u32, f32)>,
    /// Whether `buffers.wipe_at` holds this transition's wipe.
    wipe_ready: bool,
    prev_switches: u8,
}

/// Runs two pieces of work and returns when both are done: at once, on two
/// cores, where there are two.
pub trait Cores {
    fn join(&mut self, a: &mut (dyn FnMut() + Send), b: &mut (dyn FnMut() + Send));
}

/// One core: one piece of work after the other.
pub struct OneCore;

impl Cores for OneCore {
    fn join(&mut self, a: &mut (dyn FnMut() + Send), b: &mut (dyn FnMut() + Send)) {
        a();
        b();
    }
}

impl Showcase {
    pub fn new(seed: u64, buffers: &'static mut Buffers) -> Self {
        let mut rng = Rng(seed);
        let scene = Scene {
            palette: Palette::theme(0),
            backdrop: Backdrop::Stars,
            shader: Shader::Sheen,
            subject: Subject::Mascot,
            style: Style::Solid,
            motion: Motion::Float,
        };
        let _ = rng.next();
        Self {
            art: scene.subject.bitmap(),
            caption: scene.caption(),
            scene,
            rng,
            t: 0.0,
            old: None,
            wipe: Wipe::Radial,
            cut_at: 0.0,
            buffers,
            grid: None,
            wipe_ready: false,
            prev_switches: 0,
        }
    }

    pub fn update(&mut self, input: &Reading, dt: f32) {
        use crate::input::{DOWN, ENTER, UP};
        self.t += dt;
        let pressed = input.switches & !self.prev_switches;
        self.prev_switches = input.switches;
        if pressed & ENTER != 0 {
            let next = self.scene.varied(&mut self.rng);
            self.cut(next);
        } else if pressed & UP != 0 {
            let mut next = self.scene.clone();
            next.palette = next.palette.next_theme();
            self.cut(next);
        } else if pressed & DOWN != 0 {
            let mut next = self.scene.clone();
            let i = BACKDROPS
                .iter()
                .position(|&b| b == next.backdrop)
                .map_or(0, |i| i + 1);
            next.backdrop = BACKDROPS
                .get(i % BACKDROPS.len())
                .copied()
                .unwrap_or(Backdrop::Vignette);
            self.cut(next);
        } else if self.t - self.cut_at >= HOLD {
            // Runs of a few related scenes, then a fresh start.
            let next = if self.rng.below(4) == 0 {
                Scene::random(&mut self.rng)
            } else {
                self.scene.varied(&mut self.rng)
            };
            self.cut(next);
        }
        if self.old.is_some() && self.t - self.cut_at >= TRANSITION {
            self.old = None;
        }
    }

    fn cut(&mut self, next: Scene) {
        let art = next.subject.bitmap();
        let old_scene = core::mem::replace(&mut self.scene, next);
        let old_art = core::mem::replace(&mut self.art, art);
        self.old = Some((old_scene, old_art));
        self.wipe = self.rng.pick(WIPES, Wipe::Radial);
        self.cut_at = self.t;
        self.caption = self.scene.caption();
        self.wipe_ready = false;
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, cores: &mut dyn Cores) {
        let area = Rect {
            width: area.width.min(MAX_COLS),
            height: area.height.min(MAX_ROWS),
            ..area
        };
        if area.is_empty() {
            return;
        }
        let (w, h) = (u32::from(area.width) * 2, u32::from(area.height) * 2);
        let aspect = (f32::from(area.width) * 6.0) / (f32::from(area.height) * 13.0);
        let Buffers {
            samples,
            polar,
            wipe_at,
        } = &mut *self.buffers;
        if self.grid != Some((w, h, aspect)) {
            Geometry::fill(polar, w, h, aspect);
            self.grid = Some((w, h, aspect));
            self.wipe_ready = false;
        }
        let g = &Geometry::new(polar, w, h, aspect);
        let t = self.t;
        if self.old.is_some() && !self.wipe_ready {
            let cells = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
            for (slot, (x, y)) in wipe_at.iter_mut().zip(cells) {
                *slot = self.wipe.at(g, &g.point(x, y));
            }
            self.wipe_ready = true;
        }
        let new = Layer::new(&self.scene, &self.art, g, t);
        let old = self
            .old
            .as_ref()
            .map(|(scene, art)| Layer::new(scene, art, g, t));
        let frame = &FrameJob {
            g,
            new,
            old,
            wipe_at: &wipe_at[..],
            progress: smoothstep(0.0, 1.0, (t - self.cut_at) / TRANSITION),
            t,
        };

        let width = w as usize;
        let samples = samples.get_mut(..width * h as usize).unwrap_or(&mut []);
        // Even rows on one core, odd rows on the other: the art in the middle
        // and the busy parts of a background fall to both alike.
        let (paired, rest) = samples.split_at_mut((h as usize / 2) * 2 * width);
        let mut even: [Option<&mut [Rgb]>; MAX_H / 2] = [const { None }; MAX_H / 2];
        let mut odd: [Option<&mut [Rgb]>; MAX_H / 2] = [const { None }; MAX_H / 2];
        for ((pair, e), o) in paired
            .chunks_exact_mut(2 * width)
            .zip(&mut even)
            .zip(&mut odd)
        {
            let (a, b) = pair.split_at_mut(width);
            (*e, *o) = (Some(a), Some(b));
        }
        cores.join(
            &mut || {
                for (i, row) in even.iter_mut().enumerate() {
                    if let Some(row) = row {
                        frame.row(2 * i, row);
                    }
                }
            },
            &mut || {
                for (i, row) in odd.iter_mut().enumerate() {
                    if let Some(row) = row {
                        frame.row(2 * i + 1, row);
                    }
                }
            },
        );
        if !rest.is_empty() {
            frame.row(h as usize - 1, rest);
        }

        encode(samples, width, area, buf);

        if t - self.cut_at < CAPTION {
            let y = area.bottom().saturating_sub(1);
            for (i, ch) in self.caption.as_str().chars().enumerate() {
                let Ok(dx) = u16::try_from(i) else { break };
                let x = area.left().saturating_add(dx);
                if x >= area.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch)
                        .set_fg(WHITE.scale(0.9).color())
                        .set_bg(self.scene.palette.background.scale(0.6).color());
                }
            }
        }
    }
}

/// A scene ready to draw this frame.
struct Layer<'a> {
    scene: &'a Scene,
    art: Placed<'a>,
    backdrop: Prepared,
}

impl<'a> Layer<'a> {
    fn new(scene: &'a Scene, art: &'a Bitmap, g: &Geometry, t: f32) -> Self {
        Self {
            scene,
            art: Placed::new(art, scene.motion, g, t),
            backdrop: scene.backdrop.prepare(&scene.palette, g, t),
        }
    }

    /// One sample: the art, over the background where it shows through.
    fn sample(&self, pt: &Point, t: f32) -> Rgb {
        let (scene, art) = (self.scene, &self.art);
        let p = &scene.palette;
        let bg = || scene.backdrop.paint(p, pt, &self.backdrop);
        let (xi, yi) = (pt.px as i32, pt.py as i32);
        let (m, u, v) = art.at(xi, yi);
        let ink = |m: Material| {
            let c = scene.shader.shade(p, u, v, t, pt.py);
            match m {
                Material::Hat => c.lerp(WHITE, 0.75),
                Material::Eye => {
                    if t - floorf(t / 5.0) * 5.0 < 0.15 {
                        p.accent
                    } else {
                        p.background.scale(2.5)
                    }
                }
                Material::Screen => p.background.lerp(c, 0.1),
                Material::Border => c.scale(0.55),
                Material::Cursor => {
                    if wrap(t * 1.2) < 0.5 {
                        p.accent
                    } else {
                        p.background.lerp(c, 0.1)
                    }
                }
                Material::Ink | Material::Empty => c,
            }
        };
        let inked = m != Material::Empty;
        match scene.style {
            // A halo keeps the art readable over a busy background.
            Style::Solid | Style::Shadow if inked => ink(m),
            Style::Solid if art.near(xi, yi) => bg().scale(0.45),
            Style::Shadow if art.ink(xi - 2, yi - 1) => bg().scale(0.25),
            Style::Outline if inked && art.rim(xi, yi) => ink(m),
            Style::Outline if inked => bg().scale(0.5).lerp(ink(m), 0.15),
            Style::Neon if inked && art.rim(xi, yi) => ink(m).lerp(WHITE, 0.55),
            Style::Neon if inked => bg().scale(0.35).add(ink(m).scale(0.2)),
            Style::Neon if art.near(xi, yi) => bg().scale(0.6).add(ink(Material::Ink).scale(0.45)),
            Style::Solid | Style::Shadow | Style::Outline | Style::Neon => bg(),
        }
    }
}

/// Everything a frame's shading reads, shared by both cores.
struct FrameJob<'a> {
    g: &'a Geometry<'a>,
    new: Layer<'a>,
    old: Option<Layer<'a>>,
    /// When the wipe reaches each sample.
    wipe_at: &'a [f32],
    progress: f32,
    t: f32,
}

impl FrameJob<'_> {
    fn row(&self, y: usize, out: &mut [Rgb]) {
        let start = y * self.g.w as usize;
        let wipe = self.wipe_at.get(start..start + out.len()).unwrap_or(&[]);
        for (i, colour) in out.iter_mut().enumerate() {
            let pt = &self.g.point(i as u32, y as u32);
            *colour = match &self.old {
                Some(old) => {
                    let at = wipe.get(i).copied().unwrap_or(0.0);
                    if at > self.progress {
                        old.sample(pt, self.t)
                    } else if self.progress - at < 0.05 {
                        self.new.scene.palette.accent
                    } else {
                        self.new.sample(pt, self.t)
                    }
                }
                None => self.new.sample(pt, self.t),
            };
        }
    }
}

/// Each cell's four samples split into the two colour groups that fit them
/// best; the split's shape picks the quadrant glyph.
fn encode(samples: &[Rgb], w: usize, area: Rect, buf: &mut Buffer) {
    for row in 0..area.height {
        for col in 0..area.width {
            let (sx, sy) = (usize::from(col) * 2, usize::from(row) * 2);
            let at = |dx: usize, dy: usize| {
                samples
                    .get((sy + dy) * w + sx + dx)
                    .copied()
                    .unwrap_or_default()
            };
            // Bit order of `QUADRANTS`: top-left 8, top-right 4, bottom-left 2, bottom-right 1.
            let quad = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
            let bits = [8usize, 4, 2, 1];
            let mut best = (f32::MAX, 0usize, Rgb::default(), Rgb::default());
            // Masks 0..8 leave the top-left in the background group: every split once.
            for mask in 0..8usize {
                let (mut fg, mut bg, mut nf, mut nb) = (Rgb::default(), Rgb::default(), 0.0, 0.0);
                for (c, bit) in quad.iter().zip(bits) {
                    if mask & bit != 0 {
                        fg = fg.add(*c);
                        nf += 1.0;
                    } else {
                        bg = bg.add(*c);
                        nb += 1.0;
                    }
                }
                let fg = if nf > 0.0 { fg.scale(1.0 / nf) } else { fg };
                let bg = if nb > 0.0 { bg.scale(1.0 / nb) } else { bg };
                let err: f32 = quad
                    .iter()
                    .zip(bits)
                    .map(|(c, bit)| c.dist2(if mask & bit != 0 { fg } else { bg }))
                    .sum();
                if err < best.0 - 1e-6 {
                    best = (err, mask, fg, bg);
                }
            }
            let (_, mask, fg, bg) = best;
            let glyph = QUADRANTS.get(mask).copied().unwrap_or(' ');
            if let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) {
                cell.set_char(glyph).set_fg(fg.color()).set_bg(bg.color());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{DOWN, ENTER, UP};
    use alloc::boxed::Box;

    fn showcase(seed: u64) -> Showcase {
        Showcase::new(seed, Box::leak(Box::new(Buffers::new())))
    }

    /// The samples the last frame drew.
    fn drawn(show: &Showcase) -> &[Rgb] {
        let (w, h, _) = show.grid.expect("drawn");
        &show.buffers.samples[..(w * h) as usize]
    }

    #[test]
    fn controls_act_once_per_press_and_the_show_advances_on_time() {
        let mut show = showcase(42);
        let mut input = Reading {
            switches: UP,
            ..Reading::default()
        };
        show.update(&input, 0.05);
        assert_eq!(show.scene.palette.name, "Aurora");
        show.update(&input, 0.05);
        assert_eq!(show.scene.palette.name, "Aurora");
        input.switches = 0;
        show.update(&input, 0.05);
        input.switches = DOWN;
        show.update(&input, 0.05);
        assert!(show.scene.backdrop == Backdrop::Grid);
        input.switches = 0;
        show.update(&input, 0.05);
        input.switches = ENTER;
        show.update(&input, 0.05);
        let cut_at = show.cut_at;
        input.switches = 0;
        show.update(&input, HOLD);
        assert!(show.cut_at > cut_at);
    }

    #[test]
    fn all_ported_effects_render_finite_colours_and_transition() {
        let mut show = showcase(42);
        let area = Rect::new(0, 0, 53, 18);
        let mut buf = Buffer::empty(area);
        for (index, shader) in SHADERS.iter().copied().enumerate() {
            let mut scene = Scene::random(&mut show.rng);
            scene.shader = shader;
            scene.backdrop = BACKDROPS
                .get(index % BACKDROPS.len())
                .copied()
                .expect("backdrop");
            show.cut(scene);
            for phase in [0.0, TRANSITION * 0.5, TRANSITION + 0.1] {
                show.update(&Reading::default(), phase);
                show.render(area, &mut buf, &mut OneCore);
                assert_eq!(drawn(&show).len(), 53 * 18 * 4);
                assert!(
                    drawn(&show)
                        .iter()
                        .all(|Rgb(r, g, b)| r.is_finite() && g.is_finite() && b.is_finite())
                );
            }
        }
        assert!(buf.content.iter().any(|c| c.symbol() != " "));
    }

    #[test]
    fn quadrant_encoder_preserves_a_two_colour_split() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        let black = Rgb::default();
        encode(&[WHITE, WHITE, black, black], 2, area, &mut buf);
        let c = buf.cell((0, 0)).expect("cell");
        // Equivalent to a white upper half, encoded as a black lower half
        // on white (the encoder fixes the first sample in the bg group).
        assert_eq!(c.symbol(), "▄");
        assert_eq!(c.fg, black.color());
        assert_eq!(c.bg, WHITE.color());
    }

    /// Runs the second piece of work first, as another core might.
    struct Reversed;

    impl Cores for Reversed {
        fn join(&mut self, a: &mut (dyn FnMut() + Send), b: &mut (dyn FnMut() + Send)) {
            b();
            a();
        }
    }

    #[test]
    fn rows_split_between_cores_match_one_core_and_an_odd_height() {
        for rows in [18u16, 7] {
            let area = Rect::new(0, 0, 20, rows);
            let (mut one, mut two) = (showcase(7), showcase(7));
            let mut scene = Scene::random(&mut Rng(3));
            scene.backdrop = Backdrop::Aurora;
            scene.motion = Motion::Wave;
            for show in [&mut one, &mut two] {
                show.cut(scene.clone());
                show.update(&Reading::default(), TRANSITION * 0.5);
            }
            let mut buf = Buffer::empty(area);
            one.render(area, &mut buf, &mut OneCore);
            two.render(area, &mut buf, &mut Reversed);
            assert_eq!(drawn(&one).len(), usize::from(rows) * 2 * 40);
            assert!(drawn(&one) == drawn(&two));
        }
    }
}
