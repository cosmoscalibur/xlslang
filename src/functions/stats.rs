//! Aggregates and conditional aggregates.

use super::{Reg, VAR, num, opt};
use crate::coerce;
use crate::engine::{Engine, Res};
use crate::matching::Matcher;
use crate::value::{Array, ErrorKind, Value, cmp_text, compare};
use std::cmp::Ordering;
use std::sync::Arc;

pub(super) fn register(r: &mut Reg) {
    r.eager("AVERAGE", 1, VAR, |e, a| {
        let v = e.numbers(a)?;
        if v.is_empty() {
            Err(ErrorKind::DivZero)
        } else {
            num(v.iter().sum::<f64>() / v.len() as f64)
        }
    });
    r.eager("MIN", 1, VAR, |e, a| {
        num(e.numbers(a)?.into_iter().reduce(f64::min).unwrap_or(0.0))
    });
    r.eager("MAX", 1, VAR, |e, a| {
        num(e.numbers(a)?.into_iter().reduce(f64::max).unwrap_or(0.0))
    });
    r.eager("MEDIAN", 1, VAR, |e, a| {
        let mut v = e.numbers(a)?;
        if v.is_empty() {
            return Err(ErrorKind::Num);
        }
        v.sort_by(f64::total_cmp);
        let n = v.len();
        num(if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        })
    });
    r.eager("LARGE", 2, 2, |e, a| nth(e, a, true));
    r.eager("SMALL", 2, 2, |e, a| nth(e, a, false));
    r.eager("STDEV", 1, VAR, |e, a| {
        variance(e, a, true).map(|v| v.sqrt()).and_then(num)
    });
    r.eager("VAR", 1, VAR, |e, a| variance(e, a, true).and_then(num));
    r.eager("COUNT", 1, VAR, count);
    r.eager("COUNTA", 1, VAR, |e, a| {
        let mut n = 0u32;
        for v in a {
            match v {
                Value::Ref(_) | Value::Array(_) => {
                    n += e
                        .array_of(v.clone())
                        .data
                        .iter()
                        .filter(|x| !matches!(x, Value::Empty))
                        .count() as u32;
                }
                _ => n += 1,
            }
        }
        num(f64::from(n))
    });
    r.eager("COUNTBLANK", 1, 1, |e, a| {
        let arr = e.array_of(a[0].clone());
        let total = match &a[0] {
            Value::Ref(rect) => rect.area() as usize,
            _ => arr.data.len(),
        };
        let filled = arr
            .data
            .iter()
            .filter(|x| !matches!(x, Value::Empty) && !matches!(x, Value::Text(t) if t.is_empty()))
            .count();
        num((total - filled) as f64)
    });
    r.eager("SUMIF", 2, 3, |e, a| conditional(e, a, Agg::Sum));
    r.eager("AVERAGEIF", 2, 3, |e, a| conditional(e, a, Agg::Avg));
    r.eager("COUNTIF", 2, 2, |e, a| {
        let crit = Criterion::new(e, &a[1])?;
        let arr = e.array_of(a[0].clone());
        num(arr.data.iter().filter(|v| crit.matches(v)).count() as f64)
    });
    r.eager("SUMIFS", 3, VAR, |e, a| multi(e, a, Agg::Sum));
    r.eager("AVERAGEIFS", 3, VAR, |e, a| multi(e, a, Agg::Avg));
    r.eager("MAXIFS", 3, VAR, |e, a| multi(e, a, Agg::Max));
    r.eager("MINIFS", 3, VAR, |e, a| multi(e, a, Agg::Min));
    r.eager("COUNTIFS", 2, VAR, |e, a| {
        let mut conds = Vec::new();
        for p in a.chunks(2) {
            if p.len() < 2 {
                return Err(ErrorKind::Value);
            }
            conds.push((e.array_of(p[0].clone()), Criterion::new(e, &p[1])?));
        }
        let n = matching(&conds)?.len();
        num(n as f64)
    });
}

fn count(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let mut n = 0u32;
    for v in a {
        match v {
            Value::Ref(_) | Value::Array(_) => {
                n += e
                    .array_of(v.clone())
                    .data
                    .iter()
                    .filter(|x| matches!(x, Value::Number(_)))
                    .count() as u32;
            }
            Value::Empty | Value::Error(_) => {}
            v => n += u32::from(coerce::to_num(v, &e.opts.locale, e.opts.date_system).is_ok()),
        }
    }
    num(f64::from(n))
}

fn nth(e: &mut Engine, a: &[Value], largest: bool) -> Res<Value> {
    let mut v = e.numbers(&a[..1])?;
    let k = e.num(&a[1])?.trunc();
    if k < 1.0 || k as usize > v.len() {
        return Err(ErrorKind::Num);
    }
    v.sort_by(f64::total_cmp);
    let k = k as usize;
    num(if largest { v[v.len() - k] } else { v[k - 1] })
}

fn variance(e: &mut Engine, a: &[Value], sample: bool) -> Res<f64> {
    let v = e.numbers(a)?;
    let n = v.len() as f64;
    if n < if sample { 2.0 } else { 1.0 } {
        return Err(ErrorKind::DivZero);
    }
    let mean = v.iter().sum::<f64>() / n;
    Ok(v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / if sample { n - 1.0 } else { n })
}

// ----- criteria -----

pub(crate) enum Criterion {
    Num(Cmp, f64),
    /// Text criterion: the operator, the matcher for `=`/`<>` and the plain
    /// text for ordering operators.
    Text(Cmp, Matcher, String, bool),
    Blank(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Criterion {
    pub(crate) fn new(e: &mut Engine, v: &Value) -> Res<Self> {
        let v = e.scalar(v.clone());
        match v {
            Value::Error(er) => Err(er),
            Value::Number(n) => Ok(Self::Num(Cmp::Eq, n)),
            Value::Bool(b) => Ok(Self::Num(Cmp::Eq, f64::from(u8::from(b)))),
            Value::Empty => Ok(Self::Num(Cmp::Eq, 0.0)),
            Value::Text(t) => {
                let (op, rest) = [
                    ("<=", Cmp::Le),
                    (">=", Cmp::Ge),
                    ("<>", Cmp::Ne),
                    ("<", Cmp::Lt),
                    (">", Cmp::Gt),
                    ("=", Cmp::Eq),
                ]
                .into_iter()
                .find_map(|(p, c)| t.strip_prefix(p).map(|r| (c, r)))
                .unwrap_or((Cmp::Eq, &t));
                if rest.is_empty() {
                    return Ok(Self::Blank(op != Cmp::Ne));
                }
                match coerce::parse_number(rest, &e.opts.locale, e.opts.date_system) {
                    Some(n) => Ok(Self::Num(op, n)),
                    None => {
                        let s = e.opts.search;
                        Ok(Self::Text(
                            op,
                            Matcher::new(rest, &s, s.whole_cell),
                            rest.to_string(),
                            false,
                        ))
                    }
                }
            }
            _ => Err(ErrorKind::Value),
        }
    }

    pub(crate) fn matches(&self, v: &Value) -> bool {
        match self {
            Self::Blank(want_blank) => {
                let blank =
                    matches!(v, Value::Empty) || matches!(v, Value::Text(t) if t.is_empty());
                blank == *want_blank
            }
            Self::Num(op, n) => match v {
                Value::Number(x) => apply(*op, x.partial_cmp(n)),
                Value::Text(t) if *op == Cmp::Eq || *op == Cmp::Ne => {
                    // Text that looks numeric also matches (`COUNTIF(r,"1")`).
                    let loose = t.trim().parse::<f64>().ok();
                    apply(*op, loose.and_then(|x| x.partial_cmp(n)))
                }
                Value::Empty if *op == Cmp::Ne => true,
                _ => false,
            },
            Self::Text(op, m, pat, cs) => match v {
                Value::Text(t) => match op {
                    Cmp::Eq => m.is_match(t),
                    Cmp::Ne => !m.is_match(t),
                    _ => apply(
                        *op,
                        Some(if *cs {
                            t.as_ref().cmp(pat.as_str())
                        } else {
                            cmp_text(t, pat)
                        }),
                    ),
                },
                Value::Empty | Value::Number(_) | Value::Bool(_) | Value::Error(_) => {
                    *op == Cmp::Ne
                }
                _ => false,
            },
        }
    }
}

fn apply(op: Cmp, ord: Option<Ordering>) -> bool {
    let Some(o) = ord else { return op == Cmp::Ne };
    match op {
        Cmp::Eq => o == Ordering::Equal,
        Cmp::Ne => o != Ordering::Equal,
        Cmp::Lt => o == Ordering::Less,
        Cmp::Le => o != Ordering::Greater,
        Cmp::Gt => o == Ordering::Greater,
        Cmp::Ge => o != Ordering::Less,
    }
}

#[derive(Clone, Copy)]
enum Agg {
    Sum,
    Avg,
    Max,
    Min,
}

/// Indices of the cells satisfying every `(range, criterion)` pair.
fn matching(conds: &[(Arc<Array>, Criterion)]) -> Res<Vec<usize>> {
    let (rows, cols) = (conds[0].0.rows, conds[0].0.cols);
    if conds.iter().any(|(r, _)| r.rows != rows || r.cols != cols) {
        return Err(ErrorKind::Value);
    }
    Ok((0..conds[0].0.data.len())
        .filter(|&i| conds.iter().all(|(r, c)| c.matches(&r.data[i])))
        .collect())
}

fn reduce(vals: &Array, idx: &[usize], agg: Agg) -> Res<Value> {
    let mut nums = Vec::new();
    for &i in idx {
        match &vals.data[i] {
            Value::Number(n) => nums.push(*n),
            Value::Error(e) => return Err(*e),
            _ => {}
        }
    }
    match agg {
        Agg::Sum => num(nums.iter().sum()),
        Agg::Avg if nums.is_empty() => Err(ErrorKind::DivZero),
        Agg::Avg => num(nums.iter().sum::<f64>() / nums.len() as f64),
        Agg::Max => num(nums.into_iter().reduce(f64::max).unwrap_or(0.0)),
        Agg::Min => num(nums.into_iter().reduce(f64::min).unwrap_or(0.0)),
    }
}

fn conditional(e: &mut Engine, a: &[Value], agg: Agg) -> Res<Value> {
    let crit = Criterion::new(e, &a[1])?;
    let range = e.array_of(a[0].clone());
    let vals = match opt(a, 2) {
        Some(v) => e.array_of(v.clone()),
        None => range.clone(),
    };
    if vals.data.len() != range.data.len() {
        return Err(ErrorKind::Value);
    }
    let idx = matching(&[(range, crit)])?;
    reduce(&vals, &idx, agg)
}

fn multi(e: &mut Engine, a: &[Value], agg: Agg) -> Res<Value> {
    let vals = e.array_of(a[0].clone());
    let mut conds = Vec::new();
    for p in a[1..].chunks(2) {
        if p.len() < 2 {
            return Err(ErrorKind::Value);
        }
        conds.push((e.array_of(p[0].clone()), Criterion::new(e, &p[1])?));
    }
    if conds.iter().any(|(r, _)| r.data.len() != vals.data.len()) {
        return Err(ErrorKind::Value);
    }
    let idx = matching(&conds)?;
    reduce(&vals, &idx, agg)
}

/// Shared with MATCH-style lookups.
pub(crate) fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // Lookups and criteria ignore case whatever the comparison setting says.
        (Value::Text(x), Value::Text(y)) => cmp_text(x, y) == Ordering::Equal,
        (Value::Number(_) | Value::Bool(_), Value::Text(_))
        | (Value::Text(_), Value::Number(_) | Value::Bool(_)) => false,
        _ => compare(a, b, false) == Ordering::Equal,
    }
}
