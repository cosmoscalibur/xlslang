//! Basic time-value-of-money functions.

use super::{Reg, VAR, num, opt, opt_num};
use crate::engine::Engine;
use crate::value::{ErrorKind, Value};

pub(super) fn register(r: &mut Reg) {
    r.eager("PMT", 3, 5, |e, a| {
        let (rate, n, pv, fv, ty) = tvm(e, a)?;
        if n == 0.0 {
            return Err(ErrorKind::Num);
        }
        if rate == 0.0 {
            return num(-(pv + fv) / n);
        }
        let g = (1.0 + rate).powf(n);
        num(-(rate * (fv + pv * g)) / ((1.0 + rate * ty) * (g - 1.0)))
    });
    r.eager("FV", 3, 5, |e, a| {
        let (rate, n, pmt, pv, ty) = tvm(e, a)?;
        if rate == 0.0 {
            return num(-(pv + pmt * n));
        }
        let g = (1.0 + rate).powf(n);
        num(-(pv * g + pmt * (1.0 + rate * ty) * (g - 1.0) / rate))
    });
    r.eager("PV", 3, 5, |e, a| {
        let (rate, n, pmt, fv, ty) = tvm(e, a)?;
        if rate == 0.0 {
            return num(-(fv + pmt * n));
        }
        let g = (1.0 + rate).powf(n);
        num(-(fv + pmt * (1.0 + rate * ty) * (g - 1.0) / rate) / g)
    });
    r.eager("NPV", 2, VAR, |e, a| {
        let rate = e.num(&a[0])?;
        if rate == -1.0 {
            return Err(ErrorKind::DivZero);
        }
        let flows = e.numbers(&a[1..])?;
        num(flows
            .iter()
            .enumerate()
            .map(|(i, c)| c / (1.0 + rate).powi(i as i32 + 1))
            .sum())
    });
    r.eager("IRR", 1, 2, |e, a| {
        let flows = e.numbers(&a[..1])?;
        if !flows.iter().any(|c| *c > 0.0) || !flows.iter().any(|c| *c < 0.0) {
            return Err(ErrorKind::Num);
        }
        let mut x = opt_num(e, a, 1, 0.1)?;
        for _ in 0..100 {
            let f: f64 = flows
                .iter()
                .enumerate()
                .map(|(i, c)| c / (1.0 + x).powi(i as i32))
                .sum();
            let d: f64 = flows
                .iter()
                .enumerate()
                .map(|(i, c)| -(i as f64) * c / (1.0 + x).powi(i as i32 + 1))
                .sum();
            if d == 0.0 {
                break;
            }
            let nx = x - f / d;
            if (nx - x).abs() < 1e-10 {
                return num(nx);
            }
            x = nx;
        }
        Err(ErrorKind::Num)
    });
}

/// `(rate, nper, a, b, type)` for the annuity family.
fn tvm(e: &mut Engine, a: &[Value]) -> Result<(f64, f64, f64, f64, f64), ErrorKind> {
    let ty = if opt(a, 4).is_some() && e.num(&a[4])? != 0.0 {
        1.0
    } else {
        0.0
    };
    Ok((
        e.num(&a[0])?,
        e.num(&a[1])?,
        e.num(&a[2])?,
        opt_num(e, a, 3, 0.0)?,
        ty,
    ))
}
