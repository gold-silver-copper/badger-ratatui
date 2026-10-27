//! A ratatui backend for small RGB565 panels.
//!
//! It owns a framebuffer already in the panel's byte order and draws each
//! changed cell as 6x13 two-colour pixels from cached glyph bitmasks, then
//! hands the rows that changed to a [`Panel`] to send, which may do so in the
//! background (DMA) while the next frame is computed.

mod glyphs;

use core::convert::Infallible;
use core::ops::Range;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};

use self::glyphs::{Glyph, Glyphs};
pub use self::glyphs::{H as CELL_H, W as CELL_W};

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
/// Words in the framebuffer: two pixels each.
pub const WORDS: usize = WIDTH * HEIGHT / 2;

/// Where the panel's pixels go.
pub trait Panel {
    /// Returns once the panel no longer reads the framebuffer.
    fn wait(&mut self);
    /// Starts sending full-width pixel rows `rows` of `framebuffer`; it may
    /// still be reading it when this returns, until `wait`.
    fn send(&mut self, framebuffer: &[u32; WORDS], rows: Range<usize>);
}

/// The colours `Color::Reset` and the named colours stand for.
pub struct Theme {
    pub foreground: u32,
    pub background: u32,
    /// Black, red, green, yellow, blue, magenta, cyan, gray, dark gray,
    /// light red, green, yellow, blue, magenta, cyan, white (ANSI 0..15).
    pub ansi: [u32; 16],
}

/// Tokyo Night, as mousefood's `ColorTheme::tokyo_night`.
pub const TOKYO_NIGHT: Theme = Theme {
    foreground: 0xa9b1d6,
    background: 0x1a1b26,
    ansi: [
        0x414868, 0xf7768e, 0x73daca, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5, 0x414868,
        0xf7768e, 0x73daca, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
    ],
};

pub struct LcdBackend<P: Panel> {
    panel: P,
    framebuffer: &'static mut [u32; WORDS],
    glyphs: Glyphs,
    theme: Theme,
    cols: u16,
    rows: u16,
    /// The grid's top-left pixel; `left` is even so cells start on a word.
    left: usize,
    top: usize,
    /// Pixel rows changed since the last flush.
    dirty: Option<Range<usize>>,
    cursor: Position,
}

impl<P: Panel> LcdBackend<P> {
    pub fn new(panel: P, framebuffer: &'static mut [u32; WORDS], theme: Theme) -> Self {
        let cols = WIDTH / CELL_W;
        let rows = HEIGHT / CELL_H;
        let mut backend = Self {
            panel,
            framebuffer,
            glyphs: Glyphs::new(),
            theme,
            cols: cols as u16,
            rows: rows as u16,
            left: ((WIDTH - cols * CELL_W) / 2) & !1,
            top: (HEIGHT - rows * CELL_H) / 2,
            dirty: None,
            cursor: Position::ORIGIN,
        };
        backend.fill_all();
        backend
    }

    pub fn panel_mut(&mut self) -> &mut P {
        &mut self.panel
    }

    fn fill_all(&mut self) {
        self.panel.wait();
        let bg = pair(rgb565(self.theme.background), rgb565(self.theme.background));
        self.framebuffer.fill(bg);
        self.dirty = Some(0..HEIGHT);
    }

    /// The panel colour of `color`, swapped into the panel's byte order.
    fn resolve(&self, color: Color, foreground: bool) -> u16 {
        let ansi = |i: usize| self.theme.ansi.get(i).copied().unwrap_or(0);
        let rgb = match color {
            Color::Reset if foreground => self.theme.foreground,
            Color::Reset => self.theme.background,
            Color::Black => ansi(0),
            Color::Red => ansi(1),
            Color::Green => ansi(2),
            Color::Yellow => ansi(3),
            Color::Blue => ansi(4),
            Color::Magenta => ansi(5),
            Color::Cyan => ansi(6),
            Color::Gray => ansi(7),
            Color::DarkGray => ansi(8),
            Color::LightRed => ansi(9),
            Color::LightGreen => ansi(10),
            Color::LightYellow => ansi(11),
            Color::LightBlue => ansi(12),
            Color::LightMagenta => ansi(13),
            Color::LightCyan => ansi(14),
            Color::White => ansi(15),
            Color::Rgb(r, g, b) => u32::from_be_bytes([0, r, g, b]),
            Color::Indexed(i) => indexed(i, ansi),
        };
        rgb565(rgb)
    }

    fn draw_cell(&mut self, col: u16, row: u16, cell: &Cell) {
        if col >= self.cols || row >= self.rows {
            return;
        }
        let modifier = cell.modifier;
        let mut fg = self.resolve(cell.fg, true);
        let mut bg = self.resolve(cell.bg, false);
        if modifier.contains(Modifier::REVERSED) {
            core::mem::swap(&mut fg, &mut bg);
        }
        if modifier.contains(Modifier::HIDDEN) {
            fg = bg;
        } else if modifier.contains(Modifier::DIM) {
            fg = dim(fg);
        }
        let c = cell.symbol().chars().next().unwrap_or(' ');
        let mut glyph: Glyph = self.glyphs.get(c, modifier.contains(Modifier::BOLD));
        if modifier.contains(Modifier::UNDERLINED)
            && let Some(r) = glyph.get_mut(CELL_H - 2)
        {
            *r = 0b11_1111;
        }
        if modifier.contains(Modifier::CROSSED_OUT)
            && let Some(r) = glyph.get_mut(CELL_H / 2)
        {
            *r = 0b11_1111;
        }

        // Two pixels per word: each pair of glyph bits picks one of four.
        let pairs = [pair(bg, bg), pair(fg, bg), pair(bg, fg), pair(fg, fg)];
        let x = self.left + usize::from(col) * CELL_W;
        let y = self.top + usize::from(row) * CELL_H;
        for (dy, bits) in glyph.iter().enumerate() {
            let start = ((y + dy) * WIDTH + x) / 2;
            if let Some(words) = self.framebuffer.get_mut(start..start + CELL_W / 2) {
                for (i, word) in words.iter_mut().enumerate() {
                    let two = usize::from(bits >> (i * 2)) & 3;
                    *word = pairs.get(two).copied().unwrap_or(0);
                }
            }
        }
        let rows = y..y + CELL_H;
        self.dirty = Some(match self.dirty.take() {
            Some(d) => d.start.min(rows.start)..d.end.max(rows.end),
            None => rows,
        });
    }

    fn clear_cells(&mut self, cells: impl Iterator<Item = (u16, u16)>) {
        let blank = Cell::default();
        self.panel.wait();
        for (col, row) in cells {
            self.draw_cell(col, row, &blank);
        }
    }
}

/// Two pixels in one word, each byte-swapped, so the bytes in memory are in
/// the order the panel takes them.
fn pair(left: u16, right: u16) -> u32 {
    u32::from(left.swap_bytes()) | (u32::from(right.swap_bytes()) << 16)
}

fn rgb565(rgb: u32) -> u16 {
    let [_, r, g, b] = rgb.to_be_bytes();
    ((u16::from(r) >> 3) << 11) | ((u16::from(g) >> 2) << 5) | (u16::from(b) >> 3)
}

/// Half brightness.
fn dim(c: u16) -> u16 {
    // Each channel halved: red and blue 4 bits, green 5.
    (c >> 1) & 0x7bef
}

/// xterm's 256 colours.
fn indexed(i: u8, ansi: impl Fn(usize) -> u32) -> u32 {
    match i {
        0..=15 => ansi(usize::from(i)),
        16..=231 => {
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * u32::from(v) };
            let i = i - 16;
            (level(i / 36) << 16) | (level(i / 6 % 6) << 8) | level(i % 6)
        }
        _ => {
            let v = 8 + 10 * u32::from(i - 232);
            (v << 16) | (v << 8) | v
        }
    }
}

impl<P: Panel> Backend for LcdBackend<P> {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Infallible>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut content = content.peekable();
        if content.peek().is_none() {
            return Ok(());
        }
        // The previous frame may still be on its way to the panel.
        self.panel.wait();
        for (col, row, cell) in content {
            self.draw_cell(col, row, cell);
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Infallible> {
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Infallible> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Infallible> {
        Ok(self.cursor)
    }

    fn set_cursor_position<Pos: Into<Position>>(
        &mut self,
        position: Pos,
    ) -> Result<(), Infallible> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Infallible> {
        self.fill_all();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Infallible> {
        let (cols, rows) = (self.cols, self.rows);
        let Position { x, y } = self.cursor;
        match clear_type {
            ClearType::All => self.fill_all(),
            ClearType::AfterCursor => self.clear_cells(
                (y..rows)
                    .flat_map(|r| (0..cols).map(move |c| (c, r)))
                    .filter(|&(c, r)| r > y || c >= x),
            ),
            ClearType::BeforeCursor => self.clear_cells(
                (0..=y)
                    .flat_map(|r| (0..cols).map(move |c| (c, r)))
                    .filter(|&(c, r)| r < y || c <= x),
            ),
            ClearType::CurrentLine => self.clear_cells((0..cols).map(|c| (c, y))),
            ClearType::UntilNewLine => self.clear_cells((x..cols).map(|c| (c, y))),
        }
        Ok(())
    }

    fn size(&self) -> Result<Size, Infallible> {
        Ok(Size::new(self.cols, self.rows))
    }

    fn window_size(&mut self) -> Result<WindowSize, Infallible> {
        Ok(WindowSize {
            columns_rows: Size::new(self.cols, self.rows),
            pixels: Size::new(WIDTH as u16, HEIGHT as u16),
        })
    }

    fn flush(&mut self) -> Result<(), Infallible> {
        if let Some(rows) = self.dirty.take() {
            self.panel.send(self.framebuffer, rows);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    use ratatui::Terminal;
    use ratatui::style::Stylize;
    use ratatui::widgets::Paragraph;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        sends: Vec<Range<usize>>,
        waits: usize,
    }

    impl Panel for &mut Recorder {
        fn wait(&mut self) {
            self.waits += 1;
        }
        fn send(&mut self, _: &[u32; WORDS], rows: Range<usize>) {
            self.sends.push(rows);
        }
    }

    fn framebuffer() -> &'static mut [u32; WORDS] {
        Box::leak(Box::new([0; WORDS]))
    }

    fn pixel(fb: &[u32; WORDS], x: usize, y: usize) -> u16 {
        let word = fb.get((y * WIDTH + x) / 2).copied().unwrap_or(0);
        let half = if x % 2 == 0 {
            word as u16
        } else {
            (word >> 16) as u16
        };
        half.swap_bytes()
    }

    #[test]
    fn draws_changed_cells_and_sends_only_their_rows() {
        let mut recorder = Recorder::default();
        let backend = LcdBackend::new(&mut recorder, framebuffer(), TOKYO_NIGHT);
        let mut terminal = Terminal::new(backend).expect("terminal");
        assert_eq!(terminal.size().expect("size"), Size::new(53, 18));
        terminal
            .draw(|f| f.render_widget(Paragraph::new("x"), f.area()))
            .expect("draw");
        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new("█")
                        .fg(Color::Rgb(255, 0, 0))
                        .bg(Color::Blue),
                    f.area(),
                )
            })
            .expect("draw");
        let fb = &terminal.backend().framebuffer;
        let (left, top) = (terminal.backend().left, terminal.backend().top);
        assert_eq!(pixel(fb, left, top), 0xf800);
        assert_eq!(pixel(fb, left + 5, top + 12), 0xf800);
        // The paragraph's background fills its area: the next cell is blue.
        assert_eq!(pixel(fb, left + 6, top), rgb565(TOKYO_NIGHT.ansi[4]));
        // Only the first cell changes.
        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new("\u{2580}")
                        .fg(Color::Rgb(255, 0, 0))
                        .bg(Color::Blue),
                    f.area(),
                )
            })
            .expect("draw");
        drop(terminal);
        // The first frame sent the whole screen, the second every row (the
        // background changed everywhere), the third just the first text row.
        assert_eq!(recorder.sends.first(), Some(&(0..HEIGHT)));
        assert_eq!(recorder.sends.len(), 3);
        assert_eq!(recorder.sends.last(), Some(&(top..top + CELL_H)));
        assert!(recorder.waits > 0);
    }

    #[test]
    fn colours_convert_and_dim() {
        assert_eq!(rgb565(0xffffff), 0xffff);
        assert_eq!(rgb565(0x00ff00), 0x07e0);
        assert_eq!(dim(0xffff), 0x7bef);
        assert_eq!(indexed(196, |_| 0), 0xff0000);
        assert_eq!(indexed(232, |_| 0), 0x080808);
        assert_eq!(pair(0x1234, 0xabcd).to_le_bytes(), [0x12, 0x34, 0xab, 0xcd]);
    }
}
