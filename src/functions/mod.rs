//! Function registry. Every function has a namespaced identity; the parser
//! asks for `(dialect, name)` and gets the dialect-specific override when one
//! exists, the `core` implementation otherwise.

mod datetime;
mod finance;
mod logic;
mod lookup;
mod math;
mod ods;
mod stats;
mod text;

use crate::engine::{Engine, Res};
use crate::ir::{Expr, FuncId, Ns};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Scalar functions that act element-wise over arrays in array context.
const LIFTED: &[&str] = &[
    "LEN",
    "UPPER",
    "LOWER",
    "TRIM",
    "PROPER",
    "LEFT",
    "RIGHT",
    "MID",
    "ABS",
    "ROUND",
    "ROUNDUP",
    "ROUNDDOWN",
    "INT",
    "MOD",
    "SQRT",
    "EXP",
    "LN",
    "SIGN",
    "POWER",
    "ISNUMBER",
    "ISTEXT",
    "ISBLANK",
    "ISERROR",
    "ISNA",
    "ISEVEN",
    "ISODD",
    "VALUE",
    "TEXT",
    "SEARCH",
    "FIND",
    "SUBSTITUTE",
    "YEAR",
    "MONTH",
    "DAY",
    "WEEKDAY",
    "EXACT",
    "REPT",
    "CODE",
    "CHAR",
    "NOT",
];

/// Arity limit meaning "any number of arguments".
pub const VAR: u8 = u8::MAX;

pub type EagerFn = fn(&mut Engine, &[Value]) -> Res<Value>;
/// Special forms receive their arguments unevaluated.
pub type LazyFn = fn(&mut Engine, &[Expr]) -> Res<Value>;

#[derive(Clone, Copy)]
pub enum Imp {
    Eager(EagerFn),
    Lazy(LazyFn),
}

pub struct FuncDef {
    pub ns: Ns,
    pub name: &'static str,
    pub min: u8,
    pub max: u8,
    pub imp: Imp,
    /// Result can change between evaluations (`RAND`, `NOW`): never folded.
    pub volatile: bool,
    /// May dereference cells unknown at build time (`INDIRECT`, `OFFSET`).
    pub dynamic: bool,
    /// Arguments are evaluated in array context: operators over ranges work
    /// element-wise instead of intersecting with the current cell.
    pub array_args: bool,
    /// Scalar function applied element-wise when given arrays in array context.
    pub lift: bool,
}

pub(crate) struct Reg(Vec<FuncDef>);

impl Reg {
    pub(crate) fn eager(
        &mut self,
        name: &'static str,
        min: u8,
        max: u8,
        f: EagerFn,
    ) -> &mut FuncDef {
        self.push(Ns::Core, name, min, max, Imp::Eager(f))
    }

    pub(crate) fn lazy(&mut self, name: &'static str, min: u8, max: u8, f: LazyFn) -> &mut FuncDef {
        self.push(Ns::Core, name, min, max, Imp::Lazy(f))
    }

    fn push(&mut self, ns: Ns, name: &'static str, min: u8, max: u8, imp: Imp) -> &mut FuncDef {
        self.0.push(FuncDef {
            ns,
            name,
            min,
            max,
            imp,
            volatile: false,
            dynamic: false,
            array_args: false,
            lift: false,
        });
        let last = self.0.len() - 1;
        &mut self.0[last]
    }

    pub(crate) fn eager_in(
        &mut self,
        ns: Ns,
        name: &'static str,
        min: u8,
        max: u8,
        f: EagerFn,
    ) -> &mut FuncDef {
        self.push(ns, name, min, max, Imp::Eager(f))
    }
}

impl FuncDef {
    pub(crate) fn volatile(&mut self) -> &mut Self {
        self.volatile = true;
        self
    }

    pub(crate) fn array_args(&mut self) -> &mut Self {
        self.array_args = true;
        self
    }

    pub(crate) fn dynamic(&mut self) -> &mut Self {
        self.dynamic = true;
        self
    }
}

pub fn table() -> &'static [FuncDef] {
    &REGISTRY.0
}

struct Registry(Vec<FuncDef>, HashMap<(Ns, &'static str), u16>);

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let mut r = Reg(Vec::new());
    math::register(&mut r);
    stats::register(&mut r);
    logic::register(&mut r);
    text::register(&mut r);
    lookup::register(&mut r);
    datetime::register(&mut r);
    finance::register(&mut r);
    ods::register(&mut r);
    for d in &mut r.0 {
        d.lift = LIFTED.contains(&d.name);
    }
    let map =
        r.0.iter()
            .enumerate()
            .map(|(i, d)| ((d.ns, d.name), i as u16))
            .collect();
    Registry(r.0, map)
});

/// Resolve `name` (uppercase) for a dialect: the dialect namespace first, then
/// `core`.
pub fn lookup(ns: Ns, name: &str) -> Option<FuncId> {
    let reg = &*REGISTRY;
    reg.1
        .get(&(ns, name))
        .or_else(|| reg.1.get(&(Ns::Core, name)))
        .map(|&i| FuncId(i))
}

// ----- argument helpers shared by the function modules -----

/// Argument `i` unless it is absent or was omitted (`F(a,,c)`).
pub(crate) fn opt(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Empty))
}

pub(crate) fn opt_num(e: &mut Engine, args: &[Value], i: usize, default: f64) -> Res<f64> {
    match opt(args, i) {
        Some(v) => e.num(v),
        None => Ok(default),
    }
}

pub(crate) fn num(n: f64) -> Res<Value> {
    if n.is_finite() {
        Ok(Value::Number(n))
    } else {
        Err(crate::value::ErrorKind::Num)
    }
}

/// Round half away from zero at `digits` decimal places on the 15 significant
/// digits of `x` (the way spreadsheets round).
pub fn round_decimal(x: f64, digits: i32) -> f64 {
    math::round_dec(x, digits, math::Mode::Nearest)
}
