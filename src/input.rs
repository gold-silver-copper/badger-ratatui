//! Hardware-independent input shared by the showcase and the sensor driver.

pub const DOWN: u8 = 1 << 0;
pub const LEFT: u8 = 1 << 1;
pub const RIGHT: u8 = 1 << 2;
pub const ENTER: u8 = 1 << 3;
pub const UP: u8 = 1 << 4;

#[derive(Clone, Copy, Default)]
pub struct Reading {
    /// Physical switches, pressed = 1: `DOWN`, `LEFT`, `RIGHT`, `ENTER`, `UP`.
    pub switches: u8,
    /// Acceleration in g, in the screen's frame: x right, y down, z out of
    /// the screen. At rest it points downhill: the badge's tilt.
    pub accel: [f32; 3],
    /// Rotation rate in degrees per second, in the same frame.
    pub gyro: [f32; 3],
}
