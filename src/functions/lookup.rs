//! Lookup and reference functions.

use super::stats::{values_equal, wildcard};
use super::{Reg, num, opt, opt_num};
use crate::addr::{self, MAX_COLS, MAX_ROWS, Rect};
use crate::engine::{Engine, Res};
use crate::value::{Array, ErrorKind, Value, compare};
use std::cmp::Ordering;
use std::sync::Arc;

pub(super) fn register(r: &mut Reg) {
    r.eager("MATCH", 2, 3, |e, a| {
        let needle = e.scalar(a[0].clone());
        let hay = e.array_of(a[1].clone());
        let kind = opt_num(e, a, 2, 1.0)?;
        position(&needle, &hay.data, kind).map_or(Err(ErrorKind::Na), |p| num((p + 1) as f64))
    })
    .array_args();
    r.eager("INDEX", 2, 4, index).array_args();
    r.eager("VLOOKUP", 3, 4, |e, a| table_lookup(e, a, false));
    r.eager("HLOOKUP", 3, 4, |e, a| table_lookup(e, a, true));
    r.eager("XLOOKUP", 3, 6, xlookup).array_args();
    r.eager("OFFSET", 3, 5, offset).dynamic();
    r.eager("INDIRECT", 1, 2, indirect).dynamic();
    r.eager("_RANGE", 2, 2, |_, a| match (&a[0], &a[1]) {
        (Value::Ref(x), Value::Ref(y)) if x.sheet == y.sheet => Ok(Value::Ref(Rect::new(
            x.sheet,
            x.r0.min(y.r0),
            x.c0.min(y.c0),
            x.r1.max(y.r1),
            x.c1.max(y.c1),
        ))),
        _ => Err(ErrorKind::Ref),
    })
    .dynamic();
    r.eager("ROW", 0, 1, |e, a| match a.first() {
        None => num(f64::from(e.at.row + 1)),
        Some(Value::Ref(r)) => num(f64::from(r.r0 + 1)),
        Some(_) => Err(ErrorKind::Value),
    });
    r.eager("COLUMN", 0, 1, |e, a| match a.first() {
        None => num(f64::from(e.at.col + 1)),
        Some(Value::Ref(r)) => num(f64::from(r.c0 + 1)),
        Some(_) => Err(ErrorKind::Value),
    });
    r.eager("ROWS", 1, 1, |_, a| match &a[0] {
        Value::Ref(r) => num(f64::from(r.rows())),
        Value::Array(m) => num(f64::from(m.rows)),
        _ => num(1.0),
    });
    r.eager("COLUMNS", 1, 1, |_, a| match &a[0] {
        Value::Ref(r) => num(f64::from(r.cols())),
        Value::Array(m) => num(f64::from(m.cols)),
        _ => num(1.0),
    });
    r.eager("ADDRESS", 2, 5, address);
}

/// Position (0-based) of `needle` in `hay` for a MATCH `kind` (0 exact,
/// 1 largest ≤ in ascending data, -1 smallest ≥ in descending data).
fn position(needle: &Value, hay: &[Value], kind: f64) -> Option<usize> {
    if kind == 0.0 {
        if let Value::Text(p) = needle
            && p.contains(['*', '?', '~'])
        {
            return hay
                .iter()
                .position(|v| matches!(v, Value::Text(t) if wildcard(p, t)));
        }
        return hay.iter().position(|v| values_equal(needle, v));
    }
    let same_class = |v: &Value| {
        matches!(
            (needle, v),
            (Value::Number(_), Value::Number(_))
                | (Value::Text(_), Value::Text(_))
                | (Value::Bool(_), Value::Bool(_))
        )
    };
    let mut best = None;
    for (i, v) in hay.iter().enumerate() {
        if !same_class(v) {
            continue;
        }
        let ord = compare(v, needle);
        let ok = if kind > 0.0 {
            ord != Ordering::Greater
        } else {
            ord != Ordering::Less
        };
        if ok {
            best = Some(i);
        } else {
            break;
        }
    }
    best
}

fn index(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let row = opt(a, 1).map(|v| e.num(v)).transpose()?;
    let col = opt(a, 2).map(|v| e.num(v)).transpose()?;
    let (rows, cols) = match &a[0] {
        Value::Ref(r) => (r.rows(), r.cols()),
        Value::Array(m) => (m.rows, m.cols),
        _ => (1, 1),
    };
    let (row, col) = match (row, col) {
        (Some(r), None) if cols == 1 => (r, 1.0),
        (Some(r), None) if rows == 1 => (1.0, r),
        (r, c) => (r.unwrap_or(0.0), c.unwrap_or(0.0)),
    };
    if row < 0.0 || col < 0.0 || row > f64::from(rows) || col > f64::from(cols) {
        return Err(ErrorKind::Ref);
    }
    let (row, col) = (row as u32, col as u32);
    let (r0, r1) = if row == 0 {
        (0, rows - 1)
    } else {
        (row - 1, row - 1)
    };
    let (c0, c1) = if col == 0 {
        (0, cols - 1)
    } else {
        (col - 1, col - 1)
    };
    match &a[0] {
        Value::Ref(r) => Ok(Value::Ref(Rect::new(
            r.sheet,
            r.r0 + r0,
            r.c0 + c0,
            r.r0 + r1,
            r.c0 + c1,
        ))),
        other => {
            let m = e.array_of(other.clone());
            let (h, w) = (r1 - r0 + 1, c1 - c0 + 1);
            let mut data = Vec::with_capacity((h * w) as usize);
            for i in r0..=r1 {
                for j in c0..=c1 {
                    data.push(m.get(i, j).clone());
                }
            }
            Ok(if data.len() == 1 {
                data.remove(0)
            } else {
                Value::Array(Arc::new(Array::new(h, w, data)))
            })
        }
    }
}

fn table_lookup(e: &mut Engine, a: &[Value], horizontal: bool) -> Res<Value> {
    let needle = e.scalar(a[0].clone());
    if let Value::Error(k) = needle {
        return Err(k);
    }
    let t = e.array_of(a[1].clone());
    let idx = e.num(&a[2])?.trunc();
    let approx = match opt(a, 3) {
        Some(v) => e.boolean(v)?,
        None => !matches!(a.get(3), Some(Value::Empty)),
    };
    let (lanes, depth) = if horizontal {
        (t.cols, t.rows)
    } else {
        (t.rows, t.cols)
    };
    if idx < 1.0 {
        return Err(ErrorKind::Value);
    }
    if idx as u32 > depth {
        return Err(ErrorKind::Ref);
    }
    let key = |i: u32| {
        if horizontal {
            t.get(0, i).clone()
        } else {
            t.get(i, 0).clone()
        }
    };
    let keys: Vec<Value> = (0..lanes).map(key).collect();
    let pos = position(&needle, &keys, if approx { 1.0 } else { 0.0 }).ok_or(ErrorKind::Na)? as u32;
    let k = idx as u32 - 1;
    Ok(if horizontal {
        t.get(k, pos).clone()
    } else {
        t.get(pos, k).clone()
    })
}

fn xlookup(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let needle = e.scalar(a[0].clone());
    let hay = e.array_of(a[1].clone());
    let mode = opt_num(e, a, 4, 0.0)?;
    let reverse = opt_num(e, a, 5, 1.0)? < 0.0;
    let mut order: Vec<usize> = (0..hay.data.len()).collect();
    if reverse {
        order.reverse();
    }
    let pick = |i: usize| hay.data[i].clone();
    let found = match mode as i32 {
        0 => order
            .iter()
            .copied()
            .find(|&i| values_equal(&needle, &hay.data[i])),
        2 => match &needle {
            Value::Text(p) => order
                .iter()
                .copied()
                .find(|&i| matches!(&hay.data[i], Value::Text(t) if wildcard(p, t))),
            _ => order
                .iter()
                .copied()
                .find(|&i| values_equal(&needle, &hay.data[i])),
        },
        m @ (-1 | 1) => {
            // Exact, else nearest smaller (-1) / larger (1).
            let mut best: Option<usize> = None;
            for &i in &order {
                let v = pick(i);
                if !matches!(
                    (&needle, &v),
                    (Value::Number(_), Value::Number(_)) | (Value::Text(_), Value::Text(_))
                ) {
                    continue;
                }
                match compare(&v, &needle) {
                    Ordering::Equal => {
                        best = Some(i);
                        break;
                    }
                    o if (m == -1 && o == Ordering::Less) || (m == 1 && o == Ordering::Greater) => {
                        let better = best.is_none_or(|b| {
                            let ob = compare(&v, &hay.data[b]);
                            if m == -1 {
                                ob == Ordering::Greater
                            } else {
                                ob == Ordering::Less
                            }
                        });
                        if better {
                            best = Some(i);
                        }
                    }
                    _ => {}
                }
            }
            best
        }
        _ => return Err(ErrorKind::Value),
    };
    let Some(i) = found else {
        return match a.get(3) {
            Some(v) if !matches!(v, Value::Empty) => Ok(v.clone()),
            _ => Err(ErrorKind::Na),
        };
    };
    // The match index applies along the orientation of the lookup array.
    let vertical = hay.cols == 1;
    match &a[2] {
        Value::Ref(r) => {
            let i = i as u32;
            Ok(Value::Ref(if vertical {
                Rect::new(r.sheet, r.r0 + i, r.c0, r.r0 + i, r.c1)
            } else {
                Rect::new(r.sheet, r.r0, r.c0 + i, r.r1, r.c0 + i)
            }))
        }
        other => {
            let m = e.array_of(other.clone());
            let i = i as u32;
            if vertical && m.cols == 1 || !vertical && m.rows == 1 {
                Ok(m.data.get(i as usize).cloned().ok_or(ErrorKind::Ref)?)
            } else if vertical {
                let data: Vec<Value> = (0..m.cols).map(|j| m.get(i, j).clone()).collect();
                Ok(Value::Array(Arc::new(Array::new(1, m.cols, data))))
            } else {
                let data: Vec<Value> = (0..m.rows).map(|j| m.get(j, i).clone()).collect();
                Ok(Value::Array(Arc::new(Array::new(m.rows, 1, data))))
            }
        }
    }
}

fn offset(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let Value::Ref(base) = &a[0] else {
        return Err(ErrorKind::Value);
    };
    let dr = e.num(&a[1])?.trunc() as i64;
    let dc = e.num(&a[2])?.trunc() as i64;
    let h = opt_num(e, a, 3, f64::from(base.rows()))?.trunc() as i64;
    let w = opt_num(e, a, 4, f64::from(base.cols()))?.trunc() as i64;
    let (r0, c0) = (i64::from(base.r0) + dr, i64::from(base.c0) + dc);
    let (r1, c1) = (r0 + h - 1, c0 + w - 1);
    if h < 1 || w < 1 || r0 < 0 || c0 < 0 || r1 >= i64::from(MAX_ROWS) || c1 >= i64::from(MAX_COLS)
    {
        return Err(ErrorKind::Ref);
    }
    Ok(Value::Ref(Rect::new(
        base.sheet, r0 as u32, c0 as u32, r1 as u32, c1 as u32,
    )))
}

fn indirect(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let text = e.text(&a[0])?;
    if let Some(v) = opt(a, 1)
        && !e.boolean(v)?
    {
        return Err(ErrorKind::Ref); // R1C1 notation is not supported.
    }
    let (sheet, tail) = addr::split_sheet(text.trim());
    let sheet = match sheet {
        Some(n) => e.sheet_id(&n).ok_or(ErrorKind::Ref)?,
        None => e.at.sheet,
    };
    addr::parse_area(tail)
        .map(|ar| Value::Ref(ar.bind(sheet)))
        .ok_or(ErrorKind::Ref)
}

fn address(e: &mut Engine, a: &[Value]) -> Res<Value> {
    let (r, c) = (e.num(&a[0])?.trunc(), e.num(&a[1])?.trunc());
    if r < 1.0 || c < 1.0 || r > f64::from(MAX_ROWS) || c > f64::from(MAX_COLS) {
        return Err(ErrorKind::Value);
    }
    let abs = opt_num(e, a, 2, 1.0)? as i32;
    let a1 = match opt(a, 3) {
        Some(v) => e.boolean(v)?,
        None => true,
    };
    let (row_abs, col_abs) = match abs {
        1 => (true, true),
        2 => (true, false),
        3 => (false, true),
        4 => (false, false),
        _ => return Err(ErrorKind::Value),
    };
    let mut s = String::new();
    if let Some(v) = opt(a, 4) {
        let name = e.text(v)?;
        if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            s.push_str(&name);
        } else {
            s.push_str(&format!("'{}'", name.replace('\'', "''")));
        }
        s.push('!');
    }
    let (r, c) = (r as u32, c as u32);
    if a1 {
        s.push_str(&format!(
            "{}{}{}{}",
            if col_abs { "$" } else { "" },
            addr::col_name(c - 1),
            if row_abs { "$" } else { "" },
            r
        ));
    } else {
        let rr = if row_abs {
            format!("R{r}")
        } else {
            format!("R[{}]", i64::from(r) - i64::from(e.at.row + 1))
        };
        let cc = if col_abs {
            format!("C{c}")
        } else {
            format!("C[{}]", i64::from(c) - i64::from(e.at.col + 1))
        };
        s.push_str(&rr);
        s.push_str(&cc);
    }
    Ok(Value::text(s))
}
