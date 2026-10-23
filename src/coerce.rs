//! Value coercions and number/text conversion under a [`Locale`].

use crate::dates;
use crate::options::{DateOrder, DatePatterns, DateSystem, Locale};
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
    if looks_like_numeric_date(t) {
        return parse_date_text(t, loc, sys);
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
    parse_date_text(t, loc, sys)
}

/// A text date or date-time, as typed into a cell: ISO (`2024-03-01`), numeric
/// in the locale's order (`3/1/2024` is March 1st under `en`, January 3rd under
/// `es`; a four-digit first component always means year first), with month
/// names (`1 Mar 2024`, `March 1, 2024`, English or the locale's), optionally
/// followed by a time. Also a time alone (`13:30`, `1:30 PM`), which is a
/// fraction of a day.
pub fn parse_date_text(s: &str, loc: &Locale, sys: DateSystem) -> Option<f64> {
    let t = s.trim();
    if let Some(tm) = parse_time(t) {
        return Some(tm);
    }
    // Rightmost split whose tail is a time.
    for (i, ch) in t.char_indices().rev() {
        if (ch == ' ' || ch == 'T')
            && i > 0
            && let Some(tm) = parse_time(t[i + ch.len_utf8()..].trim())
        {
            return parse_date_only(t[..i].trim(), loc, sys).map(|d| d + tm);
        }
    }
    parse_date_only(t, loc, sys)
}

fn parse_time(s: &str) -> Option<f64> {
    let up = s.trim().to_ascii_uppercase();
    let (body, pm) = if let Some(b) = up.strip_suffix("PM") {
        (b.trim(), Some(true))
    } else if let Some(b) = up.strip_suffix("AM") {
        (b.trim(), Some(false))
    } else {
        (up.as_str(), None)
    };
    if !body.contains(':') {
        return None;
    }
    let mut p = body.split(':');
    let h: f64 = p.next()?.parse().ok()?;
    let m: f64 = p.next()?.parse().ok()?;
    let sec: f64 = p.next().map_or(Some(0.0), |v| v.parse().ok())?;
    if p.next().is_some() || m >= 60.0 || sec >= 60.0 || h.is_nan() || h < 0.0 {
        return None;
    }
    let h = match pm {
        Some(is_pm) if (1.0..=12.0).contains(&h) => h % 12.0 + if is_pm { 12.0 } else { 0.0 },
        Some(_) => return None,
        None if h < 24.0 => h,
        None => return None,
    };
    Some((h * 3600.0 + m * 60.0 + sec) / 86_400.0)
}

/// Complete a two-digit year inside the 100-year window starting at `start`.
pub fn expand_year(y: i64, start: u16) -> i64 {
    if y >= 100 {
        return y;
    }
    let start = i64::from(start);
    let cand = start / 100 * 100 + y;
    if cand < start { cand + 100 } else { cand }
}

fn month_from_name(tok: &str, loc: &Locale) -> Option<i64> {
    if tok.chars().count() < 3 {
        return None;
    }
    let t = tok.trim_end_matches('.').to_lowercase();
    let english = Locale::en_us();
    for names in [&loc.months, &english.months] {
        for (i, n) in names.iter().enumerate() {
            let n = n.to_lowercase();
            let short: String = n.chars().take(3).collect();
            if t == n || t == short || (t.len() >= 3 && n.starts_with(&t)) {
                return Some(i as i64 + 1);
            }
        }
    }
    None
}

fn parse_date_only(d: &str, loc: &Locale, sys: DateSystem) -> Option<f64> {
    if loc.date_patterns == DatePatterns::Libre {
        let has_name = d.chars().any(char::is_alphabetic);
        // `1 Mar 2024` and `2024/03/01` are not dates to LibreOffice; month
        // names need `-` (`1-Mar-2024`) or a comma (`Mar 1, 2024`).
        if (has_name && !(d.contains(',') || (d.contains('-') && !d.contains(' '))))
            || (d.contains('/') && d.split('/').next().is_some_and(|f| f.len() >= 3))
        {
            return None;
        }
    }
    let toks: Vec<&str> = d
        .split(['/', '.', '-', ',', ' '])
        .filter(|t| !t.is_empty())
        .collect();
    let num = |t: &str| -> Option<i64> {
        (t.chars().all(|c| c.is_ascii_digit()) && t.len() <= 4).then(|| t.parse().ok())?
    };
    let (y, m, day) = match toks.as_slice() {
        [a, b, c] if num(a).is_some() && num(b).is_some() && num(c).is_some() => {
            let (a, b, c) = (num(a)?, num(b)?, num(c)?);
            let first_is_year = toks[0].len() >= 3;
            if first_is_year {
                (a, b, c)
            } else {
                match loc.date_order {
                    DateOrder::Mdy => (c, a, b),
                    DateOrder::Dmy => (c, b, a),
                    DateOrder::Ymd => (a, b, c),
                }
            }
        }
        [a, b, c] => {
            // One component is a month name; the others are day and year.
            let (mi, month) = [a, b, c]
                .iter()
                .enumerate()
                .find_map(|(i, t)| month_from_name(t, loc).map(|m| (i, m)))?;
            let rest: Vec<i64> = [a, b, c]
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != mi)
                .map(|(_, t)| num(t))
                .collect::<Option<_>>()?;
            let year_first = toks
                .iter()
                .enumerate()
                .find(|(i, _)| *i != mi)
                .is_some_and(|(_, t)| t.len() >= 3);
            if year_first {
                (rest[0], month, rest[1])
            } else {
                (rest[1], month, rest[0])
            }
        }
        [a, b] => {
            let (mi, month) = [a, b]
                .iter()
                .enumerate()
                .find_map(|(i, t)| month_from_name(t, loc).map(|m| (i, m)))?;
            (num([a, b][1 - mi])?, month, 1)
        }
        _ => return None,
    };
    let y = expand_year(y, loc.two_digit_year_start);
    if !(1..=12).contains(&m) || day < 1 || day > dates::days_in_month(y, m) {
        return None;
    }
    dates::ymd_to_serial(y, m, day, sys)
}

/// `a/b/c`, `a-b-c` or `a.b.c` with numeric parts: read as a date before any
/// attempt to read it as a number (`3.1.2024` is not 312024 under `de`).
fn looks_like_numeric_date(s: &str) -> bool {
    let sep = s.chars().find(|c| matches!(c, '/' | '.' | '-'));
    let Some(sep) = sep else { return false };
    let parts: Vec<&str> = s.split(sep).collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))
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
        // Text dates follow the locale.
        assert_eq!(parse_number("3/1/2024", &en, s), Some(45352.0)); // March 1st
        assert_eq!(parse_number("3/1/2024", &es, s), Some(45294.0)); // January 3rd
        assert_eq!(parse_number("3.1.2024", &Locale::de_de(), s), Some(45294.0));
        assert_eq!(parse_number("1 Mar 2024", &en, s), Some(45352.0));
        assert_eq!(parse_number("March 1, 2024", &en, s), Some(45352.0));
        assert_eq!(parse_number("1 marzo 2024", &es, s), Some(45352.0));
        assert_eq!(parse_number("3/1/24", &en, s), Some(45352.0));
        assert_eq!(parse_number("3/1/99", &en, s), Some(36220.0)); // 1999: window 1930–2029
        assert_eq!(parse_number("2/30/2024", &en, s), None);
        assert_eq!(
            parse_number("3/1/2024 1:30 PM", &en, s),
            Some(45352.0 + 13.5 / 24.0)
        );
        assert_eq!(parse_number("13:30", &en, s), Some(13.5 / 24.0));
        assert_eq!(parse_number("25:00", &en, s), None);
        assert_eq!(expand_year(29, 1930), 2029);
        assert_eq!(expand_year(30, 1930), 1930);
        assert_eq!(expand_year(5, 1950), 2005);
    }
}
