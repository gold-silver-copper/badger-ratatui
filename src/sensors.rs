//! The internal I2C bus: CAP1208 touch pads (0x28) and LSM6DS3TR-C IMU (0x6A),
//! plus the physical switches on GPIO.

use embassy_rp::gpio::Input;
use embassy_rp::i2c::{Blocking, I2c};
use embassy_rp::peripherals::I2C1;

const CAP: u8 = 0x28;
const IMU: u8 = 0x6A;

pub use crate::input::{DOWN, ENTER, LEFT, RIGHT, Reading, UP};

/// One g on the IMU's ±2 g range.
pub const ONE_G: f32 = 16_393.0;

pub struct Sensors {
    i2c: I2c<'static, I2C1, Blocking>,
    switches: [Input<'static>; 5],
    imu_ok: bool,
    cap_ok: bool,
}

impl Sensors {
    pub fn new(i2c: I2c<'static, I2C1, Blocking>, switches: [Input<'static>; 5]) -> Self {
        let mut s = Self {
            i2c,
            switches,
            imu_ok: false,
            cap_ok: false,
        };
        s.imu_ok = s.read_reg(IMU, 0x0F) == Some(0x6A)
            && s.write_reg(IMU, 0x12, 0x44) // CTRL3_C: block data update, auto-increment
            && s.write_reg(IMU, 0x10, 0x40) // CTRL1_XL: 104 Hz, ±2 g
            && s.write_reg(IMU, 0x11, 0x40); // CTRL2_G: 104 Hz, 245 dps
        s.cap_ok = s.read_reg(CAP, 0xFD) == Some(0x6B)
            && s.write_reg(CAP, 0x1F, 0x3F) // sensitivity, as MonaOS sets it
            && s.write_reg(CAP, 0x2A, 0x00); // allow several pads at once
        s
    }

    pub fn read(&mut self) -> Reading {
        let mut switches = 0;
        for (i, sw) in self.switches.iter().enumerate() {
            if sw.is_low() {
                switches |= 1 << i;
            }
        }
        Reading {
            touch: if self.cap_ok { self.touch() } else { 0 },
            switches,
            accel: if self.imu_ok { self.accel() } else { [0.0; 3] },
            imu_ok: self.imu_ok,
            cap_ok: self.cap_ok,
        }
    }

    fn touch(&mut self) -> u8 {
        let status = self.read_reg(CAP, 0x03).unwrap_or(0);
        // Clearing INT lets the status bits of released pads clear.
        if let Some(main) = self.read_reg(CAP, 0x00)
            && main & 1 != 0
        {
            self.write_reg(CAP, 0x00, main & !1);
        }
        status
    }

    fn accel(&mut self) -> [f32; 3] {
        let mut buf = [0u8; 6];
        if self
            .i2c
            .blocking_write_read(IMU, &[0x28], &mut buf)
            .is_err()
        {
            return [0.0; 3];
        }
        let [x0, x1, y0, y1, z0, z1] = buf;
        [[x0, x1], [y0, y1], [z0, z1]].map(|b| f32::from(i16::from_le_bytes(b)) / ONE_G)
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
