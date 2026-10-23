//! Value coercions and number/text conversion under a [`Locale`].

use crate::dates;
use crate::options::{DateSystem, Locale};
use crate::value::{ErrorKind, Value};

type Res<T> = Result<T, ErrorKind>;

/// Number → text as the `General` format would show it: 15 significant
/// digits, positional between 1e-7 and 1e15, scientific otherwise
/// (`E+17`). LibreOffice (`lo`) stays positional down to 1e-14 and writes
/// the exponent with three digits (`E+017`).
pub fn fmt_number(x: f64, loc: &Locale, lo: bool) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let sci = format!("{:.14e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let positional = if lo {
        (-14..=15).contains(&exp)
    } else {
        (-7..15).contains(&exp)
    };
    if positional {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
            } else {
                out.push_str(&digits[..int_len]);
                out.push(loc.decimal);
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push('0');
            out.push(loc.decimal);
            out.extend(std::iter::repeat_n('0', (-exp - 1) as usize));
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push(loc.decimal);
            out.push_str(&digits[1..]);
        }
        let width = if lo { 3 } else { 2 };
        out.push_str(&format!(
            "E{}{:0width$}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        ));
    }
    out
}

/// Text → number: locale decimal/group separators, `%` suffix, and ISO dates.
pub fn parse_number(s: &str, loc: &Locale, sys: DateSystem) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (t, pct) = match t.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (t, false),
    };
    let mut clean = String::with_capacity(t.len());
    for ch in t.chars() {
        if ch == loc.group || ch == '\u{a0}' {
            continue;
        }
        clean.push(if ch == loc.decimal { '.' } else { ch });
    }
    let valid = clean
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        && clean.chars().any(|c| c.is_ascii_digit());
    if valid && let Ok(n) = clean.parse::<f64>() {
        return Some(if pct { n / 100.0 } else { n });
    }
    parse_iso_date(t, sys)
}

fn parse_iso_date(t: &str, sys: DateSystem) -> Option<f64> {
    let (date, time) = match t.split_once(['T', ' ']) {
        Some((d, tm)) => (d, Some(tm)),
        None => (t, None),
    };
    let mut it = date.split('-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 {
        return None;
    }
    let base = dates::ymd_to_serial(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?, sys)?;
    let frac = match time {
        None => 0.0,
        Some(tm) => {
            let mut p = tm.split(':');
            let h: f64 = p.next()?.parse().ok()?;
            let mi: f64 = p.next().map_or(Some(0.0), |v| v.parse().ok())?;
            let se: f64 = p.next().map_or(Some(0.0), |v| v.parse().ok())?;
            (h * 3600.0 + mi * 60.0 + se) / 86_400.0
        }
    };
    Some(base + frac)
}

pub fn to_num(v: &Value, loc: &Locale, sys: DateSystem) -> Res<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Empty => Ok(0.0),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Text(s) => parse_number(s, loc, sys).ok_or(ErrorKind::Value),
        Value::Error(e) => Err(*e),
        Value::Array(_) | Value::Ref(_) => Err(ErrorKind::Value),
    }
}

/// `lo`: LibreOffice conventions (booleans are the numbers 1/0 and numbers use
/// its general format).
pub fn to_text(v: &Value, loc: &Locale, lo: bool) -> Res<String> {
    match v {
        Value::Text(s) => Ok(s.to_string()),
        Value::Number(n) => Ok(fmt_number(*n, loc, lo)),
        Value::Bool(b) if lo => Ok(if *b { "1" } else { "0" }.into()),
        Value::Bool(b) => Ok(if *b { &loc.true_text } else { &loc.false_text }.clone()),
        Value::Empty => Ok(String::new()),
        Value::Error(e) => Err(*e),
        Value::Array(_) | Value::Ref(_) => Err(ErrorKind::Value),
    }
}

pub fn to_bool(v: &Value, loc: &Locale) -> Res<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(*n != 0.0),
        Value::Empty => Ok(false),
        Value::Text(s) => {
            if s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case(&loc.true_text) {
                Ok(true)
            } else if s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case(&loc.false_text) {
                Ok(false)
            } else {
                Err(ErrorKind::Value)
            }
        }
        Value::Error(e) => Err(*e),
        Value::Array(_) | Value::Ref(_) => Err(ErrorKind::Value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_format() {
        let en = Locale::en_us();
        assert_eq!(fmt_number(0.1 + 0.2, &en, false), "0.3");
        assert_eq!(fmt_number(1234.5, &en, false), "1234.5");
        assert_eq!(fmt_number(-42.0, &en, false), "-42");
        assert_eq!(fmt_number(1e20, &en, false), "1E+20");
        assert_eq!(fmt_number(1.5e-10, &en, false), "1.5E-10");
        assert_eq!(fmt_number(0.00012, &en, false), "0.00012");
        assert_eq!(fmt_number(1.5, &Locale::es_es(), false), "1,5");
        assert_eq!(fmt_number(100.0, &en, false), "100");
        // LibreOffice: wider positional range, three-digit exponent.
        assert_eq!(fmt_number(1e-10, &en, true), "0.0000000001");
        assert_eq!(fmt_number(1e-15, &en, true), "1E-015");
        assert_eq!(fmt_number(1e20, &en, true), "1E+020");
        assert_eq!(fmt_number(1e16, &en, false), "1E+16");
    }

    #[test]
    fn parsing() {
        let en = Locale::en_us();
        let es = Locale::es_es();
        let s = DateSystem::Excel1900;
        assert_eq!(parse_number(" 1,234.5 ", &en, s), Some(1234.5));
        assert_eq!(parse_number("1.234,5", &es, s), Some(1234.5));
        assert_eq!(parse_number("50%", &en, s), Some(0.5));
        assert_eq!(parse_number("abc", &en, s), None);
        assert_eq!(parse_number("inf", &en, s), None);
        assert_eq!(parse_number("2024-01-01", &en, s), Some(45292.0));
        assert_eq!(parse_number("2024-01-01 12:00", &en, s), Some(45292.5));
    }
}
