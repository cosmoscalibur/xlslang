//! Functions that exist (or behave differently) only in the OpenDocument
//! namespace. Registered under `Ns::Ods`; the parser reaches them for
//! ODS-sourced formulas and for `COM.MICROSOFT.`/`ORG.OPENOFFICE.` prefixes.

use super::{Reg, num};
use crate::dates;
use crate::ir::Ns;
use crate::value::{ErrorKind, Value};

pub(super) fn register(r: &mut Reg) {
    // LibreOffice has no boolean type: TRUE() is a number.
    r.eager_in(Ns::Ods, "ISNUMBER", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(
            e.scalar(a[0].clone()),
            Value::Number(_) | Value::Bool(_)
        )))
    });

    // Gregorian Easter (Meeus/Jones/Butcher); Excel has no such function.
    r.eager_in(Ns::Ods, "EASTERSUNDAY", 1, 1, |e, a| {
        let y = e.num(&a[0])?.trunc() as i64;
        if !(1583..=9999).contains(&y) {
            return Err(ErrorKind::Num);
        }
        let (a_, b, c) = (y % 19, y / 100, y % 100);
        let (d, e_) = (b / 4, b % 4);
        let f = (b + 8) / 25;
        let g = (b - f + 1) / 3;
        let h = (19 * a_ + b - d - g + 15) % 30;
        let (i, k) = (c / 4, c % 4);
        let l = (32 + 2 * e_ + 2 * i - h - k) % 7;
        let m = (a_ + 11 * h + 22 * l) / 451;
        let month = (h + l - 7 * m + 114) / 31;
        let day = (h + l - 7 * m + 114) % 31 + 1;
        dates::ymd_to_serial(y, month, day, e.opts.date_system).map_or(Err(ErrorKind::Num), num)
    });
}
