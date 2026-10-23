//! Text functions.

use super::{Reg, VAR, num, opt, opt_num};
use crate::coerce;
use crate::engine::{Engine, Res};
use crate::format;
use crate::functions::stats::wildcard;
use crate::value::{ErrorKind, Value};

pub(super) fn register(r: &mut Reg) {
    r.eager("CONCATENATE", 1, VAR, |e, a| concat(e, a, false));
    r.eager("CONCAT", 1, VAR, |e, a| concat(e, a, true));
    r.eager("TEXTJOIN", 3, VAR, |e, a| {
        let sep = e.text(&a[0])?;
        let skip = e.boolean(&a[1])?;
        let mut parts = Vec::new();
        for v in &a[2..] {
            for x in &e.array_of(v.clone()).data {
                let t = coerce::to_text(x, &e.opts.locale)?;
                if !(skip && t.is_empty()) {
                    parts.push(t);
                }
            }
        }
        Ok(Value::text(parts.join(&sep)))
    });
    r.eager("LEN", 1, 1, |e, a| {
        num(e.text(&a[0])?.chars().count() as f64)
    });
    r.eager("UPPER", 1, 1, |e, a| {
        Ok(Value::text(e.text(&a[0])?.to_uppercase()))
    });
    r.eager("LOWER", 1, 1, |e, a| {
        Ok(Value::text(e.text(&a[0])?.to_lowercase()))
    });
    r.eager("PROPER", 1, 1, |e, a| {
        let mut out = String::new();
        let mut start = true;
        for c in e.text(&a[0])?.chars() {
            if c.is_alphabetic() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = false;
            } else {
                out.push(c);
                start = true;
            }
        }
        Ok(Value::text(out))
    });
    r.eager("TRIM", 1, 1, |e, a| {
        let t = e.text(&a[0])?;
        Ok(Value::text(
            t.split(' ')
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        ))
    });
    r.eager("LEFT", 1, 2, |e, a| {
        let t = e.text(&a[0])?;
        let n = count(e, a, 1)?;
        Ok(Value::text(t.chars().take(n).collect::<String>()))
    });
    r.eager("RIGHT", 1, 2, |e, a| {
        let t = e.text(&a[0])?;
        let n = count(e, a, 1)?;
        let len = t.chars().count();
        Ok(Value::text(
            t.chars().skip(len.saturating_sub(n)).collect::<String>(),
        ))
    });
    r.eager("MID", 3, 3, |e, a| {
        let t = e.text(&a[0])?;
        let (s, n) = (e.num(&a[1])?.trunc(), e.num(&a[2])?.trunc());
        if s < 1.0 || n < 0.0 {
            return Err(ErrorKind::Value);
        }
        Ok(Value::text(
            t.chars()
                .skip(s as usize - 1)
                .take(n as usize)
                .collect::<String>(),
        ))
    });
    r.eager("REPT", 2, 2, |e, a| {
        let (t, n) = (e.text(&a[0])?, e.num(&a[1])?.trunc());
        if !(0.0..=32767.0).contains(&n) {
            return Err(ErrorKind::Value);
        }
        Ok(Value::text(t.repeat(n as usize)))
    });
    r.eager("EXACT", 2, 2, |e, a| {
        Ok(Value::Bool(e.text(&a[0])? == e.text(&a[1])?))
    });
    r.eager("FIND", 2, 3, |e, a| find(e, a, true));
    r.eager("SEARCH", 2, 3, |e, a| find(e, a, false));
    r.eager("SUBSTITUTE", 3, 4, |e, a| {
        let (t, old, new) = (e.text(&a[0])?, e.text(&a[1])?, e.text(&a[2])?);
        if old.is_empty() {
            return Ok(Value::text(t));
        }
        match opt(a, 3) {
            None => Ok(Value::text(t.replace(&old, &new))),
            Some(n) => {
                let n = e.num(n)?.trunc();
                if n < 1.0 {
                    return Err(ErrorKind::Value);
                }
                let mut out = String::new();
                let mut rest = t.as_str();
                let mut seen = 0.0;
                while let Some(p) = rest.find(&old) {
                    seen += 1.0;
                    out.push_str(&rest[..p]);
                    out.push_str(if seen == n { &new } else { &old });
                    rest = &rest[p + old.len()..];
                }
                out.push_str(rest);
                Ok(Value::text(out))
            }
        }
    });
    r.eager("REPLACE", 4, 4, |e, a| {
        let t: Vec<char> = e.text(&a[0])?.chars().collect();
        let (s, n) = (e.num(&a[1])?.trunc(), e.num(&a[2])?.trunc());
        let new = e.text(&a[3])?;
        if s < 1.0 || n < 0.0 {
            return Err(ErrorKind::Value);
        }
        let s = (s as usize - 1).min(t.len());
        let end = (s + n as usize).min(t.len());
        Ok(Value::text(
            t[..s].iter().collect::<String>() + &new + &t[end..].iter().collect::<String>(),
        ))
    });
    r.eager("VALUE", 1, 1, |e, a| match e.scalar(a[0].clone()) {
        Value::Text(t) => coerce::parse_number(&t, &e.opts.locale, e.opts.date_system)
            .map_or(Err(ErrorKind::Value), |n| Ok(Value::Number(n))),
        v => num(coerce::to_num(&v, &e.opts.locale, e.opts.date_system)?),
    });
    r.eager("NUMBERVALUE", 1, 3, |e, a| {
        let t = e.text(&a[0])?;
        let dec = opt(a, 1)
            .map(|v| e.text(v))
            .transpose()?
            .and_then(|s| s.chars().next());
        let grp = opt(a, 2)
            .map(|v| e.text(v))
            .transpose()?
            .and_then(|s| s.chars().next());
        let mut loc = e.opts.locale.clone();
        loc.decimal = dec.unwrap_or(loc.decimal);
        loc.group = grp.unwrap_or(loc.group);
        coerce::parse_number(&t, &loc, e.opts.date_system)
            .map_or(Err(ErrorKind::Value), |n| Ok(Value::Number(n)))
    });
    r.eager("T", 1, 1, |e, a| match e.scalar(a[0].clone()) {
        Value::Text(t) => Ok(Value::Text(t)),
        Value::Error(k) => Err(k),
        _ => Ok(Value::text("")),
    });
    r.eager("TEXT", 2, 2, |e, a| {
        let v = e.scalar(a[0].clone());
        let fmt = e.text(&a[1])?;
        match v {
            Value::Text(t) => Ok(Value::Text(t)),
            Value::Error(k) => Err(k),
            Value::Bool(_) => Ok(Value::text(coerce::to_text(&v, &e.opts.locale)?)),
            v => {
                let x = coerce::to_num(&v, &e.opts.locale, e.opts.date_system)?;
                Ok(Value::text(format::format_number(
                    x,
                    &fmt,
                    &e.opts.locale,
                    e.opts.date_system,
                )))
            }
        }
    });
    r.eager("FIXED", 1, 3, |e, a| {
        let x = e.num(&a[0])?;
        let d = opt_num(e, a, 1, 2.0)?.trunc().max(0.0) as usize;
        let no_commas = match opt(a, 2) {
            Some(v) => e.boolean(v)?,
            None => false,
        };
        let mut fmt = String::from(if no_commas { "0" } else { "#,##0" });
        if d > 0 {
            fmt.push('.');
            fmt.push_str(&"0".repeat(d));
        }
        Ok(Value::text(format::format_number(
            x,
            &fmt,
            &e.opts.locale,
            e.opts.date_system,
        )))
    });
    r.eager("CHAR", 1, 1, |e, a| {
        let n = e.num(&a[0])?.trunc();
        if !(1.0..=255.0).contains(&n) {
            return Err(ErrorKind::Value);
        }
        Ok(Value::text(char::from(n as u8).to_string()))
    });
    r.eager("CODE", 1, 1, |e, a| {
        let t = e.text(&a[0])?;
        t.chars()
            .next()
            .map_or(Err(ErrorKind::Value), |c| num(f64::from(u32::from(c))))
    });
}

fn count(e: &mut Engine, a: &[Value], i: usize) -> Res<usize> {
    let n = opt_num(e, a, i, 1.0)?.trunc();
    if n < 0.0 {
        Err(ErrorKind::Value)
    } else {
        Ok(n as usize)
    }
}

fn concat(e: &mut Engine, a: &[Value], flatten: bool) -> Res<Value> {
    let mut out = String::new();
    for v in a {
        if flatten && matches!(v, Value::Ref(_) | Value::Array(_)) {
            for x in &e.array_of(v.clone()).data {
                out.push_str(&coerce::to_text(x, &e.opts.locale)?);
            }
        } else {
            out.push_str(&e.text(v)?);
        }
    }
    Ok(Value::text(out))
}

fn find(e: &mut Engine, a: &[Value], case_sensitive: bool) -> Res<Value> {
    let needle = e.text(&a[0])?;
    let hay: Vec<char> = e.text(&a[1])?.chars().collect();
    let start = opt_num(e, a, 2, 1.0)?.trunc();
    if start < 1.0 || start as usize > hay.len() + 1 {
        return Err(ErrorKind::Value);
    }
    let from = start as usize - 1;
    let nd: Vec<char> = needle.chars().collect();
    for i in from..=hay.len() {
        let hit = if case_sensitive {
            hay[i..].starts_with(&nd)
        } else {
            // SEARCH: wildcards match a window of the same size or longer.
            wildcard_prefix(&needle, &hay[i..])
        };
        if hit {
            return num((i + 1) as f64);
        }
    }
    Err(ErrorKind::Value)
}

/// Whether `pat` (with `*`/`?`) matches some prefix of `text`.
fn wildcard_prefix(pat: &str, text: &[char]) -> bool {
    (0..=text.len()).any(|n| wildcard(pat, &text[..n].iter().collect::<String>()))
}
