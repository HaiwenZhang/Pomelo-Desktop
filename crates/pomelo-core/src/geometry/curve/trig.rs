//! Deterministic fdlibm rounding for ordinary PCB angles. Exact endpoint equality
//! affects connector deduplication, so platform CRT trig is unsuitable here.
//!
//! Adapted from Node v24.18.0 / V8 src/base/ieee754.cc (sin, cos and medium
//! argument reduction). Large arguments use the platform-independent libm crate.
//
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
//
// Copyright 2016 the V8 project authors. All rights reserved.
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
// * Redistributions of source code must retain the above copyright notice,
//   this list of conditions and the following disclaimer.
// * Redistributions in binary form must reproduce the above copyright notice,
//   this list of conditions and the following disclaimer in the documentation
//   and/or other materials provided with the distribution.
// * Neither the name of Google Inc. nor the names of its contributors may be
//   used to endorse or promote products derived from this software without
//   specific prior written permission.
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

pub(super) fn sin(x: f64) -> f64 {
    if high_abs(x) <= 0x3fe921fb {
        return kernel_sin(x, 0.0, false);
    }
    let Some((n, a, b)) = reduce(x) else {
        return libm::sin(x);
    };
    match n & 3 {
        0 => kernel_sin(a, b, true),
        1 => kernel_cos(a, b),
        2 => -kernel_sin(a, b, true),
        _ => -kernel_cos(a, b),
    }
}

pub(super) fn cos(x: f64) -> f64 {
    if high_abs(x) <= 0x3fe921fb {
        return kernel_cos(x, 0.0);
    }
    let Some((n, a, b)) = reduce(x) else {
        return libm::cos(x);
    };
    match n & 3 {
        0 => kernel_cos(a, b),
        1 => -kernel_sin(a, b, true),
        2 => -kernel_cos(a, b),
        _ => kernel_sin(a, b, true),
    }
}

fn high_abs(x: f64) -> u32 {
    ((x.to_bits() >> 32) as u32) & 0x7fffffff
}

fn kernel_sin(x: f64, y: f64, tail: bool) -> f64 {
    const S1: f64 = f64::from_bits(0xbfc5555555555549);
    const S2: f64 = f64::from_bits(0x3f8111111110f8a6);
    const S3: f64 = f64::from_bits(0xbf2a01a019c161d5);
    const S4: f64 = f64::from_bits(0x3ec71de357b1fe7d);
    const S5: f64 = f64::from_bits(0xbe5ae5e68a2b9ceb);
    const S6: f64 = f64::from_bits(0x3de5d93a5acfd57c);
    if high_abs(x) < 0x3e400000 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if tail {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    } else {
        x + v * (S1 + z * r)
    }
}

fn kernel_cos(x: f64, y: f64) -> f64 {
    const C1: f64 = f64::from_bits(0x3fa555555555554c);
    const C2: f64 = f64::from_bits(0xbf56c16c16c15177);
    const C3: f64 = f64::from_bits(0x3efa01a019cb1590);
    const C4: f64 = f64::from_bits(0xbe927e4f809c52ad);
    const C5: f64 = f64::from_bits(0x3e21ee9ebdb4b1c4);
    const C6: f64 = f64::from_bits(0xbda8fae9be8838d4);
    let ix = high_abs(x);
    if ix < 0x3e400000 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3fd33333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3fe90000 {
            0.28125
        } else {
            f64::from_bits(u64::from(ix - 0x00200000) << 32)
        };
        let iz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (iz - (z * r - x * y))
    }
}

fn reduce(x: f64) -> Option<(i32, f64, f64)> {
    const INV_PIO2: f64 = f64::from_bits(0x3fe45f306dc9c883);
    const PIO2_1: f64 = f64::from_bits(0x3ff921fb54400000);
    const PIO2_1T: f64 = f64::from_bits(0x3dd0b4611a626331);
    const PIO2_2: f64 = f64::from_bits(0x3dd0b4611a600000);
    const PIO2_2T: f64 = f64::from_bits(0x3ba3198a2e037073);
    const PIO2_3: f64 = f64::from_bits(0x3ba3198a2e000000);
    const PIO2_3T: f64 = f64::from_bits(0x397b839a252049c1);
    const NPIO2_HW: [u32; 32] = [
        0x3ff921fb, 0x400921fb, 0x4012d97c, 0x401921fb, 0x401f6a7a, 0x4022d97c, 0x4025fdbb,
        0x402921fb, 0x402c463a, 0x402f6a7a, 0x4031475c, 0x4032d97c, 0x40346b9c, 0x4035fdbb,
        0x40378fdb, 0x403921fb, 0x403ab41b, 0x403c463a, 0x403dd85a, 0x403f6a7a, 0x40407e4c,
        0x4041475c, 0x4042106c, 0x4042d97c, 0x4043a28c, 0x40446b9c, 0x404534ac, 0x4045fdbb,
        0x4046c6cb, 0x40478fdb, 0x404858eb, 0x404921fb,
    ];
    let ix = high_abs(x);
    if ix > 0x413921fb {
        return None;
    }
    if ix < 0x4002d97c {
        let (n, z, correction) = if x > 0.0 {
            let z = x - PIO2_1;
            if ix == 0x3ff921fb {
                (1, z - PIO2_2, -PIO2_2T)
            } else {
                (1, z, -PIO2_1T)
            }
        } else {
            let z = x + PIO2_1;
            if ix == 0x3ff921fb {
                (-1, z + PIO2_2, PIO2_2T)
            } else {
                (-1, z, PIO2_1T)
            }
        };
        let a = z + correction;
        return Some((n, a, (z - a) + correction));
    }
    let n = (x.abs() * INV_PIO2 + 0.5) as i32;
    let turns = f64::from(n);
    let mut r = x.abs() - turns * PIO2_1;
    let mut w = turns * PIO2_1T;
    let mut a = r - w;
    if !(n < 32 && ix != NPIO2_HW[(n - 1) as usize]) {
        let exponent = (ix >> 20) as i32;
        if exponent - ((high_abs(a) >> 20) as i32) > 16 {
            let t = r;
            w = turns * PIO2_2;
            r = t - w;
            w = turns * PIO2_2T - ((t - r) - w);
            a = r - w;
            if exponent - ((high_abs(a) >> 20) as i32) > 49 {
                let t = r;
                w = turns * PIO2_3;
                r = t - w;
                w = turns * PIO2_3T - ((t - r) - w);
                a = r - w;
            }
        }
    }
    let b = (r - a) - w;
    Some(if x < 0.0 { (-n, -a, -b) } else { (n, a, b) })
}
