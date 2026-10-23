//! Cell addressing: zero-based coordinates, A1 text parsing and formatting.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_ROWS: u32 = 1_048_576;
pub const MAX_COLS: u32 = 16_384;

/// A cell of a sheet. `sheet` indexes `Program::sheets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CellRef {
    pub sheet: u16,
    pub row: u32,
    pub col: u32,
}

impl CellRef {
    pub fn new(sheet: u16, row: u32, col: u32) -> Self {
        Self { sheet, row, col }
    }
}

/// Inclusive, normalized rectangle (`r0 <= r1`, `c0 <= c1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub sheet: u16,
    pub r0: u32,
    pub c0: u32,
    pub r1: u32,
    pub c1: u32,
}

impl Rect {
    pub fn new(sheet: u16, ra: u32, ca: u32, rb: u32, cb: u32) -> Self {
        Self {
            sheet,
            r0: ra.min(rb),
            c0: ca.min(cb),
            r1: ra.max(rb),
            c1: ca.max(cb),
        }
    }

    pub fn single(c: CellRef) -> Self {
        Self::new(c.sheet, c.row, c.col, c.row, c.col)
    }

    pub fn rows(&self) -> u32 {
        self.r1 - self.r0 + 1
    }

    pub fn cols(&self) -> u32 {
        self.c1 - self.c0 + 1
    }

    pub fn area(&self) -> u64 {
        u64::from(self.rows()) * u64::from(self.cols())
    }

    pub fn is_single(&self) -> bool {
        self.r0 == self.r1 && self.c0 == self.c1
    }

    pub fn contains(&self, c: CellRef) -> bool {
        c.sheet == self.sheet
            && (self.r0..=self.r1).contains(&c.row)
            && (self.c0..=self.c1).contains(&c.col)
    }

    pub fn top_left(&self) -> CellRef {
        CellRef::new(self.sheet, self.r0, self.c0)
    }
}

/// Sheet, defined-name and function names are case-insensitive, for every
/// script (`Hoja Ñ` and `HOJA ñ` are the same sheet).
pub fn same_name(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// Column letters to a 0-based index (`A` → 0). `None` unless 1–3 ASCII letters
/// within `XFD`.
pub fn col_index(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 3 || !s.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let n = s.bytes().fold(0u32, |a, b| {
        a * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    (n <= MAX_COLS).then(|| n - 1)
}

/// 0-based column index to letters, allocation free.
pub fn col_name(index: u32) -> impl fmt::Display {
    fmt::from_fn(move |f| {
        let mut i = index;
        let mut buf = [0u8; 4];
        let mut n = 0;
        i += 1;
        while i > 0 {
            i -= 1;
            buf[n] = b'A' + (i % 26) as u8;
            n += 1;
            i /= 26;
        }
        buf[..n].reverse();
        f.write_str(std::str::from_utf8(&buf[..n]).unwrap_or("?"))
    })
}

/// `A1` text for a zero-based position.
pub fn a1(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// An area as written in a formula, before it is bound to a sheet. Open ends
/// (`A:A`, `1:3`) are already expanded to the sheet limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub r0: u32,
    pub c0: u32,
    pub r1: u32,
    pub c1: u32,
}

impl Area {
    pub fn bind(self, sheet: u16) -> Rect {
        Rect::new(sheet, self.r0, self.c0, self.r1, self.c1)
    }
}

fn parse_cell(s: &str) -> Option<(u32, u32)> {
    let s = s.strip_prefix('$').unwrap_or(s);
    let split = s.find(|c: char| !c.is_ascii_alphabetic())?;
    let (letters, digits) = s.split_at(split);
    let digits = digits.strip_prefix('$').unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: u32 = digits.parse().ok()?;
    if row == 0 || row > MAX_ROWS {
        return None;
    }
    Some((row - 1, col_index(letters)?))
}

fn parse_col(s: &str) -> Option<u32> {
    col_index(s.strip_prefix('$').unwrap_or(s))
}

fn parse_row(s: &str) -> Option<u32> {
    let s = s.strip_prefix('$').unwrap_or(s);
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let r: u32 = s.parse().ok()?;
    (1..=MAX_ROWS).contains(&r).then(|| r - 1)
}

/// Parse `A1`, `$A$1`, `A1:B2`, `A:C` or `2:5` (no sheet qualifier).
pub fn parse_area(s: &str) -> Option<Area> {
    match s.split_once(':') {
        None => {
            let (r, c) = parse_cell(s)?;
            Some(Area {
                r0: r,
                c0: c,
                r1: r,
                c1: c,
            })
        }
        Some((a, b)) => {
            if let (Some((ra, ca)), Some((rb, cb))) = (parse_cell(a), parse_cell(b)) {
                return Some(Area {
                    r0: ra.min(rb),
                    c0: ca.min(cb),
                    r1: ra.max(rb),
                    c1: ca.max(cb),
                });
            }
            if let (Some(ca), Some(cb)) = (parse_col(a), parse_col(b)) {
                return Some(Area {
                    r0: 0,
                    c0: ca.min(cb),
                    r1: MAX_ROWS - 1,
                    c1: ca.max(cb),
                });
            }
            if let (Some(ra), Some(rb)) = (parse_row(a), parse_row(b)) {
                return Some(Area {
                    r0: ra.min(rb),
                    c0: 0,
                    r1: ra.max(rb),
                    c1: MAX_COLS - 1,
                });
            }
            None
        }
    }
}

/// Split an optional sheet qualifier: `Sheet1!A1`, `'My Sheet'!A1:B2`
/// (`''` escapes a quote). Returns the unquoted sheet name and the remainder.
pub fn split_sheet(s: &str) -> (Option<String>, &str) {
    if let Some(rest) = s.strip_prefix('\'') {
        let mut name = String::new();
        let mut it = rest.char_indices().peekable();
        while let Some((i, ch)) = it.next() {
            if ch == '\'' {
                if matches!(it.peek(), Some((_, '\''))) {
                    name.push('\'');
                    it.next();
                } else if let Some(tail) = rest[i + 1..].strip_prefix('!') {
                    return (Some(name), tail);
                } else {
                    break;
                }
            } else {
                name.push(ch);
            }
        }
        return (None, s);
    }
    match s.rsplit_once('!') {
        Some((sheet, tail)) if !sheet.is_empty() => (Some(sheet.to_string()), tail),
        _ => (None, s),
    }
}

/// Canonical text of a range with its sheet: `Sheet1!B2:C9`, quoting the sheet
/// name when it needs it (`'My Sheet'!A1`).
pub fn qualified(sheets: &[String], r: &Rect) -> String {
    let name = sheets.get(r.sheet as usize).map_or("?", String::as_str);
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    let sheet = if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    };
    format!("{sheet}!{}", rect_text(r))
}

/// Format a rectangle as A1 text (without sheet).
pub fn rect_text(r: &Rect) -> String {
    if r.is_single() {
        a1(r.r0, r.c0)
    } else {
        format!("{}:{}", a1(r.r0, r.c0), a1(r.r1, r.c1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_roundtrip() {
        for (s, i) in [("A", 0), ("Z", 25), ("AA", 26), ("XFD", 16383)] {
            assert_eq!(col_index(s), Some(i));
            assert_eq!(col_name(i).to_string(), s);
        }
        assert_eq!(col_index("XFE"), None);
    }

    #[test]
    fn areas() {
        assert_eq!(
            parse_area("$B$2"),
            Some(Area {
                r0: 1,
                c0: 1,
                r1: 1,
                c1: 1
            })
        );
        assert_eq!(
            parse_area("B3:A1"),
            Some(Area {
                r0: 0,
                c0: 0,
                r1: 2,
                c1: 1
            })
        );
        assert_eq!(
            parse_area("A:B").map(|a| (a.r0, a.r1)),
            Some((0, MAX_ROWS - 1))
        );
        assert_eq!(
            parse_area("2:3").map(|a| (a.c0, a.c1)),
            Some((0, MAX_COLS - 1))
        );
        assert_eq!(parse_area("A0"), None);
        assert_eq!(parse_area("SUM"), None);
    }

    #[test]
    fn sheet_qualifiers() {
        assert_eq!(split_sheet("S1!A1"), (Some("S1".into()), "A1"));
        assert_eq!(split_sheet("'It''s'!B2:C3"), (Some("It's".into()), "B2:C3"));
        assert_eq!(split_sheet("A1"), (None, "A1"));
    }
}
