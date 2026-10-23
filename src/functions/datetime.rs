//! Date and time functions (serial numbers in the configured date system).

use super::{Reg, num, opt_num};
use crate::dates;
use crate::engine::{Engine, Res};
use crate::value::{ErrorKind, Value};

pub(super) fn register(r: &mut Reg) {
    r.eager("DATE", 3, 3, |e, a| {
        let (mut y, m, d) = (
            e.num(&a[0])?.trunc() as i64,
            e.num(&a[1])?.trunc() as i64,
            e.num(&a[2])?.trunc() as i64,
        );
        if (0..1900).contains(&y) && e.opts.date_system == crate::options::DateSystem::Excel1900 {
            y += 1900;
        }
        dates::ymd_to_serial(y, m, d, e.opts.date_system).map_or(Err(ErrorKind::Num), num)
    });
    r.eager("YEAR", 1, 1, |e, a| {
        ymd(e, a).map(|t| t.0 as f64).and_then(num)
    });
    r.eager("MONTH", 1, 1, |e, a| {
        ymd(e, a).map(|t| t.1 as f64).and_then(num)
    });
    r.eager("DAY", 1, 1, |e, a| {
        ymd(e, a).map(|t| t.2 as f64).and_then(num)
    });
    r.eager("HOUR", 1, 1, |e, a| {
        hms(e, a).map(|t| (t / 3600) as f64).and_then(num)
    });
    r.eager("MINUTE", 1, 1, |e, a| {
        hms(e, a).map(|t| (t / 60 % 60) as f64).and_then(num)
    });
    r.eager("SECOND", 1, 1, |e, a| {
        hms(e, a).map(|t| (t % 60) as f64).and_then(num)
    });
    r.eager("TIME", 3, 3, |e, a| {
        let (h, m, s) = (e.num(&a[0])?, e.num(&a[1])?, e.num(&a[2])?);
        let total = (h.trunc() * 3600.0 + m.trunc() * 60.0 + s.trunc()) / 86_400.0;
        if total < 0.0 {
            Err(ErrorKind::Num)
        } else {
            num(total.fract())
        }
    });
    r.eager("TODAY", 0, 0, |e, _| num(e.opts.now_serial().floor()))
        .volatile();
    r.eager("NOW", 0, 0, |e, _| num(e.opts.now_serial()))
        .volatile();
    r.eager("DATEVALUE", 1, 1, |e, a| {
        let t = e.text(&a[0])?;
        crate::coerce::parse_number(&t, &e.opts.locale, e.opts.date_system)
            .map_or(Err(ErrorKind::Value), |n| num(n.floor()))
    });
    r.eager("WEEKDAY", 1, 2, |e, a| {
        let s = e.num(&a[0])?;
        if s < 0.0 {
            return Err(ErrorKind::Num);
        }
        let sun0 = dates::weekday_sun0(s, e.opts.date_system);
        let v = match opt_num(e, a, 1, 1.0)? as i32 {
            1 => sun0 + 1,
            2 => (sun0 + 6) % 7 + 1,
            3 => (sun0 + 6) % 7,
            _ => return Err(ErrorKind::Num),
        };
        num(v as f64)
    });
    r.eager("EDATE", 2, 2, |e, a| shift_months(e, a, false));
    r.eager("EOMONTH", 2, 2, |e, a| shift_months(e, a, true));
    r.eager("DAYS", 2, 2, |e, a| {
        num(e.num(&a[0])?.floor() - e.num(&a[1])?.floor())
    });
    r.eager("DATEDIF", 3, 3, |e, a| {
        let (s, t) = (e.num(&a[0])?.floor(), e.num(&a[1])?.floor());
        if s > t {
            return Err(ErrorKind::Num);
        }
        let sys = e.opts.date_system;
        let (y1, m1, d1) = dates::serial_to_ymd(s, sys).ok_or(ErrorKind::Num)?;
        let (y2, m2, d2) = dates::serial_to_ymd(t, sys).ok_or(ErrorKind::Num)?;
        let months = (y2 - y1) * 12 + (m2 - m1) - i64::from(d2 < d1);
        match e.text(&a[2])?.to_ascii_uppercase().as_str() {
            "D" => num(t - s),
            "M" => num(months as f64),
            "Y" => num((months / 12) as f64),
            "YM" => num((months % 12) as f64),
            _ => Err(ErrorKind::Num),
        }
    });
}

fn ymd(e: &mut Engine, a: &[Value]) -> Res<(i64, i64, i64)> {
    let s = e.num(&a[0])?;
    dates::serial_to_ymd(s, e.opts.date_system).ok_or(ErrorKind::Num)
}

fn hms(e: &mut Engine, a: &[Value]) -> Res<i64> {
    let s = e.num(&a[0])?;
    if s < 0.0 {
        return Err(ErrorKind::Num);
    }
    Ok(((s - s.floor()) * 86_400.0).round() as i64 % 86_400)
}

fn shift_months(e: &mut Engine, a: &[Value], end_of_month: bool) -> Res<Value> {
    let s = e.num(&a[0])?;
    let k = e.num(&a[1])?.trunc() as i64;
    let sys = e.opts.date_system;
    let (y, m, d) = dates::serial_to_ymd(s, sys).ok_or(ErrorKind::Num)?;
    let total = y * 12 + (m - 1) + k;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let dim = dates::days_in_month(ny, nm);
    let nd = if end_of_month { dim } else { d.min(dim) };
    dates::ymd_to_serial(ny, nm, nd, sys).map_or(Err(ErrorKind::Num), num)
}
