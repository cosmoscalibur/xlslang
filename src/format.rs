//! Number and date formatting for `TEXT` and friends (the common subset of the
//! format-code language, rendered through a [`Locale`]).

use crate::coerce::fmt_number;
use crate::dates;
use crate::functions::round_decimal;
use crate::options::{DateSystem, Locale};

/// Split a format code at top-level `;`.
fn sections(fmt: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), 0, false);
    for (i, c) in fmt.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                out.push(&fmt[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&fmt[start..]);
    out
}

#[derive(Debug, PartialEq)]
enum Tok {
    Lit(String),
    Y(usize),
    M(usize),
    D(usize),
    H(usize),
    S(usize),
    Ampm(bool),
}

fn tokenize_dt(fmt: &str) -> Option<Vec<Tok>> {
    let cs: Vec<char> = fmt.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut dt = false;
    while i < cs.len() {
        let c = cs[i];
        let lc = c.to_ascii_lowercase();
        let run = cs[i..]
            .iter()
            .take_while(|x| x.to_ascii_lowercase() == lc)
            .count();
        match lc {
            '"' => {
                let end = cs[i + 1..]
                    .iter()
                    .position(|&x| x == '"')
                    .map_or(cs.len(), |p| i + 1 + p);
                out.push(Tok::Lit(cs[i + 1..end].iter().collect()));
                i = end + 1;
            }
            '\\' => {
                if let Some(&n) = cs.get(i + 1) {
                    out.push(Tok::Lit(n.to_string()));
                }
                i += 2;
            }
            'y' => {
                dt = true;
                out.push(Tok::Y(run));
                i += run;
            }
            'm' => {
                dt = true;
                out.push(Tok::M(run));
                i += run;
            }
            'd' => {
                dt = true;
                out.push(Tok::D(run));
                i += run;
            }
            'h' => {
                dt = true;
                out.push(Tok::H(run));
                i += run;
            }
            's' => {
                dt = true;
                out.push(Tok::S(run));
                i += run;
            }
            'a' if cs[i..]
                .iter()
                .take(5)
                .collect::<String>()
                .eq_ignore_ascii_case("AM/PM") =>
            {
                dt = true;
                out.push(Tok::Ampm(true));
                i += 5;
            }
            _ => {
                out.push(Tok::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    dt.then_some(out)
}

fn render_dt(toks: &[Tok], serial: f64, loc: &Locale, sys: DateSystem) -> Option<String> {
    let (y, mo, d) = dates::serial_to_ymd(serial, sys)?;
    let secs = ((serial - serial.floor()) * 86_400.0).round() as i64;
    let (hh, mi, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let twelve = toks.iter().any(|t| matches!(t, Tok::Ampm(_)));
    let dow = dates::weekday_sun0(serial, sys) as usize;
    let mut out = String::new();
    for (k, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Y(n) => {
                if *n <= 2 {
                    out.push_str(&format!("{:02}", y % 100));
                } else {
                    out.push_str(&format!("{y:04}"));
                }
            }
            Tok::M(n) => {
                // `m` right after hours or right before seconds means minutes.
                let after_h = toks[..k]
                    .iter()
                    .rev()
                    .find(|t| !matches!(t, Tok::Lit(_)))
                    .is_some_and(|t| matches!(t, Tok::H(_)));
                let before_s = toks[k + 1..]
                    .iter()
                    .find(|t| !matches!(t, Tok::Lit(_)))
                    .is_some_and(|t| matches!(t, Tok::S(_)));
                if after_h || before_s {
                    out.push_str(&if *n >= 2 {
                        format!("{mi:02}")
                    } else {
                        mi.to_string()
                    });
                } else {
                    let name = &loc.months[(mo - 1) as usize];
                    match n {
                        1 => out.push_str(&mo.to_string()),
                        2 => out.push_str(&format!("{mo:02}")),
                        3 => out.extend(name.chars().take(3)),
                        _ => out.push_str(name),
                    }
                }
            }
            Tok::D(n) => match n {
                1 => out.push_str(&d.to_string()),
                2 => out.push_str(&format!("{d:02}")),
                3 => out.extend(loc.days[dow].chars().take(3)),
                _ => out.push_str(&loc.days[dow]),
            },
            Tok::H(n) => {
                let h = if twelve { (hh + 11) % 12 + 1 } else { hh };
                out.push_str(&if *n >= 2 {
                    format!("{h:02}")
                } else {
                    h.to_string()
                });
            }
            Tok::S(n) => out.push_str(&if *n >= 2 {
                format!("{ss:02}")
            } else {
                ss.to_string()
            }),
            Tok::Ampm(_) => out.push_str(if hh < 12 { "AM" } else { "PM" }),
        }
    }
    Some(out)
}

fn group_digits(int: &str, sep: char) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn render_num(section: &str, x: f64, loc: &Locale, neg_prefix: bool) -> String {
    let cs: Vec<char> = section.chars().collect();
    let is_pat = |c: char| matches!(c, '0' | '#' | '?' | ',' | '.');
    let first = cs.iter().position(|&c| is_pat(c));
    let last = cs.iter().rposition(|&c| is_pat(c));
    let lit = |slice: &[char]| {
        let mut s = String::new();
        let mut i = 0;
        while i < slice.len() {
            match slice[i] {
                '"' => {
                    i += 1;
                    while i < slice.len() && slice[i] != '"' {
                        s.push(slice[i]);
                        i += 1;
                    }
                }
                '\\' => {
                    i += 1;
                    if let Some(&c) = slice.get(i) {
                        s.push(c);
                    }
                }
                '%' | '_' | '*' => {}
                '$' => s.push_str(&loc.currency),
                c => s.push(c),
            }
            i += 1;
        }
        s
    };
    let pct = cs.iter().filter(|&&c| c == '%').count();
    let x = x * 100f64.powi(pct as i32);
    let (head, body, tail) = match (first, last) {
        (Some(f), Some(l)) => (lit(&cs[..f]), &cs[f..=l], lit(&cs[l + 1..])),
        _ => (lit(&cs), &cs[..0], String::new()),
    };
    let pct_sign = "%".repeat(pct);
    if body.is_empty() {
        return format!(
            "{}{head}{tail}{pct_sign}",
            if neg_prefix { "-" } else { "" }
        );
    }
    let pat: String = body.iter().collect();
    let (int_pat, dec_pat) = pat.split_once('.').unwrap_or((&pat, ""));
    let thousands = int_pat.contains(',');
    let int_pat: String = int_pat.chars().filter(|&c| c != ',').collect();
    let decs = dec_pat
        .chars()
        .filter(|&c| matches!(c, '0' | '#' | '?'))
        .count();
    let min_decs = dec_pat.chars().filter(|&c| c == '0').count();
    let rounded = round_decimal(x.abs(), decs as i32);
    let s = format!("{rounded:.decs$}");
    let (int, dec) = s.split_once('.').unwrap_or((&s, ""));
    let mut dec = dec.to_string();
    while dec.len() > min_decs && dec.ends_with('0') {
        dec.pop();
    }
    let mut int = int.to_string();
    let min_int = int_pat.chars().filter(|&c| c == '0').count();
    if int == "0" && min_int == 0 {
        int.clear();
    }
    while int.len() < min_int {
        int.insert(0, '0');
    }
    if thousands {
        int = group_digits(&int, loc.group);
    }
    let mut out = String::new();
    if neg_prefix && (rounded != 0.0) {
        out.push('-');
    }
    out.push_str(&head);
    out.push_str(&int);
    if !dec.is_empty() {
        out.push(loc.decimal);
        out.push_str(&dec);
    }
    out.push_str(&pct_sign);
    out.push_str(&tail);
    out
}

/// Format a number with an Excel-style format code. `General` (or an empty
/// code) uses the general number format.
pub fn format_number(x: f64, fmt: &str, loc: &Locale, sys: DateSystem) -> String {
    if fmt.trim().is_empty() || fmt.trim().eq_ignore_ascii_case("general") {
        return fmt_number(x, loc);
    }
    let secs = sections(fmt);
    let (section, neg_prefix) = match (secs.len(), x) {
        (1, v) => (secs[0], v < 0.0),
        (_, v) if v < 0.0 => (secs[1], false),
        (n, v) if v == 0.0 && n >= 3 => (secs[2], false),
        _ => (secs[0], false),
    };
    if let Some(toks) = tokenize_dt(section)
        && let Some(s) = render_dt(&toks, x.abs(), loc, sys)
    {
        return if neg_prefix { format!("-{s}") } else { s };
    }
    render_num(section, x, loc, neg_prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: f64, fmt: &str) -> String {
        format_number(x, fmt, &Locale::en_us(), DateSystem::Excel1900)
    }

    #[test]
    fn numbers() {
        assert_eq!(f(1234.567, "0.00"), "1234.57");
        assert_eq!(f(1234.567, "#,##0.00"), "1,234.57");
        assert_eq!(f(0.256, "0%"), "26%");
        assert_eq!(f(5.0, "000"), "005");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(-3.5, "0.0"), "-3.5");
        assert_eq!(f(-3.5, "0.0;(0.0)"), "(3.5)");
        assert_eq!(f(12.0, "\"EUR \"0"), "EUR 12");
        assert_eq!(f(1.5, "General"), "1.5");
        assert_eq!(
            format_number(1234.5, "#,##0.00", &Locale::es_es(), DateSystem::Excel1900),
            "1.234,50"
        );
    }

    #[test]
    fn dates() {
        assert_eq!(f(45292.0, "yyyy-mm-dd"), "2024-01-01");
        assert_eq!(f(45292.5, "yyyy-mm-dd hh:mm:ss"), "2024-01-01 12:00:00");
        assert_eq!(f(45292.0, "dddd d mmmm yyyy"), "Monday 1 January 2024");
        assert_eq!(f(45292.75, "h:mm AM/PM"), "6:00 PM");
        assert_eq!(
            format_number(
                45292.0,
                "dddd d mmmm",
                &Locale::es_es(),
                DateSystem::Excel1900
            ),
            "lunes 1 enero"
        );
    }
}
