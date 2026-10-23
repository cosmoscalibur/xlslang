//! Format-independent intermediate representation.
//!
//! Neither XLSX nor ODS survive past the frontend: formulas are lowered to
//! [`Expr`], cells to [`Program`] slots, and a single engine executes them.
//! Functions are identified by a namespaced [`FuncId`] (`core::SUM`,
//! `ods::EASTERSUNDAY`) so that behavioral differences between the formats can
//! coexist in one registry.

use crate::addr::{CellRef, Rect};
use crate::functions;
use crate::options::EngineOptions;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Function namespace. Lookup tries the dialect namespace first, then `Core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ns {
    /// Semantics shared by both formats.
    Core,
    /// Excel-specific functions or behavior.
    Xlsx,
    /// OpenDocument-specific functions or behavior.
    Ods,
}

impl Ns {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Xlsx => "xlsx",
            Self::Ods => "ods",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "core" => Some(Self::Core),
            "xlsx" => Some(Self::Xlsx),
            "ods" => Some(Self::Ods),
            _ => None,
        }
    }
}

/// Index into the function registry. Serialized as `"ns::NAME"` so programs
/// stay valid when the registry grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u16);

impl FuncId {
    pub fn def(self) -> &'static functions::FuncDef {
        &functions::table()[self.0 as usize]
    }

    pub fn qualified(self) -> String {
        let d = self.def();
        format!("{}::{}", d.ns.as_str(), d.name)
    }
}

impl Serialize for FuncId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.qualified())
    }
}

impl<'de> Deserialize<'de> for FuncId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let (ns, name) = s
            .split_once("::")
            .and_then(|(ns, n)| Ns::parse(ns).map(|ns| (ns, n)))
            .ok_or_else(|| serde::de::Error::custom(format!("bad function id `{s}`")))?;
        functions::lookup(ns, name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown function `{s}`")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Plus,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Const(Value),
    Cell(CellRef),
    Range(Rect),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(FuncId, Vec<Expr>),
    /// Call to a function missing from the registry: evaluates to `#NAME?`.
    Unknown(Box<str>, Vec<Expr>),
    /// Omitted argument (`IF(A1,,2)`).
    Missing,
}

impl Expr {
    /// Visit this node and all descendants.
    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Self::Unary(_, a) => a.walk(f),
            Self::Binary(_, a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Self::Call(_, args) | Self::Unknown(_, args) => args.iter().for_each(|a| a.walk(f)),
            _ => {}
        }
    }

    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        match self {
            Self::Unary(_, a) => a.walk_mut(f),
            Self::Binary(_, a, b) => {
                a.walk_mut(f);
                b.walk_mut(f);
            }
            Self::Call(_, args) | Self::Unknown(_, args) => {
                args.iter_mut().for_each(|a| a.walk_mut(f));
            }
            _ => {}
        }
        f(self);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellKind {
    Const(Value),
    Formula(Expr),
    /// Run-time parameter, with the value found in the source as default.
    Input(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellDef {
    pub at: CellRef,
    pub kind: CellKind,
}

/// A named group of cells exchanged with the outside world as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    pub rect: Rect,
}

/// The compiled artifact. `cells` is in evaluation order: dependencies first.
/// Slots listed in `cycles` (half-open ranges) form strongly connected
/// components that need iterative calculation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub version: u32,
    pub sheets: Vec<String>,
    pub options: EngineOptions,
    /// The locale (the user's regional settings) may be chosen when the program
    /// runs. Otherwise it is fixed: constants may have been folded with it.
    pub locale_open: bool,
    pub cells: Vec<CellDef>,
    pub cycles: Vec<(u32, u32)>,
    pub inputs: Vec<Binding>,
    pub outputs: Vec<Binding>,
    pub diagnostics: Vec<String>,
}

pub const PROGRAM_VERSION: u32 = 1;
