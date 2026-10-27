//! Single-precision approximations of the maths the showcase needs.
//!
//! The badge's Cortex-M33 has a single-precision FPU only, and libm's `f32`
//! functions do much of their work in `f64`, in software there. These stay
//! in `f32`, accurate to well under what a pixel's colour shows (sine to
//! about 0.001, exp and log to about 1e-4 relative).

use core::f32::consts::{FRAC_PI_2, PI};

const INV_TAU: f32 = 1.0 / core::f32::consts::TAU;

#[inline]
pub fn floorf(x: f32) -> f32 {
    // Exact for |x| < 2^31, which covers everything here.
    let i = x as i32 as f32;
    if i > x { i - 1.0 } else { i }
}

#[inline]
pub fn roundf(x: f32) -> f32 {
    floorf(x + 0.5)
}

#[inline]
pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    // 1/sqrt(x) from the exponent bits, sharpened by two Newton steps.
    let mut y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    let half = 0.5 * x;
    y *= 1.5 - half * y * y;
    y *= 1.5 - half * y * y;
    x * y
}

#[inline]
pub fn sinf(x: f32) -> f32 {
    // One turn in t, centred on 0: t in [-0.5, 0.5).
    let t = x * INV_TAU;
    let t = t - floorf(t + 0.5);
    // A parabola through sin's zeros and peaks, then refined.
    let y = 8.0 * t - 16.0 * t * t.abs();
    0.225 * (y * y.abs() - y) + y
}

#[inline]
pub fn cosf(x: f32) -> f32 {
    sinf(x + FRAC_PI_2)
}

#[inline]
fn exp2f(x: f32) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let i = floorf(x);
    let f = x - i;
    // 2^f on [0, 1), a least-squares fit.
    let p = 0.999_999_77
        + f * (0.693_156_8
            + f * (0.240_131_73 + f * (0.055_876_505 + f * (0.008_940_604 + f * 0.001_894_382_7))));
    f32::from_bits(((i as i32 + 127) as u32) << 23) * p
}

#[inline]
pub fn expf(x: f32) -> f32 {
    exp2f(x * core::f32::consts::LOG2_E)
}

#[inline]
pub fn log2f(x: f32) -> f32 {
    if x <= 0.0 {
        return -126.0;
    }
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    // The mantissa as 1 + t, t in [0, 1); log2(1 + t) by a least-squares fit.
    let t = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000) - 1.0;
    let p = 3.180_727_3e-5
        + t * (1.441_268_9
            + t * (-0.705_711 + t * (0.408_734_17 + t * (-0.187_732_14 + t * 0.043_431_324))));
    e as f32 + p
}

#[inline]
pub fn powf(x: f32, y: f32) -> f32 {
    if x <= 0.0 { 0.0 } else { exp2f(y * log2f(x)) }
}

#[inline]
pub fn atan2f(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let big = ax.max(ay);
    if big == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / big;
    let s = a * a;
    let mut r = ((-0.046_496_475 * s + 0.159_314_22) * s - 0.327_622_76) * s * a + a;
    if ay > ax {
        r = FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 { -r } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance * b.abs().max(1.0)
    }

    #[test]
    fn approximations_track_libm() {
        let mut x = -40.0f32;
        while x < 40.0 {
            assert!(close(sinf(x), libm::sinf(x), 2e-3), "sin {x}");
            assert!(close(cosf(x), libm::cosf(x), 2e-3), "cos {x}");
            assert!(close(floorf(x), libm::floorf(x), 0.0), "floor {x}");
            assert!(
                close(
                    atan2f(x, 3.0 - x * 0.5),
                    libm::atan2f(x, 3.0 - x * 0.5),
                    2e-3
                ),
                "atan2 {x}"
            );
            assert!(close(expf(x * 0.2), libm::expf(x * 0.2), 2e-4), "exp {x}");
            let p = x.abs() + 0.01;
            assert!(close(log2f(p), libm::log2f(p), 2e-4), "log2 {p}");
            assert!(close(powf(p, 1.5), libm::powf(p, 1.5), 1e-3), "pow {p}");
            assert!(close(sqrtf(p), libm::sqrtf(p), 1e-5), "sqrt {p}");
            x += 0.037;
        }
        assert_eq!(powf(0.0, 2.0), 0.0);
        assert!(close(sinf(1000.0), libm::sinf(1000.0), 2e-3));
    }
}
