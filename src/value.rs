//! Runtime values shared by the IR, the optimizer and the engine.

use crate::addr::Rect;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;

/// Spreadsheet error values. `Circ` is an engine extension for unresolved
/// circular references (no counterpart in the file formats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorKind {
    Null,
    DivZero,
    Value,
    Ref,
    Name,
    Num,
    Na,
    Circ,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Null => "#NULL!",
            Self::DivZero => "#DIV/0!",
            Self::Value => "#VALUE!",
            Self::Ref => "#REF!",
            Self::Name => "#NAME?",
            Self::Num => "#NUM!",
            Self::Na => "#N/A",
            Self::Circ => "#CIRC!",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "#NULL!" => Self::Null,
            "#DIV/0!" => Self::DivZero,
            "#VALUE!" => Self::Value,
            "#REF!" => Self::Ref,
            "#NAME?" => Self::Name,
            "#NUM!" => Self::Num,
            "#N/A" => Self::Na,
            "#CIRC!" => Self::Circ,
            _ => return None,
        })
    }

    /// Code returned by the `ERROR.TYPE` function (1-based, `#N/A` is 7).
    pub fn code(self) -> f64 {
        match self {
            Self::Null => 1.0,
            Self::DivZero => 2.0,
            Self::Value => 3.0,
            Self::Ref => 4.0,
            Self::Name => 5.0,
            Self::Num => 6.0,
            Self::Na => 7.0,
            Self::Circ => 8.0,
        }
    }
}

/// Rectangular block of scalar values (row-major). Cells are never `Array`
/// or `Ref`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Array {
    pub rows: u32,
    pub cols: u32,
    pub data: Vec<Value>,
}

impl Array {
    pub fn new(rows: u32, cols: u32, data: Vec<Value>) -> Self {
        debug_assert_eq!(data.len(), rows as usize * cols as usize);
        Self { rows, cols, data }
    }

    pub fn filled(rows: u32, cols: u32, v: Value) -> Self {
        Self::new(rows, cols, vec![v; rows as usize * cols as usize])
    }

    pub fn get(&self, r: u32, c: u32) -> &Value {
        &self.data[r as usize * self.cols as usize + c as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Value {
    #[default]
    Empty,
    Number(f64),
    Bool(bool),
    Text(Arc<str>),
    Error(ErrorKind),
    Array(Arc<Array>),
    /// A reference. Only produced while evaluating; never stored in a cell.
    Ref(Rect),
}

impl Value {
    pub fn text(s: impl AsRef<str>) -> Self {
        Self::Text(Arc::from(s.as_ref()))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    pub fn as_error(&self) -> Option<ErrorKind> {
        match self {
            Self::Error(e) => Some(*e),
            _ => None,
        }
    }

    /// Rank used by the cross-type ordering: numbers < text < booleans.
    fn type_rank(&self) -> u8 {
        match self {
            Self::Empty | Self::Number(_) => 0,
            Self::Text(_) => 1,
            Self::Bool(_) => 2,
            _ => 3,
        }
    }
}

/// How text orders: whether case counts, and the language's alphabet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Collation {
    /// Case decides ties (`a` < `A`) instead of being ignored.
    pub case_sensitive: bool,
    /// Spanish alphabet: `ñ` is a letter of its own between `n` and `o`.
    pub spanish: bool,
}

/// Spreadsheet ordering of two scalars. `Empty` takes the type of the other
/// operand.
pub fn compare(a: &Value, b: &Value, col: Collation) -> Ordering {
    let (a, b) = match (a, b) {
        (Value::Empty, Value::Text(_)) => (&Value::Text(Arc::from("")), b),
        (Value::Text(_), Value::Empty) => (a, &Value::Text(Arc::from(""))),
        (Value::Empty, Value::Bool(_)) => (&Value::Bool(false), b),
        (Value::Bool(_), Value::Empty) => (a, &Value::Bool(false)),
        _ => (a, b),
    };
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => cmp_text(x, y, col),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Empty | Value::Number(_), Value::Empty | Value::Number(_)) => {
            let x = if let Value::Number(n) = a { *n } else { 0.0 };
            let y = if let Value::Number(n) = b { *n } else { 0.0 };
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        _ => a.type_rank().cmp(&b.type_rank()),
    }
}

const ACCENTED: &str = "àáâãäåçèéêëìíîïñòóôõöùúûüýÿ";
const BASE: &str = "aaaaaaceeeeiiiinooooouuuuyy";

/// Primary key (letter), accent rank (0 = none) and lowercase flag of a
/// character. Accented letters sort with their base letter at the primary level
/// (`á` with `a`); in the Spanish alphabet `ñ` is the exception, a primary
/// level of its own between `n` and `o`.
fn char_keys(c: char, spanish: bool) -> (u32, u8, bool) {
    let lower = c.is_lowercase();
    let mut it = c.to_lowercase();
    let l = match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    };
    if spanish && l == 'ñ' {
        return ('n' as u32 * 2 + 1, 0, lower);
    }
    match ACCENTED.chars().position(|a| a == l) {
        Some(i) => (
            BASE.chars().nth(i).unwrap_or(l) as u32 * 2,
            i as u8 + 1,
            lower,
        ),
        None => (l as u32 * 2, 0, lower),
    }
}

/// Text order as a collator gives it: letters first, then accents, then — only
/// when `case_sensitive` — case, lowercase first (`abc` < `ABD`, `a` < `á`,
/// `a` < `A`).
pub fn cmp_text(x: &str, y: &str, col: Collation) -> Ordering {
    let keys = |s: &str| {
        s.chars()
            .map(|c| char_keys(c, col.spanish))
            .collect::<Vec<_>>()
    };
    let (kx, ky) = (keys(x), keys(y));
    let primary = |k: &[(u32, u8, bool)]| k.iter().map(|t| t.0).collect::<Vec<_>>();
    let accent = |k: &[(u32, u8, bool)]| k.iter().map(|t| t.1).collect::<Vec<_>>();
    primary(&kx)
        .cmp(&primary(&ky))
        .then_with(|| accent(&kx).cmp(&accent(&ky)))
        .then_with(|| {
            if !col.case_sensitive {
                return Ordering::Equal;
            }
            // Lowercase sorts before uppercase at the first difference.
            kx.iter()
                .zip(&ky)
                .find(|(a, b)| a.2 != b.2)
                .map_or(Ordering::Equal, |(a, _)| {
                    if a.2 {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collation() {
        let ci = Collation::default();
        let cs = Collation {
            case_sensitive: true,
            ..ci
        };
        let es = Collation {
            spanish: true,
            ..ci
        };
        assert_eq!(
            cmp_text("á", "b", ci),
            Ordering::Less,
            "accents do not jump past letters"
        );
        assert_eq!(cmp_text("a", "á", ci), Ordering::Less);
        assert_eq!(cmp_text("ABC", "abc", ci), Ordering::Equal);
        assert_eq!(cmp_text("abc", "ABD", cs), Ordering::Less);
        assert_eq!(cmp_text("a", "A", cs), Ordering::Less);
        assert_eq!(cmp_text("Ñandú", "ñANDÚ", ci), Ordering::Equal);
        // English: ñ is n with a tilde. Spanish: a letter between n and o.
        assert_eq!(cmp_text("ña", "nb", ci), Ordering::Less);
        assert_eq!(cmp_text("ña", "nz", es), Ordering::Greater);
        assert_eq!(cmp_text("ña", "oa", es), Ordering::Less);
        assert_eq!(cmp_text("nz", "ñ", es), Ordering::Less);
        assert_eq!(cmp_text("ñ", "Ñ", es), Ordering::Equal);
    }
}
