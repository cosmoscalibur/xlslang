//! Serial-number date arithmetic for both spreadsheet date systems.

use crate::options::DateSystem;

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn epoch(sys: DateSystem) -> i64 {
    match sys {
        // Serial 0 → 1899-12-30 makes serials ≥ 61 exact; earlier ones are
        // shifted below to reproduce the fictitious 1900-02-29.
        DateSystem::Excel1900 => days_from_civil(1899, 12, 30),
        DateSystem::Excel1904 => days_from_civil(1904, 1, 1),
        DateSystem::Real1899 => days_from_civil(1899, 12, 30),
    }
}

/// Serial number of a calendar date; months and days may overflow like in
/// `DATE()`. `None` when the result is outside the representable range.
pub fn ymd_to_serial(y: i64, m: i64, d: i64, sys: DateSystem) -> Option<f64> {
    let y = y + (m - 1).div_euclid(12);
    let m = (m - 1).rem_euclid(12) + 1;
    let days = days_from_civil(y, m, 1) + (d - 1);
    let mut serial = days - epoch(sys);
    if sys == DateSystem::Excel1900 && days < days_from_civil(1900, 3, 1) {
        serial -= 1;
    }
    let min = if sys == DateSystem::Real1899 {
        -693_594
    } else {
        0
    };
    (min..2_958_466).contains(&serial).then_some(serial as f64)
}

pub fn serial_to_ymd(serial: f64, sys: DateSystem) -> Option<(i64, i64, i64)> {
    let min = if sys == DateSystem::Real1899 {
        -693_594.0
    } else {
        0.0
    };
    if !(min..2_958_466.0).contains(&serial) {
        return None;
    }
    let s = serial.floor() as i64;
    if sys == DateSystem::Excel1900 {
        match s {
            // Excel's day zero is the non-date 1900-01-00.
            0 => return Some((1900, 1, 0)),
            60 => return Some((1900, 2, 29)),
            1..=59 => return Some(civil_from_days(epoch(sys) + s + 1)),
            _ => {}
        }
    }
    Some(civil_from_days(epoch(sys) + s))
}

/// 0 = Sunday … 6 = Saturday.
pub fn weekday_sun0(serial: f64, sys: DateSystem) -> i64 {
    let s = serial.floor() as i64;
    match sys {
        DateSystem::Excel1900 => (s - 1).rem_euclid(7),
        DateSystem::Excel1904 => (s + 5).rem_euclid(7),
        // 1899-12-30 was a Saturday.
        DateSystem::Real1899 => (s + 6).rem_euclid(7),
    }
}

pub fn unix_to_serial(secs: f64, sys: DateSystem) -> f64 {
    let days = secs / 86_400.0;
    let mut serial = days + (days_from_civil(1970, 1, 1) - epoch(sys)) as f64;
    if sys == DateSystem::Excel1900 {
        serial = serial.max(61.0);
    }
    serial
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DateSystem::*;

    #[test]
    fn known_serials_1900() {
        assert_eq!(ymd_to_serial(1900, 1, 1, Excel1900), Some(1.0));
        assert_eq!(ymd_to_serial(1900, 2, 28, Excel1900), Some(59.0));
        assert_eq!(ymd_to_serial(1900, 3, 1, Excel1900), Some(61.0));
        assert_eq!(ymd_to_serial(2024, 1, 1, Excel1900), Some(45292.0));
        assert_eq!(serial_to_ymd(60.0, Excel1900), Some((1900, 2, 29)));
        assert_eq!(serial_to_ymd(45292.0, Excel1900), Some((2024, 1, 1)));
        assert_eq!(serial_to_ymd(1.0, Excel1900), Some((1900, 1, 1)));
    }

    #[test]
    fn known_serials_1904() {
        assert_eq!(ymd_to_serial(1904, 1, 1, Excel1904), Some(0.0));
        assert_eq!(ymd_to_serial(2024, 1, 1, Excel1904), Some(43830.0));
        assert_eq!(serial_to_ymd(43830.0, Excel1904), Some((2024, 1, 1)));
    }

    #[test]
    fn overflow_and_weekday() {
        assert_eq!(
            ymd_to_serial(2024, 14, 1, Excel1900),
            ymd_to_serial(2025, 2, 1, Excel1900)
        );
        assert_eq!(
            ymd_to_serial(2024, 3, 0, Excel1900),
            ymd_to_serial(2024, 2, 29, Excel1900)
        );
        // 2024-01-01 was a Monday.
        assert_eq!(weekday_sun0(45292.0, Excel1900), 1);
        assert_eq!(weekday_sun0(43830.0, Excel1904), 1);
    }
}
