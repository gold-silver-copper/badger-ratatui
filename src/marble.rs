//! A marble that rolls with the badge's tilt, sparks from the touch pads, and
//! a few live gauges, drawn with ratatui.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Points};
use ratatui::widgets::{Block, BorderType, LineGauge, Paragraph, Sparkline};

use crate::sensors::{self, Reading};

/// Pixels of a font cell, to keep the canvas world square.
const CELL_W: f32 = 6.0;
const CELL_H: f32 = 13.0;
const RADIUS: f32 = 7.0;
const GRAVITY: f32 = 600.0;
/// How hard the arrow buttons push the marble, px/s².
const PUSH: f32 = 700.0;
/// Speed Enter adds to the marble, px/s.
const KICK: f32 = 260.0;
const TRAIL: usize = 24;
const SHAKE: usize = 64;
const PAD_COLORS: [Color; 8] = [
    Color::Red,
    Color::Yellow,
    Color::Green,
    Color::Cyan,
    Color::Blue,
    Color::Magenta,
    Color::LightRed,
    Color::LightGreen,
];

struct Spark {
    pos: (f32, f32),
    vel: (f32, f32),
    life: f32,
    color: Color,
}

pub struct Marble {
    world: (f32, f32),
    pos: (f32, f32),
    vel: (f32, f32),
    trail: VecDeque<(f64, f64)>,
    sparks: Vec<Spark>,
    shake: VecDeque<u64>,
    input: Reading,
    prev_touch: u8,
    prev_switches: u8,
    bumps: u32,
    rng: u32,
    fps: f32,
    uptime_s: u32,
}

impl Marble {
    pub fn new() -> Self {
        Self {
            world: (180.0, 120.0),
            pos: (90.0, 60.0),
            vel: (60.0, 0.0),
            trail: VecDeque::with_capacity(TRAIL),
            sparks: Vec::new(),
            shake: VecDeque::with_capacity(SHAKE),
            input: Reading::default(),
            prev_touch: 0,
            prev_switches: 0,
            bumps: 0,
            rng: 0x1234_5678,
            fps: 0.0,
            uptime_s: 0,
        }
    }

    pub fn update(&mut self, input: Reading, dt: f32, uptime_ms: u64) {
        self.input = input;
        self.uptime_s = u32::try_from(uptime_ms / 1000).unwrap_or(u32::MAX);
        if dt > 0.0 {
            self.fps += (1.0 / dt - self.fps) * 0.1;
        }

        // Gravity projected onto the screen: screen right is the IMU's -y,
        // screen up its -x.
        let [ax, ay, az] = input.accel;
        let g = (-ay * GRAVITY, -ax * GRAVITY);
        let held = |bit: u8| if input.switches & bit != 0 { PUSH } else { 0.0 };
        // Left and right switch screens; up and down push the marble.
        let push = (0.0, held(sensors::UP) - held(sensors::DOWN));
        self.vel.0 += (g.0 + push.0) * dt;
        self.vel.1 += (g.1 + push.1) * dt;
        let pressed = input.switches & !self.prev_switches;
        self.prev_switches = input.switches;
        if pressed & sensors::ENTER != 0 {
            self.kick();
        }
        let drag = 1.0 - 0.4 * dt;
        self.vel = (self.vel.0 * drag, self.vel.1 * drag);
        self.pos.0 += self.vel.0 * dt;
        self.pos.1 += self.vel.1 * dt;
        self.bounce();

        let fresh = input.touch & !self.prev_touch;
        self.prev_touch = input.touch;
        for (pad, color) in PAD_COLORS.into_iter().enumerate() {
            if fresh & (1 << pad) != 0 {
                self.burst(24, color, 220.0);
            }
        }

        for s in &mut self.sparks {
            s.vel.0 += g.0 * 0.5 * dt;
            s.vel.1 += g.1 * 0.5 * dt;
            s.pos.0 += s.vel.0 * dt;
            s.pos.1 += s.vel.1 * dt;
            s.life -= dt;
        }
        let world = self.world;
        self.sparks.retain(|s| {
            s.life > 0.0 && (0.0..=world.0).contains(&s.pos.0) && (0.0..=world.1).contains(&s.pos.1)
        });

        if self.trail.len() == TRAIL {
            self.trail.pop_front();
        }
        self.trail
            .push_back((f64::from(self.pos.0), f64::from(self.pos.1)));

        // How far from a calm 1 g the badge is being thrown about.
        let mag2 = ax * ax + ay * ay + az * az;
        let shake = ((mag2 - 1.0).abs() * 100.0).min(400.0);
        if self.shake.len() == SHAKE {
            self.shake.pop_front();
        }
        self.shake.push_back(shake as u64);
    }

    fn bounce(&mut self) {
        let (w, h) = self.world;
        let hits = [
            (self.pos.0 < RADIUS, 0),
            (self.pos.0 > w - RADIUS, 1),
            (self.pos.1 < RADIUS, 2),
            (self.pos.1 > h - RADIUS, 3),
        ];
        for (hit, wall) in hits {
            if !hit {
                continue;
            }
            let speed = if wall < 2 {
                self.vel.0.abs()
            } else {
                self.vel.1.abs()
            };
            match wall {
                0 => {
                    self.pos.0 = RADIUS;
                    self.vel.0 = self.vel.0.abs() * 0.7;
                }
                1 => {
                    self.pos.0 = w - RADIUS;
                    self.vel.0 = -self.vel.0.abs() * 0.7;
                }
                2 => {
                    self.pos.1 = RADIUS;
                    self.vel.1 = self.vel.1.abs() * 0.7;
                }
                _ => {
                    self.pos.1 = h - RADIUS;
                    self.vel.1 = -self.vel.1.abs() * 0.7;
                }
            }
            if speed > 60.0 {
                self.bumps += 1;
                let n = (speed / 20.0).min(16.0) as usize;
                self.burst(n, Color::White, speed * 0.6);
            }
        }
    }

    /// Enter: knock the marble the way it is already going (up, when still).
    fn kick(&mut self) {
        let speed = libm::sqrtf(self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1);
        let dir = if speed > 1.0 {
            (self.vel.0 / speed, self.vel.1 / speed)
        } else {
            (0.0, 1.0)
        };
        self.vel.0 += dir.0 * KICK;
        self.vel.1 += dir.1 * KICK;
        self.burst(32, Color::LightYellow, 260.0);
    }

    fn burst(&mut self, n: usize, color: Color, speed: f32) {
        for _ in 0..n {
            let dx = self.random() * 2.0 - 1.0;
            let dy = self.random() * 2.0 - 1.0;
            let v = speed * (0.3 + 0.7 * self.random());
            let life = 0.6 + self.random();
            self.sparks.push(Spark {
                pos: self.pos,
                vel: (dx * v, dy * v),
                life,
                color,
            });
        }
    }

    /// Uniform in 0..1, from xorshift32.
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let [header, main, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(4),
        ])
        .areas(f.area());
        let [arena, side] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).areas(main);
        let [pads, imu, stats] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Fill(1),
        ])
        .areas(side);

        self.draw_header(f, header);
        self.draw_arena(f, arena);
        self.draw_pads(f, pads);
        self.draw_imu(f, imu);
        self.draw_stats(f, stats);
        self.draw_shake(f, footer);
    }

    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let (h, m, s) = (
            self.uptime_s / 3600,
            self.uptime_s / 60 % 60,
            self.uptime_s % 60,
        );
        let title = Line::from(vec![
            Span::raw(" rig ")
                .bold()
                .fg(Color::Black)
                .bg(Color::LightMagenta),
            Span::raw(" ♥ ").fg(Color::LightRed),
            Span::raw(" badger 2350 ")
                .bold()
                .fg(Color::Black)
                .bg(Color::LightCyan),
            Span::raw("  ratatui · embassy · pio+dma").fg(Color::Gray),
        ]);
        let info = Line::from(vec![
            Span::raw(format!(" {:4.1} fps", self.fps)).fg(Color::LightGreen),
            Span::raw(format!("   up {h:02}:{m:02}:{s:02}")).fg(Color::LightYellow),
            Span::raw("   tilt/▲▼ roll · ↵ kick").fg(Color::DarkGray),
        ]);
        f.render_widget(Paragraph::new(vec![Line::raw(""), title, info]), area);
    }

    fn draw_arena(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightMagenta))
            .title(Line::from(" marble ").bold());
        let inner = block.inner(area);
        self.world = (
            f32::from(inner.width) * CELL_W,
            f32::from(inner.height) * CELL_H,
        );
        self.pos = (
            self.pos.0.clamp(0.0, self.world.0),
            self.pos.1.clamp(0.0, self.world.1),
        );

        let canvas = Canvas::default()
            .block(block)
            .marker(Marker::Braille)
            .x_bounds([0.0, f64::from(self.world.0)])
            .y_bounds([0.0, f64::from(self.world.1)])
            .paint(|ctx| {
                let trail: Vec<(f64, f64)> = self.trail.iter().copied().collect();
                ctx.draw(&Points {
                    coords: &trail,
                    color: Color::DarkGray,
                });
                for s in &self.sparks {
                    ctx.draw(&Points {
                        coords: &[(f64::from(s.pos.0), f64::from(s.pos.1))],
                        color: s.color,
                    });
                }
                ctx.draw(&Circle {
                    x: f64::from(self.pos.0),
                    y: f64::from(self.pos.1),
                    radius: f64::from(RADIUS),
                    color: Color::LightYellow,
                });
                ctx.draw(&Circle {
                    x: f64::from(self.pos.0),
                    y: f64::from(self.pos.1),
                    radius: f64::from(RADIUS) * 0.5,
                    color: Color::Yellow,
                });
            });
        f.render_widget(canvas, area);
    }

    fn draw_pads(&self, f: &mut Frame, area: Rect) {
        let pad = |i: usize, bit: u8, color: Color| {
            let on = self.input.touch & bit != 0;
            let style = if on {
                Style::new()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(color)
            };
            Span::styled(format!(" {} ", i + 1), style)
        };
        let pads: Vec<Span> = PAD_COLORS
            .into_iter()
            .enumerate()
            .map(|(i, c)| pad(i, 1 << i, c))
            .collect();
        let names = [
            ("◀", sensors::LEFT),
            ("▶", sensors::RIGHT),
            ("↵", sensors::ENTER),
            ("▲", sensors::UP),
            ("▼", sensors::DOWN),
        ];
        let switches: Vec<Span> = names
            .into_iter()
            .map(|(n, bit)| {
                if self.input.switches & bit != 0 {
                    Span::raw(format!(" {n} "))
                        .fg(Color::Black)
                        .bg(Color::White)
                } else {
                    Span::raw(format!(" {n} ")).fg(Color::Gray)
                }
            })
            .collect();
        let title = if self.input.cap_ok {
            " touch "
        } else {
            " touch (no CAP1208!) "
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightCyan))
            .title(Line::from(title).bold());
        f.render_widget(
            Paragraph::new(vec![Line::from(pads), Line::from(switches)]).block(block),
            area,
        );
    }

    fn draw_imu(&self, f: &mut Frame, area: Rect) {
        let title = if self.input.imu_ok {
            " tilt (g) "
        } else {
            " tilt (no IMU!) "
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightGreen))
            .title(Line::from(title).bold());
        let inner = block.inner(area);
        f.render_widget(block, area);
        let rows = Layout::vertical([Constraint::Length(1); 3]).split(inner);
        let axes = [
            ("x", Color::LightRed),
            ("y", Color::LightGreen),
            ("z", Color::LightBlue),
        ];
        for ((row, (name, color)), a) in rows.iter().zip(axes).zip(self.input.accel) {
            let gauge = LineGauge::default()
                .ratio(f64::from(((a + 1.0) / 2.0).clamp(0.0, 1.0)))
                .label(format!("{name}{a:+.2}"))
                .filled_style(Style::new().fg(color))
                .unfilled_style(Style::new().fg(Color::DarkGray));
            f.render_widget(gauge, *row);
        }
    }

    fn draw_stats(&self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightYellow))
            .title(Line::from(" stats ").bold());
        let speed = libm::sqrtf(self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1);
        let lines = vec![
            Line::from(vec![
                Span::raw("bumps  ").fg(Color::Gray),
                Span::raw(format!("{}", self.bumps)).bold(),
            ]),
            Line::from(vec![
                Span::raw("speed  ").fg(Color::Gray),
                Span::raw(format!("{speed:.0} px/s")).bold(),
            ]),
            Line::from(vec![
                Span::raw("sparks ").fg(Color::Gray),
                Span::raw(format!("{}", self.sparks.len())).bold(),
            ]),
        ];
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_shake(&self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightBlue))
            .title(Line::from(" shake it ").bold());
        let data: Vec<u64> = self.shake.iter().copied().collect();
        let spark = Sparkline::default()
            .block(block)
            .data(&data)
            .max(150)
            .style(Style::new().fg(Color::LightMagenta));
        f.render_widget(spark, area);
    }
}
