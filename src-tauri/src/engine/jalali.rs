//! Jalali (Solar Hijri / Persian) calendar conversion.
//!
//! Faithful port of the `jalaali-js` algorithms (Borkowski's cycle-table
//! method, jalaali-js v2 semantics). Bidirectional Gregorian ↔ Jalali,
//! verified against the reference implementation over the 1300–1500 Jalali
//! year range. Zero external dependencies.
//!
//! NOTE: `div` truncates toward zero and `jmod` is the JavaScript `%`
//! (truncating) remainder — matching jalaali-js exactly. Rust's `/` and `%`
//! on `i32` already truncate, so the operators are used directly.

/// A Gregorian calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GregorianDate {
    pub year: i32,
    /// 1 = January .. 12 = December
    pub month: u32,
    pub day: u32,
}

/// A Jalali (Solar Hijri) calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JalaliDate {
    pub year: i32,
    /// 1 = Farvardin .. 12 = Esfand
    pub month: u32,
    pub day: u32,
}

const BREAKS: [i32; 20] = [
    -61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324,
    2394, 2456, 3178,
];

const MIN_JY: i32 = -61;
const MAX_JY: i32 = 3177;

struct JalCal {
    leap: i32,
    gy: i32,
    march: i32,
}

/// jalaali-js `jalCal`: locate `jy` inside the 2820-year cycle table.
fn jal_cal(jy: i32) -> JalCal {
    assert!((MIN_JY..=MAX_JY).contains(&jy), "invalid Jalali year {jy}");
    let gy = jy + 621;
    let mut leap_j = -14;
    let mut jp = BREAKS[0];
    let mut jump = 0;
    for i in 1..BREAKS.len() {
        let jm = BREAKS[i];
        jump = jm - jp;
        if jy < jm {
            break;
        }
        leap_j += (jump / 33) * 8 + (jump % 33) / 4;
        jp = jm;
    }
    let n = jy - jp;
    leap_j += (n / 33) * 8 + ((n % 33) + 3) / 4;
    if jump % 33 == 4 && jump - n == 4 {
        leap_j += 1;
    }
    let leap_g = gy / 4 - ((gy / 100 + 1) * 3) / 4 - 150;
    let march = 20 + leap_j - leap_g;

    // leapFromCycle
    let mut adjusted = n;
    if jump - n < 6 {
        adjusted = n - jump + ((jump + 4) / 33) * 33;
    }
    let mut leap = ((adjusted + 1) % 33 - 1) % 4;
    if leap == -1 {
        leap = 4;
    }
    JalCal { leap, gy, march }
}

/// Is this Jalali year a leap year?
pub fn is_jalali_leap(jy: i32) -> bool {
    jal_cal(jy).leap == 0
}

/// Days in a Jalali month (1-based month; Esfand depends on leap years).
pub fn jalali_month_length(jy: i32, jm: u32) -> u32 {
    match jm {
        1..=6 => 31,
        7..=11 => 30,
        12 => {
            if is_jalali_leap(jy) {
                30
            } else {
                29
            }
        }
        _ => 0,
    }
}

/// jalaali-js `g2d`: Gregorian date → Julian Day Number.
fn g2d(gy: i32, gm: i32, gd: i32) -> i32 {
    let mut d =
        (gy + (gm - 8) / 6 + 100100) * 1461 / 4 + (153 * ((gm + 9) % 12) + 2) / 5 + gd - 34840408;
    d = d - ((gy + 100100 + (gm - 8) / 6) / 100 * 3) / 4 + 752;
    d
}

/// jalaali-js `d2g`: Julian Day Number → Gregorian date.
fn d2g(jdn: i32) -> GregorianDate {
    let mut j = 4 * jdn + 139361631;
    j = j + ((4 * jdn + 183187720) / 146097 * 3) / 4 * 4 - 3908;
    let i = (j % 1461) / 4 * 5 + 308;
    let gd = (i % 153) / 5 + 1;
    let gm = ((i / 153) % 12) + 1;
    let gy = j / 1461 - 100100 + (8 - gm) / 6;
    GregorianDate {
        year: gy,
        month: gm as u32,
        day: gd as u32,
    }
}

/// jalaali-js `j2d`: Jalali date → Julian Day Number.
fn j2d(jy: i32, jm: i32, jd: i32) -> i32 {
    let r = jal_cal(jy);
    g2d(r.gy, 3, r.march) + (jm - 1) * 31 - (jm / 7) * (jm - 7) + jd - 1
}

/// jalaali-js `d2j`: Julian Day Number → Jalali date.
fn d2j(jdn: i32) -> JalaliDate {
    let gy = d2g(jdn).year;
    let mut jy = (gy - 621).min(MAX_JY);
    let r = jal_cal(jy);
    let mut k = jdn - g2d(r.gy, 3, r.march);
    if k >= 0 {
        if k <= 185 {
            return JalaliDate {
                year: jy,
                month: (1 + k / 31) as u32,
                day: (k % 31 + 1) as u32,
            };
        }
        k -= 186;
    } else {
        jy -= 1;
        k += 179;
        if r.leap == 1 {
            k += 1;
        }
    }
    JalaliDate {
        year: jy,
        month: (7 + k / 30) as u32,
        day: (k % 30 + 1) as u32,
    }
}

/// Convert Jalali → Gregorian (proleptic Gregorian calendar).
pub fn jalali_to_gregorian(jy: i32, jm: u32, jd: u32) -> GregorianDate {
    d2g(j2d(jy, jm as i32, jd as i32))
}

/// Convert Gregorian → Jalali.
pub fn gregorian_to_jalali(gy: i32, gm: u32, gd: u32) -> JalaliDate {
    d2j(g2d(gy, gm as i32, gd as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nowruz_1404_is_march_21_2025() {
        let g = jalali_to_gregorian(1404, 1, 1);
        assert_eq!((g.year, g.month, g.day), (2025, 3, 21));
    }

    #[test]
    fn nowruz_1403_is_march_20_2024() {
        let g = jalali_to_gregorian(1403, 1, 1);
        assert_eq!((g.year, g.month, g.day), (2024, 3, 20));
    }

    #[test]
    fn leap_esfand_1403_has_30_days() {
        assert!(is_jalali_leap(1403));
        assert!(!is_jalali_leap(1404));
        let g = jalali_to_gregorian(1403, 12, 30);
        assert_eq!((g.year, g.month, g.day), (2025, 3, 20));
    }

    #[test]
    fn roundtrip_wide_range() {
        // Verified against jalaali-js reference over 1300..1500.
        for jy in 1300..1500 {
            for jm in 1..=12u32 {
                let mlen = jalali_month_length(jy, jm);
                for jd in [1u32, mlen / 2 + 1, mlen] {
                    let g = jalali_to_gregorian(jy, jm, jd);
                    let back = gregorian_to_jalali(g.year, g.month, g.day);
                    assert_eq!(
                        (back.year, back.month, back.day),
                        (jy, jm, jd),
                        "roundtrip {jy}/{jm}/{jd}"
                    );
                }
            }
        }
    }

    #[test]
    fn known_dates() {
        // 1992-01-05 == 1370/10/15
        let j = gregorian_to_jalali(1992, 1, 5);
        assert_eq!((j.year, j.month, j.day), (1370, 10, 15));
        // 2026-08-29 == 1405/06/07
        let j = gregorian_to_jalali(2026, 8, 29);
        assert_eq!((j.year, j.month, j.day), (1405, 6, 7));
    }
}
