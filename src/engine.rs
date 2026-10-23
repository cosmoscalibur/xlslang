//! The execution engine: a demand-driven evaluator over a [`Program`].
//!
//! Cells are normally evaluated in the precomputed topological order, so every
//! reference finds its operand ready. References that the compiler could not
//! resolve statically (`INDIRECT`, `OFFSET` with run-time arguments) simply
//! demand the target cell: it is evaluated on the spot, memoized, and a cell
//! found in progress is a circular reference. That hybrid keeps one engine for
//! both the fully static and the dynamic case.

use crate::addr::{CellRef, Rect};
use crate::coerce;
use crate::ir::{BinOp, CellKind, Expr, Program, UnOp};
use crate::options::EngineOptions;
use crate::value::{Array, ErrorKind, Value, compare};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

pub type Res<T> = Result<T, ErrorKind>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum St {
    Pending,
    Busy,
    Done,
    /// Member of a cycle being iterated: reads return the current value.
    Iter,
}

pub struct Engine<'p> {
    prog: &'p Program,
    pub opts: EngineOptions,
    index: HashMap<CellRef, u32>,
    by_sheet: Vec<Vec<u32>>,
    /// Per sheet: (rows, cols) spanned by its cells.
    extent: Vec<(u32, u32)>,
    values: Vec<Value>,
    state: Vec<St>,
    /// Cell whose formula is being evaluated.
    pub at: CellRef,
    rng: u64,
    /// >0 while evaluating arguments of array-context functions.
    array_depth: u32,
    /// Materialized ranges whose cells were all final when read.
    range_cache: HashMap<Rect, Arc<Array>>,
    /// Reads that saw a cell still in flux (circular or iterating).
    unstable_reads: u64,
}

impl<'p> Engine<'p> {
    pub fn new(prog: &'p Program, opts: EngineOptions) -> Self {
        let n = prog.cells.len();
        let mut index = HashMap::with_capacity(n);
        let mut by_sheet = vec![Vec::new(); prog.sheets.len().max(1)];
        let mut extent = vec![(0u32, 0u32); prog.sheets.len().max(1)];
        let mut values = Vec::with_capacity(n);
        let mut state = Vec::with_capacity(n);
        for (i, c) in prog.cells.iter().enumerate() {
            index.insert(c.at, i as u32);
            let s = c.at.sheet as usize;
            by_sheet[s].push(i as u32);
            extent[s].0 = extent[s].0.max(c.at.row + 1);
            extent[s].1 = extent[s].1.max(c.at.col + 1);
            match &c.kind {
                CellKind::Const(v) | CellKind::Input(v) => {
                    values.push(v.clone());
                    state.push(St::Done);
                }
                CellKind::Formula(_) => {
                    values.push(Value::Empty);
                    state.push(St::Pending);
                }
            }
        }
        let seed = opts.rand_seed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
        });
        Self {
            prog,
            opts,
            index,
            by_sheet,
            extent,
            values,
            state,
            at: CellRef::new(0, 0, 0),
            rng: seed | 1,
            array_depth: 0,
            range_cache: HashMap::new(),
            unstable_reads: 0,
        }
    }

    /// Overwrite an input cell. Returns `false` if the cell is not an input.
    pub fn set_input(&mut self, at: CellRef, v: Value) -> bool {
        match self.index.get(&at) {
            Some(&s) if matches!(self.prog.cells[s as usize].kind, CellKind::Input(_)) => {
                self.values[s as usize] = v;
                true
            }
            _ => false,
        }
    }

    /// Evaluate every cell of the program in schedule order.
    pub fn run(&mut self) {
        let prog = self.prog;
        let mut cycles = prog.cycles.iter().peekable();
        let mut i = 0u32;
        let n = prog.cells.len() as u32;
        while i < n {
            if let Some(&&(s, e)) = cycles.peek()
                && s == i
            {
                cycles.next();
                self.run_cycle(s, e);
                i = e;
                continue;
            }
            self.ensure(i);
            i += 1;
        }
    }

    fn run_cycle(&mut self, s: u32, e: u32) {
        let Some(it) = self.opts.iteration else {
            // Each member hits a busy cell in the chain and ends as #CIRC!.
            for slot in s..e {
                self.ensure(slot);
            }
            return;
        };
        for slot in s..e {
            self.values[slot as usize] = Value::Number(0.0);
            self.state[slot as usize] = St::Iter;
        }
        for _ in 0..it.max_iterations.max(1) {
            let mut delta = 0.0f64;
            for slot in s..e {
                let old = self.values[slot as usize].clone();
                self.compute(slot);
                delta = delta.max(change(&old, &self.values[slot as usize]));
            }
            if delta < it.max_change {
                break;
            }
        }
        for slot in s..e {
            self.state[slot as usize] = St::Done;
        }
    }

    fn compute(&mut self, slot: u32) {
        let prog = self.prog;
        let def = &prog.cells[slot as usize];
        let CellKind::Formula(expr) = &def.kind else {
            return;
        };
        let prev = std::mem::replace(&mut self.at, def.at);
        let v = self.eval(expr);
        let v = self.finalize(v);
        self.at = prev;
        self.values[slot as usize] = v;
    }

    fn ensure(&mut self, slot: u32) {
        if self.state[slot as usize] == St::Pending {
            self.state[slot as usize] = St::Busy;
            self.compute(slot);
            self.state[slot as usize] = St::Done;
        }
    }

    /// Value of a cell, evaluating it first if it is still pending.
    pub fn cell_value(&mut self, c: CellRef) -> Value {
        match self.index.get(&c) {
            None => Value::Empty,
            Some(&s) => {
                match self.state[s as usize] {
                    St::Busy => {
                        self.unstable_reads += 1;
                        return Value::Error(ErrorKind::Circ);
                    }
                    St::Iter => self.unstable_reads += 1,
                    _ => {}
                }
                self.ensure(s);
                self.values[s as usize].clone()
            }
        }
    }

    /// Materialize a rectangle, trimmed to the sheet's populated extent.
    pub fn range_array(&mut self, r: Rect) -> Arc<Array> {
        if let Some(a) = self.range_cache.get(&r) {
            return Arc::clone(a);
        }
        let before = self.unstable_reads;
        let a = Arc::new(self.build_range(r));
        if self.unstable_reads == before {
            self.range_cache.insert(r, Arc::clone(&a));
        }
        a
    }

    fn build_range(&mut self, r: Rect) -> Array {
        let (er, ec) = self.extent.get(r.sheet as usize).copied().unwrap_or((0, 0));
        if r.r0 >= er || r.c0 >= ec {
            return Array::filled(1, 1, Value::Empty);
        }
        let r1 = r.r1.min(er - 1);
        let c1 = r.c1.min(ec - 1);
        let (rows, cols) = (r1 - r.r0 + 1, c1 - r.c0 + 1);
        let mut data = vec![Value::Empty; rows as usize * cols as usize];
        let area = u64::from(rows) * u64::from(cols);
        let slots = &self.by_sheet[r.sheet as usize];
        if area <= slots.len() as u64 * 4 + 64 {
            for rr in 0..rows {
                for cc in 0..cols {
                    let at = CellRef::new(r.sheet, r.r0 + rr, r.c0 + cc);
                    if self.index.contains_key(&at) {
                        data[(rr * cols + cc) as usize] = self.cell_value(at);
                    }
                }
            }
        } else {
            let hits: Vec<u32> = slots
                .iter()
                .copied()
                .filter(|&s| {
                    let a = self.prog.cells[s as usize].at;
                    (r.r0..=r1).contains(&a.row) && (r.c0..=c1).contains(&a.col)
                })
                .collect();
            for s in hits {
                let a = self.prog.cells[s as usize].at;
                data[((a.row - r.r0) * cols + (a.col - r.c0)) as usize] = self.cell_value(a);
            }
        }
        Array::new(rows, cols, data)
    }

    pub fn sheet_id(&self, name: &str) -> Option<u16> {
        self.prog
            .sheets
            .iter()
            .position(|s| s.eq_ignore_ascii_case(name))
            .map(|i| i as u16)
    }

    pub fn rand(&mut self) -> f64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    // ----- value access for functions -----

    /// Dereference to a scalar: a single-cell reference reads its cell; a
    /// larger one is intersected with the current cell; an array yields its
    /// top-left element.
    pub fn scalar(&mut self, v: Value) -> Value {
        match v {
            Value::Ref(r) if r.is_single() => self.cell_value(r.top_left()),
            Value::Ref(r) => {
                let at = self.at;
                if r.sheet == at.sheet && r.cols() == 1 && (r.r0..=r.r1).contains(&at.row) {
                    self.cell_value(CellRef::new(r.sheet, at.row, r.c0))
                } else if r.sheet == at.sheet && r.rows() == 1 && (r.c0..=r.c1).contains(&at.col) {
                    self.cell_value(CellRef::new(r.sheet, r.r0, at.col))
                } else {
                    Value::Error(ErrorKind::Value)
                }
            }
            Value::Array(a) => a.data.first().cloned().unwrap_or_default(),
            v => v,
        }
    }

    fn finalize(&mut self, v: Value) -> Value {
        self.scalar(v)
    }

    /// Operand of an operator: a scalar, or `Array` where array evaluation is
    /// in force. Elsewhere a multi-cell reference is intersected with the
    /// current cell (legacy implicit intersection, as in LibreOffice and
    /// non-dynamic Excel), which is `#VALUE!` when they do not meet.
    pub fn operand(&mut self, v: Value) -> Value {
        match v {
            Value::Ref(r) if r.is_single() => self.cell_value(r.top_left()),
            Value::Ref(r) if self.array_depth > 0 => Value::Array(self.range_array(r)),
            v @ Value::Ref(_) => self.scalar(v),
            v => v,
        }
    }

    /// Any value as an array (scalars become 1×1), whatever the context.
    pub fn array_of(&mut self, v: Value) -> Arc<Array> {
        match v {
            Value::Ref(r) if r.is_single() => {
                Arc::new(Array::new(1, 1, vec![self.cell_value(r.top_left())]))
            }
            Value::Ref(r) => self.range_array(r),
            Value::Array(a) => a,
            s => Arc::new(Array::new(1, 1, vec![s])),
        }
    }

    pub fn num(&mut self, v: &Value) -> Res<f64> {
        let s = self.scalar(v.clone());
        coerce::to_num(&s, &self.opts.locale, self.opts.date_system)
    }

    pub fn text(&mut self, v: &Value) -> Res<String> {
        let s = self.scalar(v.clone());
        coerce::to_text(&s, &self.opts.locale)
    }

    pub fn boolean(&mut self, v: &Value) -> Res<bool> {
        let s = self.scalar(v.clone());
        coerce::to_bool(&s, &self.opts.locale)
    }

    /// Flatten arguments for aggregate functions. Numbers found in references
    /// and arrays are collected; text, booleans and blanks there are skipped.
    /// Direct scalar arguments are coerced. Errors propagate.
    pub fn numbers(&mut self, args: &[Value]) -> Res<Vec<f64>> {
        let mut out = Vec::new();
        for a in args {
            match a {
                Value::Ref(_) | Value::Array(_) => {
                    for v in &self.array_of(a.clone()).data {
                        match v {
                            Value::Number(n) => out.push(*n),
                            Value::Error(e) => return Err(*e),
                            _ => {}
                        }
                    }
                }
                Value::Empty => {}
                v => out.push(coerce::to_num(v, &self.opts.locale, self.opts.date_system)?),
            }
        }
        Ok(out)
    }

    // ----- evaluation -----

    pub fn eval(&mut self, e: &Expr) -> Value {
        match e {
            Expr::Const(v) => v.clone(),
            Expr::Cell(c) => Value::Ref(Rect::single(*c)),
            Expr::Range(r) => Value::Ref(*r),
            Expr::Missing => Value::Empty,
            Expr::Unknown(..) => Value::Error(ErrorKind::Name),
            Expr::Unary(op, a) => {
                let v = self.eval(a);
                let v = self.operand(v);
                self.map_array(v, |eng, x| eng.unary(*op, x))
            }
            Expr::Binary(op, a, b) => {
                let l = self.eval(a);
                let l = self.operand(l);
                let r = self.eval(b);
                let r = self.operand(r);
                self.binary(*op, l, r)
            }
            Expr::Call(id, args) => {
                let def = id.def();
                let res = match def.imp {
                    crate::functions::Imp::Lazy(f) => {
                        if args.len() < def.min as usize || args.len() > def.max as usize {
                            Err(ErrorKind::Value)
                        } else {
                            f(self, args)
                        }
                    }
                    crate::functions::Imp::Eager(f) => {
                        if args.len() < def.min as usize || args.len() > def.max as usize {
                            Err(ErrorKind::Value)
                        } else {
                            self.array_depth += u32::from(def.array_args);
                            let vals: Vec<Value> = args.iter().map(|a| self.eval(a)).collect();
                            self.array_depth -= u32::from(def.array_args);
                            let spread = self.array_depth > 0
                                && def.lift
                                && vals.iter().any(|v| {
                                    matches!(v, Value::Array(_))
                                        || matches!(v, Value::Ref(r) if !r.is_single())
                                });
                            if spread {
                                self.lifted(f, &vals)
                            } else {
                                f(self, &vals)
                            }
                        }
                    }
                };
                res.unwrap_or_else(Value::Error)
            }
        }
    }

    /// Apply a scalar function element-wise, broadcasting like operators do.
    fn lifted(&mut self, f: crate::functions::EagerFn, vals: &[Value]) -> Res<Value> {
        let arrays: Vec<Option<Arc<Array>>> = vals
            .iter()
            .map(|v| match v {
                Value::Array(_) => Some(self.array_of(v.clone())),
                Value::Ref(r) if !r.is_single() => Some(self.array_of(v.clone())),
                _ => None,
            })
            .collect();
        let rows = arrays.iter().flatten().map(|a| a.rows).max().unwrap_or(1);
        let cols = arrays.iter().flatten().map(|a| a.cols).max().unwrap_or(1);
        let mut data = Vec::with_capacity(rows as usize * cols as usize);
        for i in 0..rows {
            for j in 0..cols {
                let call: Vec<Value> = vals
                    .iter()
                    .zip(&arrays)
                    .map(|(v, a)| match a {
                        Some(m) => m
                            .get(
                                if m.rows == 1 { 0 } else { i },
                                if m.cols == 1 { 0 } else { j },
                            )
                            .clone(),
                        None => v.clone(),
                    })
                    .collect();
                let r = f(self, &call).unwrap_or_else(Value::Error);
                data.push(self.scalar(r));
            }
        }
        Ok(Value::Array(Arc::new(Array::new(rows, cols, data))))
    }

    fn map_array(&mut self, v: Value, f: impl Fn(&mut Self, Value) -> Value + Copy) -> Value {
        match v {
            Value::Array(a) => {
                let data = a.data.iter().map(|x| f(self, x.clone())).collect();
                Value::Array(Arc::new(Array::new(a.rows, a.cols, data)))
            }
            v => f(self, v),
        }
    }

    fn unary(&mut self, op: UnOp, v: Value) -> Value {
        match op {
            UnOp::Plus => v,
            UnOp::Neg | UnOp::Percent => match self.num(&v) {
                Ok(n) => Value::Number(if op == UnOp::Neg { -n } else { n / 100.0 }),
                Err(e) => Value::Error(e),
            },
        }
    }

    fn binary(&mut self, op: BinOp, l: Value, r: Value) -> Value {
        match (&l, &r) {
            (Value::Array(_), _) | (_, Value::Array(_)) => {
                let (a, b) = (self.array_of(l), self.array_of(r));
                let rows = a.rows.max(b.rows);
                let cols = a.cols.max(b.cols);
                let mut data = Vec::with_capacity(rows as usize * cols as usize);
                for i in 0..rows {
                    for j in 0..cols {
                        let pick = |m: &Array| {
                            let ri = if m.rows == 1 { 0 } else { i };
                            let ci = if m.cols == 1 { 0 } else { j };
                            (ri < m.rows && ci < m.cols).then(|| m.get(ri, ci).clone())
                        };
                        data.push(match (pick(&a), pick(&b)) {
                            (Some(x), Some(y)) => self.scalar_binary(op, &x, &y),
                            _ => Value::Error(ErrorKind::Na),
                        });
                    }
                }
                Value::Array(Arc::new(Array::new(rows, cols, data)))
            }
            _ => self.scalar_binary(op, &l, &r),
        }
    }

    fn scalar_binary(&self, op: BinOp, a: &Value, b: &Value) -> Value {
        if let Value::Error(e) = a {
            return Value::Error(*e);
        }
        if let Value::Error(e) = b {
            return Value::Error(*e);
        }
        let loc = &self.opts.locale;
        let sys = self.opts.date_system;
        match op {
            BinOp::Concat => match (coerce::to_text(a, loc), coerce::to_text(b, loc)) {
                (Ok(x), Ok(y)) => Value::text(x + &y),
                (Err(e), _) | (_, Err(e)) => Value::Error(e),
            },
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ord = compare(a, b);
                Value::Bool(match op {
                    BinOp::Eq => ord == Ordering::Equal,
                    BinOp::Ne => ord != Ordering::Equal,
                    BinOp::Lt => ord == Ordering::Less,
                    BinOp::Le => ord != Ordering::Greater,
                    BinOp::Gt => ord == Ordering::Greater,
                    _ => ord != Ordering::Less,
                })
            }
            _ => {
                let (x, y) = match (coerce::to_num(a, loc, sys), coerce::to_num(b, loc, sys)) {
                    (Ok(x), Ok(y)) => (x, y),
                    (Err(e), _) | (_, Err(e)) => return Value::Error(e),
                };
                let r = match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => {
                        if y == 0.0 {
                            return Value::Error(ErrorKind::DivZero);
                        }
                        x / y
                    }
                    _ => {
                        if x == 0.0 && y == 0.0 {
                            return Value::Error(ErrorKind::Num);
                        }
                        x.powf(y)
                    }
                };
                if r.is_finite() {
                    Value::Number(r)
                } else {
                    Value::Error(ErrorKind::Num)
                }
            }
        }
    }

    /// Final value of a cell by position (for reading outputs).
    pub fn get(&mut self, c: CellRef) -> Value {
        self.cell_value(c)
    }
}

fn change(old: &Value, new: &Value) -> f64 {
    match (old, new) {
        (Value::Number(a), Value::Number(b)) => (a - b).abs(),
        (a, b) if a == b => 0.0,
        _ => f64::INFINITY,
    }
}
