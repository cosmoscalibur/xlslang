//! Logical functions, special forms and information functions.

use super::{Reg, VAR, num};
use crate::coerce;
use crate::engine::{Engine, Res};
use crate::ir::Expr;
use crate::value::compare;
use crate::value::{ErrorKind, Value};
use std::cmp::Ordering;

pub(super) fn register(r: &mut Reg) {
    r.lazy("IF", 2, 3, |e, a| {
        let c = e.eval(&a[0]);
        let branch = if e.boolean(&c)? {
            Some(&a[1])
        } else {
            a.get(2)
        };
        match branch {
            None => Ok(Value::Bool(false)),
            Some(Expr::Missing) => Ok(Value::Number(0.0)),
            Some(x) => Ok(e.eval(x)),
        }
    });
    r.lazy("IFS", 2, VAR, |e, a| {
        for p in a.chunks(2) {
            if p.len() < 2 {
                return Err(ErrorKind::Value);
            }
            let c = e.eval(&p[0]);
            if e.boolean(&c)? {
                return Ok(e.eval(&p[1]));
            }
        }
        Err(ErrorKind::Na)
    });
    r.lazy("IFERROR", 2, 2, |e, a| guard(e, a, |_| true));
    r.lazy("IFNA", 2, 2, |e, a| guard(e, a, |k| k == ErrorKind::Na));
    r.lazy("CHOOSE", 2, VAR, |e, a| {
        let i = e.eval(&a[0]);
        let i = e.num(&i)?.trunc();
        if i < 1.0 || i as usize >= a.len() {
            return Err(ErrorKind::Value);
        }
        Ok(e.eval(&a[i as usize]))
    });
    r.lazy("SWITCH", 3, VAR, |e, a| {
        let x = e.eval(&a[0]);
        let x = e.scalar(x);
        if let Value::Error(k) = x {
            return Err(k);
        }
        let mut rest = &a[1..];
        while rest.len() >= 2 {
            let c = e.eval(&rest[0]);
            let c = e.scalar(c);
            if compare(&x, &c, e.opts.search.case_sensitive) == Ordering::Equal
                && std::mem::discriminant(&x) == std::mem::discriminant(&c)
            {
                return Ok(e.eval(&rest[1]));
            }
            rest = &rest[2..];
        }
        match rest.first() {
            Some(d) => Ok(e.eval(d)),
            None => Err(ErrorKind::Na),
        }
    });
    r.eager("AND", 1, VAR, |e, a| fold_bool(e, a, true, |x, y| x && y));
    r.eager("OR", 1, VAR, |e, a| fold_bool(e, a, false, |x, y| x || y));
    r.eager("XOR", 1, VAR, |e, a| fold_bool(e, a, false, |x, y| x ^ y));
    r.eager("NOT", 1, 1, |e, a| Ok(Value::Bool(!e.boolean(&a[0])?)));
    r.eager("TRUE", 0, 0, |_, _| Ok(Value::Bool(true)));
    r.eager("FALSE", 0, 0, |_, _| Ok(Value::Bool(false)));

    r.eager("ISBLANK", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(e.scalar(a[0].clone()), Value::Empty)))
    });
    r.eager("ISNUMBER", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(
            e.scalar(a[0].clone()),
            Value::Number(_)
        )))
    });
    r.eager("ISTEXT", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(
            e.scalar(a[0].clone()),
            Value::Text(_)
        )))
    });
    r.eager("ISNONTEXT", 1, 1, |e, a| {
        Ok(Value::Bool(!matches!(
            e.scalar(a[0].clone()),
            Value::Text(_)
        )))
    });
    r.eager("ISLOGICAL", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(
            e.scalar(a[0].clone()),
            Value::Bool(_)
        )))
    });
    r.eager("ISERROR", 1, 1, |e, a| {
        Ok(Value::Bool(e.scalar(a[0].clone()).is_error()))
    });
    r.eager("ISERR", 1, 1, |e, a| {
        Ok(Value::Bool(
            matches!(e.scalar(a[0].clone()), Value::Error(k) if k != ErrorKind::Na),
        ))
    });
    r.eager("ISNA", 1, 1, |e, a| {
        Ok(Value::Bool(matches!(
            e.scalar(a[0].clone()),
            Value::Error(ErrorKind::Na)
        )))
    });
    r.eager("ISREF", 1, 1, |_, a| {
        Ok(Value::Bool(matches!(a[0], Value::Ref(_))))
    });
    r.eager("ISEVEN", 1, 1, |e, a| {
        Ok(Value::Bool(e.num(&a[0])?.trunc() % 2.0 == 0.0))
    });
    r.eager("ISODD", 1, 1, |e, a| {
        Ok(Value::Bool(e.num(&a[0])?.trunc().abs() % 2.0 == 1.0))
    });
    r.eager("NA", 0, 0, |_, _| Err(ErrorKind::Na));
    r.eager("N", 1, 1, |e, a| match e.scalar(a[0].clone()) {
        Value::Number(n) => Ok(Value::Number(n)),
        Value::Bool(b) => num(f64::from(u8::from(b))),
        Value::Error(k) => Err(k),
        _ => num(0.0),
    });
    r.eager("ERROR.TYPE", 1, 1, |e, a| match e.scalar(a[0].clone()) {
        Value::Error(k) => num(k.code()),
        _ => Err(ErrorKind::Na),
    });
}

/// `IFERROR`/`IFNA`: evaluate the first argument, fall back to the second when
/// it is an error accepted by `catches`.
fn guard(e: &mut Engine, a: &[Expr], catches: fn(ErrorKind) -> bool) -> Res<Value> {
    let v = e.eval(&a[0]);
    let probe = match &v {
        Value::Ref(r) if r.is_single() => e.scalar(v.clone()),
        Value::Error(_) => v.clone(),
        _ => return Ok(v),
    };
    match probe {
        Value::Error(k) if catches(k) => Ok(match &a[1] {
            Expr::Missing => Value::Number(0.0),
            x => e.eval(x),
        }),
        _ => Ok(v),
    }
}

fn fold_bool(e: &mut Engine, a: &[Value], init: bool, f: fn(bool, bool) -> bool) -> Res<Value> {
    let mut acc = init;
    let mut seen = false;
    for v in a {
        match v {
            Value::Ref(_) | Value::Array(_) => {
                for x in &e.array_of(v.clone()).data {
                    match x {
                        Value::Bool(b) => {
                            acc = f(acc, *b);
                            seen = true;
                        }
                        Value::Number(n) => {
                            acc = f(acc, *n != 0.0);
                            seen = true;
                        }
                        Value::Error(k) => return Err(*k),
                        _ => {}
                    }
                }
            }
            v => {
                acc = f(acc, coerce::to_bool(v, &e.opts.locale)?);
                seen = true;
            }
        }
    }
    if seen {
        Ok(Value::Bool(acc))
    } else {
        Err(ErrorKind::Value)
    }
}
