//! Runtime values shared by the IR, the optimizer and the engine.

use crate::addr::Rect;
use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
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

/// Text collators for a language: one that ignores case (accents still count)
/// and one that breaks ties by case, lowercase first. They are ICU/CLDR
/// collators — the ones LibreOffice itself uses — so `ñ` falls between `n` and
/// `o` in Spanish, `á` sorts with `a` and a new language is a new tag.
/// A collator with its built-in (compiled) data.
pub type Coll = CollatorBorrowed<'static>;

pub struct Collators {
    ci: Coll,
    cs: Coll,
}

impl Collators {
    /// `lang`: the language subtag of the locale (`en`, `es`).
    pub fn new(lang: &str) -> Self {
        let locale = lang
            .parse::<icu_locale::Locale>()
            .unwrap_or(icu_locale::locale!("en"));
        let make = |strength| {
            let mut o = CollatorOptions::default();
            o.strength = Some(strength);
            Collator::try_new(CollatorPreferences::from(&locale), o)
                .or_else(|_| {
                    Collator::try_new(CollatorPreferences::from(icu_locale::locale!("en")), o)
                })
                .expect("built-in collation data")
        };
        Self {
            ci: make(Strength::Secondary),
            cs: make(Strength::Tertiary),
        }
    }

    /// `case_sensitive`: case decides ties instead of being ignored.
    pub fn get(&self, case_sensitive: bool) -> &Coll {
        if case_sensitive { &self.cs } else { &self.ci }
    }
}

/// Spreadsheet ordering of two scalars. `Empty` takes the type of the other
/// operand.
pub fn compare(a: &Value, b: &Value, col: &Coll) -> Ordering {
    let (a, b) = match (a, b) {
        (Value::Empty, Value::Text(_)) => (&Value::Text(Arc::from("")), b),
        (Value::Text(_), Value::Empty) => (a, &Value::Text(Arc::from(""))),
        (Value::Empty, Value::Bool(_)) => (&Value::Bool(false), b),
        (Value::Bool(_), Value::Empty) => (a, &Value::Bool(false)),
        _ => (a, b),
    };
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => col.compare(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Empty | Value::Number(_), Value::Empty | Value::Number(_)) => {
            let x = if let Value::Number(n) = a { *n } else { 0.0 };
            let y = if let Value::Number(n) = b { *n } else { 0.0 };
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        _ => a.type_rank().cmp(&b.type_rank()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collation() {
        let en = Collators::new("en");
        let es = Collators::new("es");
        let (ci, cs) = (en.get(false), en.get(true));
        assert_eq!(
            ci.compare("á", "b"),
            Ordering::Less,
            "accents do not jump past letters"
        );
        assert_eq!(ci.compare("a", "á"), Ordering::Less);
        assert_eq!(ci.compare("ABC", "abc"), Ordering::Equal);
        assert_eq!(cs.compare("abc", "ABD"), Ordering::Less);
        assert_eq!(cs.compare("a", "A"), Ordering::Less);
        assert_eq!(ci.compare("Ñandú", "ñANDÚ"), Ordering::Equal);
        // English: ñ is n with a tilde. Spanish: a letter between n and o.
        assert_eq!(ci.compare("ña", "nb"), Ordering::Less);
        assert_eq!(es.get(false).compare("ña", "nz"), Ordering::Greater);
        assert_eq!(es.get(false).compare("ña", "oa"), Ordering::Less);
        assert_eq!(es.get(false).compare("nz", "ñ"), Ordering::Less);
        assert_eq!(es.get(false).compare("ñ", "Ñ"), Ordering::Equal);
    }
}
