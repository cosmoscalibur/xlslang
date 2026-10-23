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

/// Spreadsheet ordering of two scalars. `Empty` takes the type of the other
/// operand; text compares case-insensitively.
/// `case_sensitive`: text compares exactly instead of ignoring case.
pub fn compare(a: &Value, b: &Value, case_sensitive: bool) -> Ordering {
    let (a, b) = match (a, b) {
        (Value::Empty, Value::Text(_)) => (&Value::Text(Arc::from("")), b),
        (Value::Text(_), Value::Empty) => (a, &Value::Text(Arc::from(""))),
        (Value::Empty, Value::Bool(_)) => (&Value::Bool(false), b),
        (Value::Bool(_), Value::Empty) => (a, &Value::Bool(false)),
        _ => (a, b),
    };
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => {
            if case_sensitive {
                cmp_text_cs(x, y)
            } else {
                cmp_text(x, y)
            }
        }
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Empty | Value::Number(_), Value::Empty | Value::Number(_)) => {
            let x = if let Value::Number(n) = a { *n } else { 0.0 };
            let y = if let Value::Number(n) = b { *n } else { 0.0 };
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        _ => a.type_rank().cmp(&b.type_rank()),
    }
}

/// Text order as a spreadsheet collator gives it (English/Spanish): letters
/// compare first by their base letter (`á` sorts with `a`, `abc` < `ABD` <
/// `abd`), then accents (`a` < `á`), and only then case. This variant ignores
/// case entirely; [`cmp_text_cs`] adds it as the last tie-break.
pub fn cmp_text(x: &str, y: &str) -> Ordering {
    let key = |s: &str| s.chars().map(|c| base_letter(c).0).collect::<Vec<_>>();
    key(x).cmp(&key(y)).then_with(|| {
        let acc = |s: &str| s.chars().map(|c| base_letter(c).1).collect::<Vec<_>>();
        acc(x).cmp(&acc(y))
    })
}

const ACCENTED: &str = "àáâãäåçèéêëìíîïñòóôõöùúûüýÿ";
const BASE: &str = "aaaaaaceeeeiiiinooooouuuuyy";

/// Lowercase base letter and accent rank (0 = none) of a character.
fn base_letter(c: char) -> (char, u8) {
    let mut it = c.to_lowercase();
    let l = match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    };
    match ACCENTED.chars().position(|a| a == l) {
        Some(i) => (BASE.chars().nth(i).unwrap_or(l), i as u8 + 1),
        None => (l, 0),
    }
}

/// Case-sensitive order: everything [`cmp_text`] looks at first, and only when
/// equal does case decide, lowercase first (`a` < `A`).
pub fn cmp_text_cs(x: &str, y: &str) -> Ordering {
    cmp_text(x, y).then_with(|| {
        for (a, b) in x.chars().zip(y.chars()) {
            if a != b {
                return if a.is_lowercase() {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
        }
        Ordering::Equal
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collation() {
        assert_eq!(
            cmp_text("á", "b"),
            Ordering::Less,
            "accents do not jump past letters"
        );
        assert_eq!(cmp_text("a", "á"), Ordering::Less);
        assert_eq!(cmp_text("ABC", "abc"), Ordering::Equal);
        assert_eq!(cmp_text_cs("abc", "ABD"), Ordering::Less);
        assert_eq!(cmp_text_cs("a", "A"), Ordering::Less);
        // ñ is n with a tilde here (English); Spanish makes it a letter after n.
        assert_eq!(cmp_text("ña", "nb"), Ordering::Less);
        assert_eq!(cmp_text("Ñandú", "ñANDÚ"), Ordering::Equal);
    }
}
