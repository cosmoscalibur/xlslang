//! Arithmetic, rounding and trigonometry.

use super::{Reg, VAR, num, opt_num};
use crate::engine::{Engine, Res};
use crate::value::{ErrorKind, Value};

pub(super) fn register(r: &mut Reg) {
    r.eager("SUM", 1, VAR, |e, a| num(e.numbers(a)?.iter().sum()));
    r.eager("PRODUCT", 1, VAR, |e, a| {
        num(e.numbers(a)?.iter().product())
    });
    r.eager("SUMSQ", 1, VAR, |e, a| {
        num(e.numbers(a)?.iter().map(|x| x * x).sum())
    });
    r.eager("ABS", 1, 1, |e, a| num(e.num(&a[0])?.abs()));
    r.eager("SIGN", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        num(if x == 0.0 { 0.0 } else { x.signum() })
    });
    r.eager("SQRT", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        if x < 0.0 {
            Err(ErrorKind::Num)
        } else {
            num(x.sqrt())
        }
    });
    r.eager("POWER", 2, 2, |e, a| {
        let (x, y) = (e.num(&a[0])?, e.num(&a[1])?);
        if x == 0.0 && y == 0.0 {
            Err(ErrorKind::Num)
        } else {
            num(x.powf(y))
        }
    });
    r.eager("EXP", 1, 1, |e, a| num(e.num(&a[0])?.exp()));
    r.eager("LN", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        if x <= 0.0 {
            Err(ErrorKind::Num)
        } else {
            num(x.ln())
        }
    });
    r.eager("LOG10", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        if x <= 0.0 {
            Err(ErrorKind::Num)
        } else {
            num(x.log10())
        }
    });
    r.eager("LOG", 1, 2, |e, a| {
        let x = e.num(&a[0])?;
        let b = opt_num(e, a, 1, 10.0)?;
        if x <= 0.0 || b <= 0.0 || b == 1.0 {
            Err(ErrorKind::Num)
        } else {
            num(x.ln() / b.ln())
        }
    });
    r.eager("MOD", 2, 2, |e, a| {
        let (x, y) = (e.num(&a[0])?, e.num(&a[1])?);
        if y == 0.0 {
            Err(ErrorKind::DivZero)
        } else {
            num(x - y * (x / y).floor())
        }
    });
    r.eager("INT", 1, 1, |e, a| num(e.num(&a[0])?.floor()));
    r.eager("PI", 0, 0, |_, _| num(std::f64::consts::PI));
    r.eager("SIN", 1, 1, |e, a| num(e.num(&a[0])?.sin()));
    r.eager("COS", 1, 1, |e, a| num(e.num(&a[0])?.cos()));
    r.eager("TAN", 1, 1, |e, a| num(e.num(&a[0])?.tan()));
    r.eager("ATAN", 1, 1, |e, a| num(e.num(&a[0])?.atan()));
    r.eager("ATAN2", 2, 2, |e, a| {
        let (x, y) = (e.num(&a[0])?, e.num(&a[1])?);
        if x == 0.0 && y == 0.0 {
            Err(ErrorKind::DivZero)
        } else {
            num(y.atan2(x))
        }
    });
    r.eager("DEGREES", 1, 1, |e, a| num(e.num(&a[0])?.to_degrees()));
    r.eager("RADIANS", 1, 1, |e, a| num(e.num(&a[0])?.to_radians()));
    r.eager("FACT", 1, 1, |e, a| {
        let x = e.num(&a[0])?.trunc();
        if !(0.0..=170.0).contains(&x) {
            return Err(ErrorKind::Num);
        }
        num((1..=x as u32).map(f64::from).product())
    });
    r.eager("ROUND", 2, 2, |e, a| round_fn(e, a, Mode::Nearest));
    r.eager("ROUNDUP", 2, 2, |e, a| round_fn(e, a, Mode::Away));
    r.eager("ROUNDDOWN", 2, 2, |e, a| round_fn(e, a, Mode::Down));
    r.eager("TRUNC", 1, 2, |e, a| {
        let d = opt_num(e, a, 1, 0.0)?;
        num(round_dec(e.num(&a[0])?, d as i32, Mode::Down))
    });
    r.eager("CEILING", 1, 2, |e, a| multiple(e, a, f64::ceil));
    r.eager("FLOOR", 1, 2, |e, a| multiple(e, a, f64::floor));
    r.eager("EVEN", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        let r = (x.abs() / 2.0).ceil() * 2.0;
        num(r.copysign(x))
    });
    r.eager("ODD", 1, 1, |e, a| {
        let x = e.num(&a[0])?;
        let r = ((x.abs() - 1.0) / 2.0).ceil() * 2.0 + 1.0;
        num(r.copysign(x))
    });
    r.eager("RAND", 0, 0, |e, _| num(e.rand())).volatile();
    r.eager("RANDBETWEEN", 2, 2, |e, a| {
        let (lo, hi) = (e.num(&a[0])?.ceil(), e.num(&a[1])?.floor());
        if lo > hi {
            return Err(ErrorKind::Num);
        }
        num(lo + (e.rand() * (hi - lo + 1.0)).floor())
    })
    .volatile();
    r.eager("SUMPRODUCT", 1, VAR, sumproduct).array_args();
    // Wrapper the frontends put around array (CSE) formulas.
    r.eager("_ARRAY", 1, 1, |_, a| Ok(a[0].clone()))
        .array_args();
}

fn sumproduct(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let arrays: Vec<_> = a.iter().map(|v| e.array_of(v.clone())).collect();
    let (rows, cols) = (arrays[0].rows, arrays[0].cols);
    if arrays.iter().any(|m| m.rows != rows || m.cols != cols) {
        return Err(ErrorKind::Value);
    }
    let mut total = 0.0;
    for i in 0..arrays[0].data.len() {
        let mut p = 1.0;
        for m in &arrays {
            match &m.data[i] {
                Value::Number(n) => p *= n,
                Value::Error(e) => return Err(*e),
                _ => p = 0.0,
            }
        }
        total += p;
    }
    num(total)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Nearest,
    Away,
    Down,
}

fn round_fn(e: &mut Engine, a: &[Value], mode: Mode) -> Res<Value> {
    let (x, d) = (e.num(&a[0])?, e.num(&a[1])?);
    num(round_dec(x, d.trunc() as i32, mode))
}

/// Round at `digits` decimal places the way spreadsheets do: on the 15
/// significant decimal digits of the value, so `ROUND(1.005, 2)` is 1.01.
pub(crate) fn round_dec(x: f64, digits: i32, mode: Mode) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let sci = format!("{:.14e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut d: Vec<u8> = mant
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // Digits kept: those at or above the 10^-digits place.
    let keep = exp + 1 + digits;
    if keep >= d.len() as i32 {
        return x;
    }
    if keep < 0 {
        // Everything is below the rounding place.
        return if mode == Mode::Away {
            10f64.powi(-digits).copysign(x)
        } else {
            0.0
        };
    }
    let k = keep as usize;
    let up = match mode {
        Mode::Nearest => d[k] >= 5,
        Mode::Away => d[k..].iter().any(|&t| t != 0),
        Mode::Down => false,
    };
    d.truncate(k);
    let mut exp = exp;
    if up {
        let mut i = d.len();
        loop {
            if i == 0 {
                d.insert(0, 1);
                exp += 1;
                break;
            }
            i -= 1;
            if d[i] == 9 {
                d[i] = 0;
            } else {
                d[i] += 1;
                break;
            }
        }
    }
    if d.is_empty() {
        return 0.0;
    }
    let s: String = d.iter().map(|x| char::from(b'0' + x)).collect();
    let scale = exp - (d.len() as i32 - 1);
    format!("{s}e{scale}")
        .parse::<f64>()
        .map_or(x, |r| r.copysign(x))
}

fn multiple(e: &mut Engine, a: &[Value], f: fn(f64) -> f64) -> Res<Value> {
    let x = e.num(&a[0])?;
    let m = opt_num(e, a, 1, if x < 0.0 { -1.0 } else { 1.0 })?;
    if m == 0.0 {
        return num(0.0);
    }
    if x > 0.0 && m < 0.0 {
        return Err(ErrorKind::Num);
    }
    num(f(x / m) * m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_rounding() {
        assert_eq!(round_dec(1.005, 2, Mode::Nearest), 1.01);
        assert_eq!(round_dec(2.5, 0, Mode::Nearest), 3.0);
        assert_eq!(round_dec(-2.5, 0, Mode::Nearest), -3.0);
        assert_eq!(round_dec(1234.567, -2, Mode::Nearest), 1200.0);
        assert_eq!(round_dec(0.4, 0, Mode::Nearest), 0.0);
        assert_eq!(round_dec(9.99, 1, Mode::Nearest), 10.0);
        assert_eq!(round_dec(1.21, 1, Mode::Away), 1.3);
        assert_eq!(round_dec(-1.29, 1, Mode::Down), -1.2);
        assert_eq!(round_dec(0.0004, 2, Mode::Away), 0.01);
        assert_eq!(round_dec(1.23456, 10, Mode::Nearest), 1.23456);
    }
}
