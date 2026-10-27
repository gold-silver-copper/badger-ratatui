# Ratatui on the GitHub Badger 2350

Rust firmware for the Universe 2026 badge: Embassy and Ratatui, through a
backend written for the badge's panel (`src/term/`).

## Screens and controls

Starts on the showcase. Press **left or right** to switch between showcase and
marble (once per press, wrapping around). Screens keep their state when hidden;
the hidden screen is paused.

| Button | Showcase | Marble |
| --- | --- | --- |
| Left / Right | Change screen | Change screen |
| Up | Next colour theme | Push up |
| Down | Next background | Push down |
| Enter | Next scene | Kick |
| Up + Down held 2 seconds | USB bootloader | USB bootloader |

Tilt rolls the marble. The capacitive pads are still unverified; this change
does not attempt to fix them.

## Showcase subset

Ported from [ratatui-showcase](https://github.com/gold-silver-copper/ratatui-showcase)
at `f33d69f`:

- Mascot, rat emblem and small Ratatui wordmark, with the mascot's distinct
  materials and blinking cursor/eye.
- All 11 named palettes, looping gradients and generated OKLCH harmonies.
- 13 ink shaders: flow, plasma, pulse, sheen, rainbow, scanline, chrome, holo,
  fire, stripes, marble, gold and glitter.
- 22 backgrounds: vignette, stars, synthwave grid, aurora, sun rays, Julia,
  kaleidoscope, tunnel, warp, metaballs, a Mandelbrot zoom into seahorse
  valley, the Burning Ship, Newton's method, the Apollonian gasket, hyperbolic
  {p, q} tilings, domain colouring and stained glass. From
  [hyper-surf](https://github.com/gold-silver-copper/hyper-surf)'s sky and
  worlds: the mandala, and slices through the Mandelbox (Halls and Citadel
  scales), the Kaliset (dunes, strands, badlands), a quaternion Julia set
  (Julia Reef's constants) and a quasicrystal. Each scene picks one form of
  its fractal.
- Solid, shadow, outline and neon treatments; still, float, wave, breathe and
  sway motions.
- 120 bpm, four bars per scene, related scene variations, two-beat wipes.
  Radial, diamond, clock, diagonal, blinds, checker, dissolve and spiral.
- Quadrant glyph encoding: the four samples of each cell split into two
  colour groups. A caption shows the scene for its first three seconds.

The desktop renderer requires a GPU. This is a CPU port of selected formulas,
not its Bevy/wgpu renderer. It uses 2x2 samples per cell instead of 4x8, three
noise octaves instead of four, 48 Julia iterations without orbit traps, and
six metaballs instead of seven. The fractals are tuned for the coarse grid:
shallower zooms, fewer folds and lines about a sample wide, and the 3D fields
are drawn as 2D slices. GPU simulations, 3D ray marching, distance fields,
system fonts and full-screen post-processing are not included.
Sources are in `src/showcase/{art,paint,fractal,mod}.rs`. The marble is in
`src/marble.rs`, and `src/main.rs` handles physical screen navigation.
The source artwork and ported algorithms retain their MIT notices in
`licenses/`.

## Build and test

```sh
cd ~/code/badger-ratatui
export TMPDIR="$HOME/.cache/rig-tmp"
export CARGO_TARGET_DIR="$HOME/.cache/rig-tmp/badger-target"
cargo test --lib --target aarch64-unknown-linux-gnu -j 2 -- --test-threads=2
cargo build --release -j 2
llvm-objcopy -O binary "$CARGO_TARGET_DIR/thumbv8m.main-none-eabihf/release/badger-ratatui" "$TMPDIR/badger.bin"
python3 tools/uf2.py "$TMPDIR/badger.bin" badger-ratatui.uf2
```

## Flash

```sh
cd ~/code/badger-ratatui && PYTHON=~/.cache/rig-tmp/badge-venv/bin/python tools/flash.sh
```

`tools/flash.sh` builds, packs and flashes. If the firmware is running, it is
told over USB to reboot into the bootloader; otherwise hold Up + Down for two
seconds (or hold HOME/BOOT, tap RESET, release HOME/BOOT) while it waits.
`PYTHON` needs pyserial, and your user needs access to the serial port: the
`uucp` group here (after logging in again, or under `newgrp uucp`).

## USB serial

The firmware shows up as `/dev/serial/by-id/usb-rig_badger-ratatui_badger-if00`.
Once a second it reports the mean frame timings and the showcase's backdrop;
it takes the commands `bootsel`, `screen` and `press up|down|enter`:

```sh
cd ~/code/badger-ratatui && ~/.cache/rig-tmp/badge-venv/bin/python tools/badge.py stats 5
```

`tools/bench-backdrops.sh` steps through every backdrop with `press down` and
prints each one's fps and render time.

- **render**: the screen's own drawing into ratatui's buffer (the showcase's
  shading, the marble's widgets).
- **draw**: ratatui's diff and the backend drawing the changed cells.
- **flush**: CPU time starting and waiting on panel transfers.
- **heap**: bytes in use / free, the high-water mark, and the largest single
  request since boot.
- **FAULT** / **BEFORE RESET**: the first panic or hard fault of this run, or
  of the run before a reset.

## Performance

How it runs: the backend keeps the framebuffer in the panel's byte order and
draws each changed cell as 13 rows of three two-pixel words, from 6x13 glyph
masks (fonts rasterized once per character; block elements and braille drawn
to fill the cell exactly). Only pixel rows that changed are sent: PIO1 drives
the 8-bit bus at 80 ns a byte, fed by DMA, while the next frame is computed.
The system clock is 200 MHz.

The showcase shades each sample with `f32` approximations of sin, cos, exp,
log2, pow, atan2, sqrt and floor (`src/showcase/fast.rs`, tested against
libm): libm's `f32` functions do much of their work in `f64`, which the
Cortex-M33 runs in software. Per-sample geometry (positions, radius, angle)
is computed once per grid size, and the rows are split between both cores.
The showcase's tables live in a static (`showcase::Buffers`), not the heap:
only Ratatui allocates, so the first-fit heap stays at about 54 KiB.

Mean frame times on the badge, in ms (8 seconds per screen; the showcase's
first scene, stars behind the mascot):

| Build | Showcase fps | render | draw | flush | Marble fps | render | draw | flush |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mousefood, 150 MHz | 2.3-3.0 | 255-376 | 66-76 | 14.9 | 10.4 | 78.6 | 2.0 | 14.9 |
| new backend, CPU-driven bus | 3.0-3.8 | 244-314 | 6.5-7.1 | 11.3 | 11.0 | 84.5 | 2.0 | 3.4-11 |
| new backend, PIO + DMA | 3.1-4.0 | 243-313 | 4.6-4.9 | 0.0 | 11.3 | 85.9 | 2.0 | 0.1 |
| + 200 MHz | 4.0-5.4 | 183-238 | 3.3-3.7 | 0.0 | 15.2 | 63.9 | 1.5 | 0.0 |
| + fast f32 maths, precomputed geometry | 33-41 | 21-25 | 3 | 0.0 | 15.2 | 63.9 | 1.5 | 0.0 |
| + second core | 48-58 | 14-17 | 2.4-3.1 | 0.0 | 15-18 | 55-64 | 1.5-2.1 | 0.0 |

The showcase is capped at 60 fps by the main loop's ticker. By backdrop
(`tools/bench-backdrops.sh`, with the first second of each step a transition):
31-59 fps, render 14-29 ms; the slowest are the Mandelbrot at its deepest zoom,
the kaleidoscope and the mandala.

Code runs from flash through a 16 KiB cache the two cores share. The fractals'
per-sample code is one function, kept out of line (`Fractal::paint`): inlined
into the other backdrops it made a 21 KiB function, and the Mandelbox, whose
code then fell where it evicted the shading, took 74 ms a frame instead of 24. The marble is
unchanged: ratatui's `Canvas` and widgets, in `f64`.

## Restore MonaOS

`backup/flash.bin` is the complete original 16 MiB flash dump. Its SHA-256,
verified against a digest calculated by the badge, is
`cec32378628155e8703f941cc77af6efb570fdc392858c20026ce51c83d70a68`.
The backup contains personal settings and secrets: do not publish it.

In USB bootloader mode:

```sh
cd ~/code/badger-ratatui && cp backup/restore-monaos.uf2 /run/media/gsc/RP2350/ && sync
```
