//! The LSM6DS3TR-C IMU (0x6A) on the internal I2C bus, plus the physical
//! switches on GPIO.

use embassy_rp::gpio::Input;
use embassy_rp::i2c::{Blocking, I2c};
use embassy_rp::peripherals::I2C1;

const IMU: u8 = 0x6A;

use crate::input::Reading;
pub use crate::input::{DOWN, UP};

/// One g on the ±2 g range.
const ONE_G: f32 = 16_393.0;
/// One degree per second on the ±245 dps range (8.75 mdps per count).
const ONE_DPS: f32 = 1000.0 / 8.75;

pub struct Sensors {
    i2c: I2c<'static, I2C1, Blocking>,
    switches: [Input<'static>; 5],
    imu_ok: bool,
}

impl Sensors {
    pub fn new(i2c: I2c<'static, I2C1, Blocking>, switches: [Input<'static>; 5]) -> Self {
        let mut s = Self {
            i2c,
            switches,
            imu_ok: false,
        };
        s.imu_ok = s.read_reg(IMU, 0x0F) == Some(0x6A)
            && s.write_reg(IMU, 0x12, 0x44) // CTRL3_C: block data update, auto-increment
            && s.write_reg(IMU, 0x10, 0x40) // CTRL1_XL: 104 Hz, ±2 g
            && s.write_reg(IMU, 0x11, 0x40); // CTRL2_G: 104 Hz, 245 dps
        s
    }

    pub fn read(&mut self) -> Reading {
        let mut switches = 0;
        for (i, sw) in self.switches.iter().enumerate() {
            if sw.is_low() {
                switches |= 1 << i;
            }
        }
        let (accel, gyro) = if self.imu_ok {
            self.motion()
        } else {
            ([0.0; 3], [0.0; 3])
        };
        Reading {
            switches,
            accel,
            gyro,
        }
    }

    /// Acceleration and rotation, read in one burst from OUTX_L_G (0x22):
    /// the gyro's three axes, then the accelerometer's.
    fn motion(&mut self) -> ([f32; 3], [f32; 3]) {
        let mut buf = [0u8; 12];
        if self
            .i2c
            .blocking_write_read(IMU, &[0x22], &mut buf)
            .is_err()
        {
            return ([0.0; 3], [0.0; 3]);
        }
        let [g0, g1, g2, g3, g4, g5, a0, a1, a2, a3, a4, a5] = buf;
        let axes = |b: [[u8; 2]; 3], unit: f32| b.map(|b| f32::from(i16::from_le_bytes(b)) / unit);
        // The IMU's axes on the screen: its -y points right, its +x down.
        let screen = |[x, y, z]: [f32; 3]| [-y, x, z];
        (
            screen(axes([[a0, a1], [a2, a3], [a4, a5]], ONE_G)),
            screen(axes([[g0, g1], [g2, g3], [g4, g5]], ONE_DPS)),
        )
    }

    fn read_reg(&mut self, addr: u8, reg: u8) -> Option<u8> {
        let mut b = [0u8];
        self.i2c.blocking_write_read(addr, &[reg], &mut b).ok()?;
        let [v] = b;
        Some(v)
    }

    fn write_reg(&mut self, addr: u8, reg: u8, val: u8) -> bool {
        self.i2c.blocking_write(addr, &[reg, val]).is_ok()
    }
}
