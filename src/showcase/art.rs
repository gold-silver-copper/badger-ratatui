//! The showcase's artwork as pixel bitmaps, from ratatui-showcase's `art.rs`:
//! the ratatui mascot and the `RatatuiLogo` wordmark from `ratatui-widgets`,
//! and the rat from ratatui's `assets/logo.svg` (all MIT, the ratatui
//! developers).

/// What a pixel is made of; the theme colours each material differently.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Empty,
    Ink,
    Hat,
    Eye,
    Screen,
    Border,
    Cursor,
}

/// Pixels the largest piece of art needs: the 50x50 rat.
const PIXELS: usize = 50 * 50;

/// Art up to `PIXELS` pixels, held inline: no heap.
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    /// A pixel's height over its width, as the artist drew it.
    pub aspect: f32,
    pixels: [Material; PIXELS],
}

impl Bitmap {
    pub fn get(&self, x: i32, y: i32) -> Material {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return Material::Empty;
        };
        if x >= self.width {
            return Material::Empty;
        }
        self.pixels
            .get(y * self.width + x)
            .copied()
            .unwrap_or(Material::Empty)
    }

    fn from_grid(grid: &str, aspect: f32, material: impl Fn(char) -> Material) -> Self {
        let width = grid.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        let height = grid.lines().count();
        let mut pixels = [Material::Empty; PIXELS];
        for (y, line) in grid.lines().enumerate() {
            for (x, c) in line.chars().enumerate() {
                if let Some(p) = pixels.get_mut(y * width + x) {
                    *p = material(c);
                }
            }
        }
        Self {
            width,
            height,
            aspect,
            pixels,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Mascot,
    Emblem,
    Wordmark,
}

pub const SUBJECTS: &[Subject] = &[Subject::Mascot, Subject::Emblem, Subject::Wordmark];

impl Subject {
    pub fn name(self) -> &'static str {
        match self {
            Subject::Mascot => "mascot",
            Subject::Emblem => "rat",
            Subject::Wordmark => "wordmark",
        }
    }

    pub fn bitmap(self) -> Bitmap {
        match self {
            Subject::Mascot => {
                Bitmap::from_grid(MASCOT.trim_start_matches('\n'), 1.0, |c| match c {
                    '█' => Material::Ink,
                    'h' => Material::Hat,
                    'e' => Material::Eye,
                    '░' => Material::Screen,
                    '▒' => Material::Border,
                    '▓' => Material::Cursor,
                    _ => Material::Empty,
                })
            }
            Subject::Emblem => Bitmap::from_grid(EMBLEM.trim_start_matches('\n'), 1.0, |c| {
                if c == '#' {
                    Material::Ink
                } else {
                    Material::Empty
                }
            }),
            Subject::Wordmark => quadrants(LOGO_SMALL),
        }
    }
}

/// Decodes quadrant-block art into pixels, two per cell each way; a
/// quadrant's pixels are about twice as tall as wide.
fn quadrants(art: &str) -> Bitmap {
    let width = art
        .lines()
        .map(|l| l.chars().count() * 2)
        .max()
        .unwrap_or(0);
    let height = art.lines().count() * 2;
    let mut pixels = [Material::Empty; PIXELS];
    for (row, line) in art.lines().enumerate() {
        for (col, c) in line.chars().enumerate() {
            let bits = quadrant_bits(c);
            for (i, bit) in [8u8, 4, 2, 1].into_iter().enumerate() {
                if bits & bit != 0
                    && let Some(p) = pixels.get_mut((row * 2 + i / 2) * width + col * 2 + i % 2)
                {
                    *p = Material::Ink;
                }
            }
        }
    }
    Bitmap {
        width,
        height,
        aspect: 2.0,
        pixels,
    }
}

/// Which quadrants a block character fills: top-left 8, top-right 4,
/// bottom-left 2, bottom-right 1.
fn quadrant_bits(c: char) -> u8 {
    QUADRANTS
        .iter()
        .position(|&q| q == c)
        .map_or(0, |i| i as u8)
}

/// The block character for each quadrant mask (see `quadrant_bits`).
pub const QUADRANTS: [char; 16] = [
    ' ', '▗', '▖', '▄', '▝', '▐', '▞', '▟', '▘', '▚', '▌', '▙', '▀', '▜', '▛', '█',
];

/// One character per pixel, drawn with half blocks, so pixels are square.
const MASCOT: &str = "
                   hhh
                 hhhhhh
                hhhhhhh
               hhhhhhhh
              hhhhhhhhh
             hhhhhhhhhh
            hhhhhhhhhhhh
            hhhhhhhhhhhhh
            hhhhhhhhhhhhh     ██████
             hhhhhhhhhhh    ████████
                  hhhhh ███████████
                   hhh ██ee████████
                    h █████████████
                ████ █████████████
               █████████████████
               ████████████████
               ████████████████
                ███ ██████████
              ▒▒    █████████
             ▒░░▒   █████████
            ▒░░░░▒ ██████████
           ▒░░▓░░░▒ █████████
          ▒░░▓▓░░░░▒ ████████
         ▒░░░░░░░░░░▒ ██████████
        ▒░░░░░░░░░░░░▒ ██████████
       ▒░░░░░░░▓▓░░░░░▒ █████████
      ▒░░░░░░░░░▓▓░░░░░▒ ████  ███
     ▒░░░░░░░░░░░░░░░░░░▒ ██   ███
    ▒░░░░░░░░░░░░░░░░░░░░▒ █   ███
    ▒░░░░░░░░░░░░░░░░░░░░░▒   ███
     ▒░░░░░░░░░░░░░░░░░░░░░▒ ███
      ▒░░░░░░░░░░░░░░░░░░░░░▒ █";

/// `assets/logo.svg` from the ratatui repo, rasterized at its native 50x50.
const EMBLEM: &str = "
                       ######
                     #########
                    ##########
                   ###########
                  ############
                 #############
                ##############
               ###############
              ################
             #################
             ##################
             ###################
             ###################
             ####################       ##########
              ###################    #############
                    ############ #################
                     ########## #################
                      ######## ##  ##############
                        ##### ##    #############
                         ### ###    #############
                            #####  #############
                     ##########################
                   ###########################
                  ##########################
                 ##########################
                 #########################
                  #######################
                ## ####################
               #### ####  #############
              ##  ##     ##############
             ##    ##    ##############
            ##      ##  ###############
           ##    #   ## ###############
          ##     ##   ## ##############
         ##      ##    ## #############
        ##    #####     ## ############
       ##      ####      ## ################
      ##                  ## ################
     ##                    ## ###############
    ##           #          ## ##############
   ##            ##          ## ######### ###
  ##              ##          ## #####    ###
 ##                #           ## ###     ###
##                              ## ##     ###
##                               ## #     ###
 ##                               ##     ####
  ##                               ##   ####
   ##                               ## ####
    ##                               ## ###
     ##                               ## #";

/// The `RatatuiLogo::small` wordmark, in quadrant blocks.
const LOGO_SMALL: &str = "█▀▀▄ ▄▀▀▄▝▜▛▘▄▀▀▄▝▜▛▘█  █ █\n█▀▀▄ █▀▀█ ▐▌ █▀▀█ ▐▌ ▀▄▄▀ █";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subject_fits_the_inline_pixels() {
        for &subject in SUBJECTS {
            let art = subject.bitmap();
            assert!(art.width * art.height <= PIXELS);
            assert!(
                (0..art.height as i32).any(|y| art.get(0, y) != Material::Empty
                    || art.get(1, y) != Material::Empty
                    || y > 0)
            );
        }
        assert_eq!(
            (
                Subject::Emblem.bitmap().width,
                Subject::Emblem.bitmap().height
            ),
            (50, 50)
        );
        assert_eq!(
            (
                Subject::Wordmark.bitmap().width,
                Subject::Wordmark.bitmap().height
            ),
            (54, 4)
        );
    }
}
