//! Hand-written formula parser (Pratt) producing IR directly.
//!
//! The grammar is the Excel formula syntax as the readers normalize it (ODS
//! references and `;` separators are already converted by the frontend), plus
//! the function-name prefixes each format uses for extension functions.
//! Spreadsheet formulas are tiny and regular, so a single pass over the bytes
//! with no token vector or parse tree is both the simplest and the fastest
//! option.

use crate::addr::{self, CellRef, Rect};
use crate::functions;
use crate::ir::{BinOp, Expr, Ns, UnOp};
use crate::value::{Array, ErrorKind, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Name resolution context of a formula.
pub struct ParseCtx<'a> {
    pub sheets: &'a [String],
    pub current_sheet: u16,
    /// Defined names: name (any case) → formula text without `=`.
    pub names: &'a HashMap<String, String>,
    /// Namespace tried before `core` for unqualified function names.
    pub dialect: Ns,
}

impl ParseCtx<'_> {
    fn sheet_id(&self, name: &str) -> Option<u16> {
        self.sheets
            .iter()
            .position(|s| s.eq_ignore_ascii_case(name))
            .map(|i| i as u16)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub pos: usize,
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at {}: {}", self.pos, self.msg)
    }
}

impl std::error::Error for ParseError {}

/// Parse a formula body (without the leading `=`).
pub fn parse(src: &str, ctx: &ParseCtx, diags: &mut Vec<String>) -> Result<Expr, ParseError> {
    parse_depth(src, ctx, diags, 0)
}

fn parse_depth(
    src: &str,
    ctx: &ParseCtx,
    diags: &mut Vec<String>,
    depth: u8,
) -> Result<Expr, ParseError> {
    let src = src.strip_prefix('=').unwrap_or(src);
    let mut p = Parser {
        s: src.as_bytes(),
        src,
        i: 0,
        ctx,
        diags,
        depth,
    };
    let e = p.expr(0)?;
    p.ws();
    if p.i < p.s.len() {
        return p.fail("unexpected trailing input");
    }
    Ok(e)
}

struct Parser<'a, 'c> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
    ctx: &'a ParseCtx<'c>,
    diags: &'a mut Vec<String>,
    depth: u8,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || matches!(b, b'_' | b'$' | b'\\') || b >= 0x80
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$' | b'\\' | b'?') || b >= 0x80
}

/// Left/right binding power of an infix operator.
fn infix(op: &[u8]) -> Option<(BinOp, u8, usize)> {
    Some(match op {
        [b'<', b'=', ..] => (BinOp::Le, 1, 2),
        [b'>', b'=', ..] => (BinOp::Ge, 1, 2),
        [b'<', b'>', ..] => (BinOp::Ne, 1, 2),
        [b'=', ..] => (BinOp::Eq, 1, 1),
        [b'<', ..] => (BinOp::Lt, 1, 1),
        [b'>', ..] => (BinOp::Gt, 1, 1),
        [b'&', ..] => (BinOp::Concat, 2, 1),
        [b'+', ..] => (BinOp::Add, 3, 1),
        [b'-', ..] => (BinOp::Sub, 3, 1),
        [b'*', ..] => (BinOp::Mul, 4, 1),
        [b'/', ..] => (BinOp::Div, 4, 1),
        [b'^', ..] => (BinOp::Pow, 5, 1),
        _ => return None,
    })
}

const PREFIX_BP: u8 = 7;
const RANGE_BP: u8 = 8;

impl Parser<'_, '_> {
    fn fail<T>(&self, msg: &str) -> Result<T, ParseError> {
        Err(ParseError {
            pos: self.i,
            msg: msg.to_string(),
        })
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.ws();
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.prefix()?;
        loop {
            self.ws();
            let rest = &self.s[self.i.min(self.s.len())..];
            match rest.first() {
                Some(b'%') => {
                    self.i += 1;
                    lhs = Expr::Unary(UnOp::Percent, Box::new(lhs));
                    continue;
                }
                Some(b':') if RANGE_BP >= min_bp => {
                    self.i += 1;
                    let rhs = self.expr(RANGE_BP + 1)?;
                    lhs = range_of(lhs, rhs);
                    continue;
                }
                _ => {}
            }
            let Some((op, bp, len)) = infix(rest) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            self.i += len;
            // `^` is left-associative in spreadsheets, like the rest.
            let rhs = self.expr(bp + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr, ParseError> {
        self.ws();
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                let e = self.expr(PREFIX_BP)?;
                Ok(match e {
                    Expr::Const(Value::Number(n)) => Expr::Const(Value::Number(-n)),
                    e => Expr::Unary(UnOp::Neg, Box::new(e)),
                })
            }
            Some(b'+') => {
                self.i += 1;
                let e = self.expr(PREFIX_BP)?;
                Ok(Expr::Unary(UnOp::Plus, Box::new(e)))
            }
            // Implicit intersection operator: no-op for our purposes.
            Some(b'@') => {
                self.i += 1;
                self.expr(PREFIX_BP)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        self.ws();
        let Some(b) = self.peek() else {
            return self.fail("unexpected end of formula");
        };
        match b {
            b'(' => {
                self.i += 1;
                let e = self.expr(0)?;
                if !self.eat(b')') {
                    return self.fail("expected `)`");
                }
                Ok(e)
            }
            b'"' => Ok(Expr::Const(Value::text(self.string()?))),
            b'#' => self.error_literal().map(|e| Expr::Const(Value::Error(e))),
            b'{' => self.array_literal(),
            b'\'' => {
                let (name, len) = self.quoted_sheet()?;
                self.i += len;
                self.sheet_ref(&name)
            }
            b'0'..=b'9' | b'.' => self.number_or_rows(),
            _ if is_ident_start(b) => self.ident(),
            _ => self.fail("unexpected character"),
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.i += 1;
        let mut out = String::new();
        let mut start = self.i;
        loop {
            match self.peek() {
                None => return self.fail("unterminated string"),
                Some(b'"') => {
                    out.push_str(&self.src[start..self.i]);
                    self.i += 1;
                    if self.peek() == Some(b'"') {
                        out.push('"');
                        self.i += 1;
                        start = self.i;
                    } else {
                        return Ok(out);
                    }
                }
                Some(_) => self.i += 1,
            }
        }
    }

    fn error_literal(&mut self) -> Result<ErrorKind, ParseError> {
        let rest = &self.src[self.i..];
        for len in [7, 6, 5, 4] {
            if let Some(cand) = rest.get(..len)
                && let Some(e) = ErrorKind::parse(cand)
            {
                self.i += len;
                return Ok(e);
            }
        }
        self.fail("unknown error literal")
    }

    /// Returns the unquoted name and the byte length consumed through the `!`.
    fn quoted_sheet(&self) -> Result<(String, usize), ParseError> {
        match addr::split_sheet(&self.src[self.i..]) {
            (Some(name), tail) => Ok((name, self.src.len() - self.i - tail.len())),
            _ => self.fail("malformed quoted sheet name"),
        }
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.')) {
            self.i += 1;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        self.src[start..self.i]
            .parse()
            .or_else(|_| self.fail("bad number"))
    }

    fn number_or_rows(&mut self) -> Result<Expr, ParseError> {
        let start = self.i;
        let n = self.number()?;
        // Whole-row reference `1:3`.
        if self.peek() == Some(b':') && matches!(self.s.get(self.i + 1), Some(b'0'..=b'9')) {
            let save = self.i;
            self.i += 1;
            let rhs_start = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            let text = format!(
                "{}:{}",
                &self.src[start..save],
                &self.src[rhs_start..self.i]
            );
            if let Some(area) = addr::parse_area(&text) {
                return Ok(Expr::Range(area.bind(self.ctx.current_sheet)));
            }
            self.i = save;
        }
        Ok(Expr::Const(Value::Number(n)))
    }

    fn array_literal(&mut self) -> Result<Expr, ParseError> {
        self.i += 1; // `{`
        let mut rows: Vec<Vec<Value>> = vec![vec![]];
        loop {
            self.ws();
            let v = match self.peek() {
                Some(b'"') => Value::text(self.string()?),
                Some(b'#') => Value::Error(self.error_literal()?),
                Some(b'-' | b'+' | b'0'..=b'9' | b'.') => {
                    let neg = self.peek() == Some(b'-');
                    if matches!(self.peek(), Some(b'-' | b'+')) {
                        self.i += 1;
                    }
                    let n = self.number()?;
                    Value::Number(if neg { -n } else { n })
                }
                Some(c) if is_ident_start(c) => {
                    let start = self.i;
                    while self.peek().is_some_and(is_ident) {
                        self.i += 1;
                    }
                    match self.src[start..self.i].to_ascii_uppercase().as_str() {
                        "TRUE" => Value::Bool(true),
                        "FALSE" => Value::Bool(false),
                        _ => return self.fail("array literals hold constants only"),
                    }
                }
                _ => return self.fail("bad array literal"),
            };
            if let Some(r) = rows.last_mut() {
                r.push(v);
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b';') => {
                    self.i += 1;
                    rows.push(vec![]);
                }
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return self.fail("expected `,`, `;` or `}`"),
            }
        }
        let cols = rows[0].len();
        if rows.iter().any(|r| r.len() != cols) {
            return self.fail("ragged array literal");
        }
        let n = rows.len() as u32;
        let data = rows.into_iter().flatten().collect();
        Ok(Expr::Const(Value::Array(Arc::new(Array::new(
            n,
            cols as u32,
            data,
        )))))
    }

    fn ident(&mut self) -> Result<Expr, ParseError> {
        let start = self.i;
        while self.peek().is_some_and(is_ident) {
            self.i += 1;
        }
        let text = &self.src[start..self.i];
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                return self.call(text);
            }
            Some(b'!') => {
                self.i += 1;
                return self.sheet_ref(text);
            }
            _ => {}
        }
        match text.to_ascii_uppercase().as_str() {
            "TRUE" => return Ok(Expr::Const(Value::Bool(true))),
            "FALSE" => return Ok(Expr::Const(Value::Bool(false))),
            _ => {}
        }
        if let Some(e) = self.area_from(text, self.ctx.current_sheet) {
            return Ok(e);
        }
        self.name(text)
    }

    /// Try `text` (and a following `:ref` tail) as an area on `sheet`.
    fn area_from(&mut self, text: &str, sheet: u16) -> Option<Expr> {
        if self.peek() == Some(b':') {
            // Only consume the tail when it forms a literal area; otherwise
            // `:` stays a range operator (`A1:INDEX(…)`).
            let save = self.i;
            self.i += 1;
            let tail_start = self.i;
            while self.peek().is_some_and(is_ident) {
                self.i += 1;
            }
            let mut tail = &self.src[tail_start..self.i];
            if self.peek() == Some(b'!') {
                // `A1:Sheet1!B2` — the second qualifier must match the first.
                self.i += 1;
                let t2 = self.i;
                while self.peek().is_some_and(is_ident) {
                    self.i += 1;
                }
                tail = &self.src[t2..self.i];
            }
            if self.peek() != Some(b'(')
                && let Some(area) = addr::parse_area(&format!("{text}:{tail}"))
            {
                return Some(Expr::Range(area.bind(sheet)));
            }
            self.i = save;
        }
        let area = addr::parse_area(text)?;
        Some(if area.r0 == area.r1 && area.c0 == area.c1 {
            Expr::Cell(CellRef::new(sheet, area.r0, area.c0))
        } else {
            Expr::Range(area.bind(sheet))
        })
    }

    fn sheet_ref(&mut self, sheet_name: &str) -> Result<Expr, ParseError> {
        let Some(sheet) = self.ctx.sheet_id(sheet_name) else {
            // Unknown sheet or an external workbook: keep going with #REF!.
            self.diags.push(format!("unresolved sheet `{sheet_name}`"));
            while self.peek().is_some_and(|b| is_ident(b) || b == b':') {
                self.i += 1;
            }
            return Ok(Expr::Const(Value::Error(ErrorKind::Ref)));
        };
        let start = self.i;
        while self.peek().is_some_and(is_ident) {
            self.i += 1;
        }
        let text = &self.src[start..self.i];
        match self.area_from(text, sheet) {
            Some(e) => Ok(e),
            None => self.name(text),
        }
    }

    fn name(&mut self, text: &str) -> Result<Expr, ParseError> {
        let found = self
            .ctx
            .names
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(text))
            .map(|(_, v)| v.clone());
        match found {
            Some(def) if self.depth < 8 => parse_depth(&def, self.ctx, self.diags, self.depth + 1)
                .or_else(|e| {
                    self.diags.push(format!("defined name `{text}`: {e}"));
                    Ok(Expr::Const(Value::Error(ErrorKind::Name)))
                }),
            _ => {
                self.diags.push(format!("unresolved name `{text}`"));
                Ok(Expr::Const(Value::Error(ErrorKind::Name)))
            }
        }
    }

    fn call(&mut self, raw_name: &str) -> Result<Expr, ParseError> {
        let mut args = Vec::new();
        self.ws();
        if self.peek() == Some(b')') {
            self.i += 1;
        } else {
            loop {
                self.ws();
                if matches!(self.peek(), Some(b',' | b';' | b')')) {
                    args.push(Expr::Missing);
                } else {
                    args.push(self.expr(0)?);
                }
                self.ws();
                match self.peek() {
                    Some(b',' | b';') => self.i += 1,
                    Some(b')') => {
                        self.i += 1;
                        break;
                    }
                    _ => return self.fail("expected `,` or `)` in call"),
                }
            }
        }
        let (ns, name) = split_function_name(raw_name, self.ctx.dialect);
        match functions::lookup(ns, &name) {
            Some(id) => {
                let def = id.def();
                if args.len() < def.min as usize || args.len() > def.max as usize {
                    self.diags.push(format!(
                        "{}: wrong number of arguments ({})",
                        id.qualified(),
                        args.len()
                    ));
                }
                Ok(Expr::Call(id, args))
            }
            None => {
                self.diags.push(format!("unsupported function `{name}`"));
                Ok(Expr::Unknown(name.into(), args))
            }
        }
    }
}

/// Normalize a function name and pick the namespace its prefix implies.
///
/// `_xlfn.`/`_xlws.` mark Excel extension functions in XLSX;
/// `COM.MICROSOFT.`, `ORG.OPENOFFICE.` and `ORG.LIBREOFFICE.` are the ODS
/// (OpenFormula) prefixes for the same idea.
pub fn split_function_name(raw: &str, dialect: Ns) -> (Ns, String) {
    let up = raw.to_ascii_uppercase();
    for (prefix, ns) in [
        ("_XLFN.", Ns::Xlsx),
        ("_XLWS.", Ns::Xlsx),
        ("COM.MICROSOFT.", Ns::Ods),
        ("ORG.OPENOFFICE.", Ns::Ods),
        ("ORG.LIBREOFFICE.", Ns::Ods),
    ] {
        if let Some(rest) = up.strip_prefix(prefix) {
            return (
                if dialect == Ns::Core { ns } else { dialect },
                rest.to_string(),
            );
        }
    }
    (dialect, up)
}

/// Fold `a:b` over literal references, otherwise defer to the dynamic
/// `_RANGE` function.
fn range_of(a: Expr, b: Expr) -> Expr {
    let rect = |e: &Expr| match e {
        Expr::Cell(c) => Some(Rect::single(*c)),
        Expr::Range(r) => Some(*r),
        _ => None,
    };
    if let (Some(x), Some(y)) = (rect(&a), rect(&b))
        && x.sheet == y.sheet
    {
        return Expr::Range(Rect::new(
            x.sheet,
            x.r0.min(y.r0),
            x.c0.min(y.c0),
            x.r1.max(y.r1),
            x.c1.max(y.c1),
        ));
    }
    match functions::lookup(Ns::Core, "_RANGE") {
        Some(id) => Expr::Call(id, vec![a, b]),
        None => Expr::Const(Value::Error(ErrorKind::Ref)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(sheets: &'a [String], names: &'a HashMap<String, String>) -> ParseCtx<'a> {
        ParseCtx {
            sheets,
            current_sheet: 0,
            names,
            dialect: Ns::Xlsx,
        }
    }

    fn p(src: &str) -> Expr {
        let sheets = vec!["Sheet1".to_string(), "My Sheet".to_string()];
        let names = HashMap::from([("Rate".to_string(), "Sheet1!$B$1".to_string())]);
        parse(src, &ctx(&sheets, &names), &mut vec![]).unwrap()
    }

    fn n(v: f64) -> Expr {
        Expr::Const(Value::Number(v))
    }

    #[test]
    fn precedence_and_unary() {
        // -2^2 = 4: unary minus binds tighter than ^.
        let Expr::Binary(BinOp::Pow, l, _) = p("-2^2") else {
            panic!()
        };
        assert_eq!(*l, n(-2.0));
        let Expr::Binary(BinOp::Add, _, r) = p("1+2*3") else {
            panic!()
        };
        assert!(matches!(*r, Expr::Binary(BinOp::Mul, ..)));
        assert!(matches!(p("1&2=3"), Expr::Binary(BinOp::Eq, ..)));
        assert!(matches!(p("50%"), Expr::Unary(UnOp::Percent, _)));
    }

    #[test]
    fn references() {
        assert_eq!(p("$B$2"), Expr::Cell(CellRef::new(0, 1, 1)));
        assert_eq!(p("'My Sheet'!A1:B2"), Expr::Range(Rect::new(1, 0, 0, 1, 1)));
        assert_eq!(p("Sheet1!C3"), Expr::Cell(CellRef::new(0, 2, 2)));
        assert!(matches!(p("A:A"), Expr::Range(r) if r.rows() == addr::MAX_ROWS));
        assert!(matches!(p("1:2"), Expr::Range(r) if r.cols() == addr::MAX_COLS));
        assert_eq!(p("Rate"), Expr::Cell(CellRef::new(0, 0, 1)));
    }

    #[test]
    fn calls_and_prefixes() {
        let Expr::Call(id, args) = p("SUM(A1:A3, 1)") else {
            panic!()
        };
        assert_eq!(id.qualified(), "core::SUM");
        assert_eq!(args.len(), 2);
        let Expr::Call(id, _) = p("_xlfn.CONCAT(\"a\")") else {
            panic!()
        };
        assert_eq!(id.qualified(), "core::CONCAT");
        assert!(matches!(p("IF(A1,,2)"), Expr::Call(_, a) if a[1] == Expr::Missing));
        assert!(matches!(p("NOPE(1)"), Expr::Unknown(..)));
    }

    #[test]
    fn literals() {
        assert_eq!(p("\"a\"\"b\""), Expr::Const(Value::text("a\"b")));
        assert_eq!(p("#N/A"), Expr::Const(Value::Error(ErrorKind::Na)));
        assert_eq!(p("TRUE"), Expr::Const(Value::Bool(true)));
        let Expr::Const(Value::Array(a)) = p("{1,2;3,4}") else {
            panic!()
        };
        assert_eq!((a.rows, a.cols), (2, 2));
        assert_eq!(p("1.5E3"), n(1500.0));
    }

    #[test]
    fn dynamic_range_operator() {
        assert!(matches!(p("A1:INDEX(B1:B5,2)"), Expr::Call(..)));
    }
}
