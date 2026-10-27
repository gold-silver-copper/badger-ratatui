//! Hardware-independent input shared by the screens and the sensor driver.

pub const DOWN: u8 = 1 << 0;
pub const LEFT: u8 = 1 << 1;
pub const RIGHT: u8 = 1 << 2;
pub const ENTER: u8 = 1 << 3;
pub const UP: u8 = 1 << 4;

#[derive(Clone, Copy, Default)]
pub struct Reading {
    /// One bit per touch pad, CAP1..CAP8.
    pub touch: u8,
    /// Physical switches, pressed = 1: `DOWN`, `LEFT`, `RIGHT`, `ENTER`, `UP`.
    pub switches: u8,
    /// Acceleration in g: x, y, z.
    pub accel: [f32; 3],
    pub imu_ok: bool,
    pub cap_ok: bool,
}
