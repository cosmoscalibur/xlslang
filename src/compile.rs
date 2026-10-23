//! The compiler: [`Model`] + input/output specification → [`Program`].
//!
//! Pipeline:
//!
//! 1. Parse every formula to IR and resolve statically-known `INDIRECT` /
//!    `OFFSET` targets into plain references.
//! 2. Build the dependency graph lazily from the requested outputs backwards,
//!    cutting at input cells. Anything outside that cone is dead and dropped.
//! 3. Order the cone with Tarjan SCC (dependencies first, cycles isolated).
//! 4. Mark cells that depend on an input, a volatile function or a dynamic
//!    reference as *live*; evaluate everything else now with the very same
//!    engine and keep only the resulting constants. What remains is the
//!    minimal run-time computation.

use crate::addr::{self, CellRef, Rect};
use crate::engine::Engine;
use crate::formula::{self, ParseCtx};
use crate::graph::{self, Csr};
use crate::ir::{Binding, CellDef, CellKind, Expr, Ns, PROGRAM_VERSION, Program};
use crate::model::Model;
use crate::options::{DatePatterns, DateSystem, EngineOptions, Iteration, Locale, Search};
use crate::value::{ErrorKind, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A cell or range declared as input or output. The engine knows nothing
/// else about it: JSON keys are the canonical range text (`Sheet1!B2:C9`), and
/// naming them is the job of a layer above.
#[derive(Debug, Clone)]
pub struct BindingSpec {
    /// `Sheet1!A1`, `Sheet1!A1:B3`, `A1` (first sheet) or a defined name.
    pub target: String,
}

impl BindingSpec {
    pub fn parse(s: &str) -> Self {
        Self {
            target: s.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub inputs: Vec<BindingSpec>,
    /// Empty: one binding per sheet covering its formula cells.
    pub outputs: Vec<BindingSpec>,
    /// Defaults to the date system of the source workbook.
    pub date_system: Option<DateSystem>,
    /// Defaults to the dialect of the source file (from its extension).
    pub dialect: Option<Ns>,
    pub iteration: Option<Iteration>,
    pub locale: Option<Locale>,
    pub search: Option<Search>,
    pub now: Option<f64>,
    pub rand_seed: Option<u64>,
    /// Evaluate input-independent cells at build time.
    pub fold: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            date_system: None,
            dialect: None,
            iteration: None,
            locale: None,
            search: None,
            now: None,
            rand_seed: None,
            fold: true,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub cells_total: usize,
    pub formulas_total: usize,
    /// Cells reachable from the outputs.
    pub cells_reachable: usize,
    /// Formulas evaluated at build time and replaced by constants.
    pub formulas_folded: usize,
    /// Formulas that remain in the program.
    pub formulas_kept: usize,
    pub cells_kept: usize,
    pub cycles: usize,
    /// A reference computed at run time forced the whole workbook to be kept.
    pub dynamic_refs: bool,
    pub unsupported: BTreeSet<String>,
}

#[derive(Debug)]
pub struct CompileError(pub String);

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompileError {}

fn err<T>(msg: impl Into<String>) -> Result<T, CompileError> {
    Err(CompileError(msg.into()))
}

const MAX_BINDING_CELLS: u64 = 10_000_000;

/// Resolve a user-facing target to a rectangle.
pub fn resolve_target(model: &Model, target: &str) -> Result<Rect, CompileError> {
    let (sheet, tail) = addr::split_sheet(target.trim());
    let sheet_id = match &sheet {
        Some(n) => model.sheet_id(n),
        None => Some(0),
    };
    if let (Some(s), Some(area)) = (sheet_id, addr::parse_area(tail)) {
        return Ok(area.bind(s));
    }
    if sheet.is_none()
        && let Some(def) = model
            .names
            .iter()
            .find(|(k, _)| addr::same_name(k, target.trim()))
            .map(|(_, v)| v.as_str())
    {
        let (sh, t) = addr::split_sheet(def.trim_start_matches('='));
        if let (Some(sh), Some(area)) = (
            sh.as_deref().and_then(|n| model.sheet_id(n)),
            addr::parse_area(t),
        ) {
            return Ok(area.bind(sh));
        }
    }
    err(format!("cannot resolve `{target}` to a cell or range"))
}

#[derive(Default)]
struct Flags {
    dynamic: bool,
    volatile: bool,
}

pub fn compile(model: &Model, spec: &Spec) -> Result<(Program, Report), CompileError> {
    if model.from_file
        && let Some(what) = [
            (spec.date_system.is_some(), "date system"),
            (spec.dialect.is_some(), "dialect"),
            (spec.iteration.is_some(), "iteration"),
            (spec.search.is_some(), "search settings"),
        ]
        .iter()
        .find_map(|(set, name)| set.then_some(*name))
    {
        return err(format!(
            "the {what} is defined by the document and cannot be overridden"
        ));
    }
    let mut report = Report::default();
    let mut diags: Vec<String> = model.diagnostics.clone();
    let mut parse_diags: Vec<String> = Vec::new();

    // ----- 1. lower cells -----
    let mut nodes: BTreeMap<CellRef, CellKind> = BTreeMap::new();
    let mut keys: Vec<&CellRef> = model.cells.keys().collect();
    keys.sort();
    for at in keys {
        let c = &model.cells[at];
        report.cells_total += 1;
        let kind = match &c.formula {
            None => CellKind::Const(c.value.clone()),
            Some(f) => {
                report.formulas_total += 1;
                let ctx = ParseCtx {
                    sheets: &model.sheets,
                    current_sheet: at.sheet,
                    names: &model.names,
                    dialect: model.dialect,
                };
                let before = parse_diags.len();
                let kind = match formula::parse(f, &ctx, &mut parse_diags) {
                    Ok(mut e) => {
                        fold_refs(&mut e, *at, &model.sheets);
                        if c.array
                            && let Some(id) =
                                crate::functions::lookup(crate::ir::Ns::Core, "_ARRAY")
                        {
                            e = Expr::Call(id, vec![e]);
                        }
                        e.walk(&mut |x| {
                            if let Expr::Unknown(name, _) = x {
                                report.unsupported.insert(name.to_string());
                            }
                        });
                        CellKind::Formula(e)
                    }
                    // Syntax the parser does not know (external workbook
                    // references, structured references): keep the value the
                    // file last computed.
                    Err(e) => {
                        parse_diags
                            .push(format!("unsupported syntax, using the cached value ({e})"));
                        CellKind::Const(if c.value == Value::Empty {
                            Value::Error(ErrorKind::Name)
                        } else {
                            c.value.clone()
                        })
                    }
                };
                let sheet = model
                    .sheets
                    .get(at.sheet as usize)
                    .map_or("?", String::as_str);
                for d in &mut parse_diags[before..] {
                    *d = format!("{sheet}!{}: {d}", addr::a1(at.row, at.col));
                }
                kind
            }
        };
        nodes.insert(*at, kind);
    }

    // ----- inputs / outputs -----
    let mut inputs = Vec::new();
    for b in &spec.inputs {
        let rect = resolve_target(model, &b.target)?;
        if rect.area() > MAX_BINDING_CELLS {
            return err(format!("input `{}` spans too many cells", b.target));
        }
        let name = addr::qualified(&model.sheets, &rect);
        for r in rect.r0..=rect.r1 {
            for c in rect.c0..=rect.c1 {
                let at = CellRef::new(rect.sheet, r, c);
                let default = match model.cells.get(&at) {
                    Some(mc) => mc.value.clone(),
                    None => Value::Empty,
                };
                nodes.insert(at, CellKind::Input(default));
            }
        }
        inputs.push(Binding { name, rect });
    }
    let mut outputs = Vec::new();
    for b in &spec.outputs {
        let rect = resolve_target(model, &b.target)?;
        if rect.area() > MAX_BINDING_CELLS {
            return err(format!("output `{}` spans too many cells", b.target));
        }
        outputs.push(Binding {
            name: addr::qualified(&model.sheets, &rect),
            rect,
        });
    }
    if spec.outputs.is_empty() {
        for s in 0..model.sheets.len() {
            let mut bb: Option<(u32, u32, u32, u32)> = None;
            for (at, k) in &nodes {
                if at.sheet as usize == s && matches!(k, CellKind::Formula(_)) {
                    bb = Some(match bb {
                        None => (at.row, at.col, at.row, at.col),
                        Some((r0, c0, r1, c1)) => (
                            r0.min(at.row),
                            c0.min(at.col),
                            r1.max(at.row),
                            c1.max(at.col),
                        ),
                    });
                }
            }
            if let Some((r0, c0, r1, c1)) = bb {
                outputs.push(Binding {
                    name: addr::qualified(&model.sheets, &Rect::new(s as u16, r0, c0, r1, c1)),
                    rect: Rect::new(s as u16, r0, c0, r1, c1),
                });
            }
        }
    }
    for list in [&inputs, &outputs] {
        let mut names = HashSet::new();
        for b in list {
            if !names.insert(b.name.as_str()) {
                return err(format!("`{}` is declared twice", b.name));
            }
        }
    }

    let list: Vec<(CellRef, CellKind)> = nodes.into_iter().collect();
    let n = list.len();
    let mut ids: HashMap<CellRef, u32> = HashMap::with_capacity(n);
    let mut by_sheet: Vec<Vec<u32>> = vec![Vec::new(); model.sheets.len().max(1)];
    for (i, (at, _)) in list.iter().enumerate() {
        ids.insert(*at, i as u32);
        by_sheet[at.sheet as usize].push(i as u32);
    }

    // ----- 2. dependency graph, lazily, from the outputs -----
    let mut g = Lazy {
        list: &list,
        ids: &ids,
        by_sheet: &by_sheet,
        adj: vec![None; n],
        rect_ids: HashMap::new(),
        rect_members: Vec::new(),
        flags: HashMap::new(),
    };
    let mut roots: Vec<u32> = Vec::new();
    for b in &outputs {
        g.members(&b.rect, &mut roots);
    }
    let mut visited = vec![false; n];
    let mut cone: Vec<u32> = Vec::new();
    let mut stack = roots;
    let mut all = false;
    while let Some(v) = stack.pop() {
        if g.seen(&mut visited, v) {
            continue;
        }
        cone.push(v);
        let adj = g.adj_of(v).to_vec();
        if (v as usize) < n && g.flags.get(&v).is_some_and(|f| f.dynamic) && !all {
            all = true;
            report.dynamic_refs = true;
            stack.extend(0..n as u32);
        }
        stack.extend(adj);
    }
    // Compact ids over the cone.
    let mut compact = vec![u32::MAX; visited.len()];
    for (i, &v) in cone.iter().enumerate() {
        compact[v as usize] = i as u32;
    }
    let adj_c: Vec<Vec<u32>> = cone
        .iter()
        .map(|&v| g.adj_of(v).iter().map(|&w| compact[w as usize]).collect())
        .collect();
    let csr = Csr::from_adjacency(&adj_c);
    let sccs = graph::tarjan(&csr);
    report.cells_reachable = cone.iter().filter(|&&v| (v as usize) < n).count();

    // ----- 3/4. taint and fold -----
    let mut tainted = vec![false; cone.len()];
    let mut comp_cyclic = Vec::new();
    for comp in sccs.components() {
        let mut t = false;
        for &m in comp {
            let v = cone[m as usize];
            if (v as usize) < n {
                match &list[v as usize].1 {
                    CellKind::Input(_) => t = true,
                    CellKind::Formula(_) => {
                        let f = g.flags.get(&v);
                        t |= !spec.fold || f.is_some_and(|f| f.dynamic || f.volatile);
                    }
                    CellKind::Const(_) => {}
                }
            }
            t |= csr.edges(m).iter().any(|&d| tainted[d as usize]);
        }
        for &m in comp {
            tainted[m as usize] = t;
        }
        comp_cyclic.push(graph::is_cycle(&csr, comp));
    }

    // Precedence: explicit option > what the document states > default.
    // The locale is the user's regional setting, which a file does not carry
    // (LibreOffice evaluates a document whose language says `es-ES` with the
    // system locale), so it comes from the caller and defaults to `en`. The
    // document's language is informational.
    let mut locale = spec.locale.clone().unwrap_or_default();
    if let Some(lang) = &model.settings.locale
        && !lang
            .to_lowercase()
            .starts_with(locale.tag.split('-').next().unwrap_or("en"))
    {
        diags.push(format!(
            "the document language is {lang}; text and number conversions follow the locale {} (--locale)",
            locale.tag
        ));
    }
    locale.date_patterns = if spec.dialect.unwrap_or(model.dialect) == Ns::Ods {
        DatePatterns::Libre
    } else {
        DatePatterns::Excel
    };
    // The two-digit-year window, in contrast, is stored in the document.
    if let Some(y) = model.settings.two_digit_year_start {
        locale.two_digit_year_start = y;
    }
    let opts = EngineOptions {
        date_system: spec.date_system.unwrap_or(model.date_system),
        iteration: spec.iteration.or(model.settings.iteration),
        locale,
        dialect: spec.dialect.unwrap_or(model.dialect),
        search: spec.search.or(model.settings.search).unwrap_or_default(),
        now: spec.now,
        rand_seed: spec.rand_seed,
    };

    // Evaluate the input-independent cells now.
    let mut folded: HashMap<u32, Value> = HashMap::new();
    {
        let mut cells = Vec::new();
        let mut cycles = Vec::new();
        for (ci, comp) in sccs.components().enumerate() {
            if tainted[comp[0] as usize] {
                continue;
            }
            let start = cells.len() as u32;
            for &m in comp {
                let v = cone[m as usize];
                if (v as usize) < n {
                    cells.push(CellDef {
                        at: list[v as usize].0,
                        kind: list[v as usize].1.clone(),
                    });
                }
            }
            if comp_cyclic[ci] {
                cycles.push((start, cells.len() as u32));
            }
        }
        let tmp = Program {
            version: PROGRAM_VERSION,
            sheets: model.sheets.clone(),
            options: opts.clone(),
            folded_with: None,
            cells,
            cycles,
            inputs: Vec::new(),
            outputs: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut eng = Engine::new(&tmp, opts.clone());
        eng.run();
        for d in &tmp.cells {
            let id = ids[&d.at];
            let v = eng.get(d.at);
            if matches!(d.kind, CellKind::Formula(_)) {
                report.formulas_folded += 1;
            }
            folded.insert(id, v);
        }
    }

    // Which untainted cells does the surviving computation still read?
    let mut needed: HashSet<u32> = HashSet::new();
    let mut expanded: HashSet<u32> = HashSet::new();
    for (i, _) in cone.iter().enumerate().filter(|(i, _)| tainted[*i]) {
        for &d in csr.edges(i as u32) {
            let w = cone[d as usize];
            if (w as usize) < n {
                needed.insert(w);
            } else if expanded.insert(w) {
                needed.extend(g.rect_members[w as usize - n].iter().copied());
            }
        }
    }
    let mut out_cells = Vec::new();
    for b in &outputs {
        g.members(&b.rect, &mut out_cells);
    }
    needed.extend(out_cells);
    if all {
        needed.extend(cone.iter().copied().filter(|&v| (v as usize) < n));
    }

    // ----- assemble -----
    let mut consts: Vec<u32> = needed
        .into_iter()
        .filter(|&v| compact[v as usize] != u32::MAX && !tainted[compact[v as usize] as usize])
        .collect();
    consts.sort_unstable(); // ids follow (sheet, row, col) order
    let mut cells: Vec<CellDef> = consts
        .iter()
        .map(|&v| CellDef {
            at: list[v as usize].0,
            kind: CellKind::Const(folded[&v].clone()),
        })
        .collect();
    let mut cycles = Vec::new();
    for (ci, comp) in sccs.components().enumerate() {
        if !tainted[comp[0] as usize] {
            continue;
        }
        let start = cells.len() as u32;
        for &m in comp {
            let v = cone[m as usize];
            if (v as usize) < n {
                let (at, kind) = &list[v as usize];
                if matches!(kind, CellKind::Formula(_)) {
                    report.formulas_kept += 1;
                }
                cells.push(CellDef {
                    at: *at,
                    kind: kind.clone(),
                });
            }
        }
        if comp_cyclic[ci] {
            cycles.push((start, cells.len() as u32));
        }
    }
    report.cycles = comp_cyclic.iter().filter(|&&c| c).count();
    report.cells_kept = cells.len();

    parse_diags.sort();
    parse_diags.dedup();
    diags.extend(parse_diags.into_iter().take(200));
    if report.cycles > 0 && spec.iteration.is_none() {
        diags.push(format!(
            "{} circular reference group(s) found; they evaluate to #CIRC! unless iteration is enabled",
            report.cycles
        ));
    }

    Ok((
        Program {
            version: PROGRAM_VERSION,
            sheets: model.sheets.clone(),
            options: opts.clone(),
            folded_with: spec.fold.then_some(opts),
            cells,
            cycles,
            inputs,
            outputs,
            diagnostics: diags,
        },
        report,
    ))
}

/// Lazily-built dependency adjacency over cell nodes (`0..n`) and shared
/// rectangle nodes (`n..`).
struct Lazy<'a> {
    list: &'a [(CellRef, CellKind)],
    ids: &'a HashMap<CellRef, u32>,
    by_sheet: &'a [Vec<u32>],
    adj: Vec<Option<Vec<u32>>>,
    rect_ids: HashMap<Rect, u32>,
    rect_members: Vec<Vec<u32>>,
    flags: HashMap<u32, Flags>,
}

impl Lazy<'_> {
    fn n(&self) -> u32 {
        self.list.len() as u32
    }

    fn seen(&self, visited: &mut Vec<bool>, v: u32) -> bool {
        if visited.len() <= v as usize {
            visited.resize(v as usize + 1, false);
        }
        std::mem::replace(&mut visited[v as usize], true)
    }

    /// Existing cells inside `r`.
    fn members(&self, r: &Rect, out: &mut Vec<u32>) {
        let slots = &self.by_sheet[r.sheet as usize];
        if r.area() <= slots.len() as u64 * 4 + 64 {
            for row in r.r0..=r.r1 {
                for col in r.c0..=r.c1 {
                    if let Some(&id) = self.ids.get(&CellRef::new(r.sheet, row, col)) {
                        out.push(id);
                    }
                }
            }
        } else {
            out.extend(
                slots
                    .iter()
                    .copied()
                    .filter(|&s| r.contains(self.list[s as usize].0)),
            );
        }
    }

    fn rect_node(&mut self, r: Rect) -> Option<u32> {
        if let Some(&id) = self.rect_ids.get(&r) {
            return Some(id);
        }
        let mut m = Vec::new();
        self.members(&r, &mut m);
        match m.len() {
            0 => None,
            1 => Some(m[0]),
            _ => {
                let id = self.n() + self.rect_members.len() as u32;
                self.rect_members.push(m);
                self.adj.push(None);
                self.rect_ids.insert(r, id);
                Some(id)
            }
        }
    }

    fn adj_of(&mut self, v: u32) -> &[u32] {
        if self.adj.len() <= v as usize {
            self.adj.resize(v as usize + 1, None);
        }
        if self.adj[v as usize].is_none() {
            let deps = if v < self.n() {
                match &self.list[v as usize].1 {
                    CellKind::Formula(e) => {
                        let (deps, flags) = dependencies(e);
                        self.flags.insert(v, flags);
                        let mut out = Vec::new();
                        for r in deps {
                            out.extend(self.rect_node(r));
                        }
                        out.sort_unstable();
                        out.dedup();
                        out
                    }
                    _ => Vec::new(),
                }
            } else {
                self.rect_members[(v - self.n()) as usize].clone()
            };
            self.adj[v as usize] = Some(deps);
        }
        self.adj[v as usize].as_deref().unwrap_or(&[])
    }
}

fn dependencies(e: &Expr) -> (Vec<Rect>, Flags) {
    let mut deps = Vec::new();
    let mut flags = Flags::default();
    e.walk(&mut |x| match x {
        Expr::Cell(c) => deps.push(Rect::single(*c)),
        Expr::Range(r) => deps.push(*r),
        Expr::Call(id, _) => {
            let d = id.def();
            flags.dynamic |= d.dynamic;
            flags.volatile |= d.volatile;
        }
        _ => {}
    });
    (deps, flags)
}

/// Resolve `INDIRECT`/`OFFSET`/`_RANGE` whose arguments are known at build
/// time into plain references, so the graph (and the optimizer) sees them.
pub fn fold_refs(e: &mut Expr, at: CellRef, sheets: &[String]) {
    e.walk_mut(&mut |x| {
        let Expr::Call(id, args) = x else { return };
        let folded = match id.def().name {
            "_RANGE" => match (rect_of(&args[0]), rect_of(&args[1])) {
                (Some(p), Some(q)) if p.sheet == q.sheet => Some(Rect::new(
                    p.sheet,
                    p.r0.min(q.r0),
                    p.c0.min(q.c0),
                    p.r1.max(q.r1),
                    p.c1.max(q.c1),
                )),
                _ => None,
            },
            "OFFSET" => offset_const(args),
            "INDIRECT" => indirect_const(args, at, sheets),
            _ => None,
        };
        if let Some(r) = folded {
            *x = if r.is_single() {
                Expr::Cell(r.top_left())
            } else {
                Expr::Range(r)
            };
        }
    });
}

fn indirect_const(args: &[Expr], at: CellRef, sheets: &[String]) -> Option<Rect> {
    let Expr::Const(Value::Text(t)) = &args[0] else {
        return None;
    };
    if !matches!(args.get(1), None | Some(Expr::Const(Value::Bool(true)))) {
        return None;
    }
    let (sheet, tail) = addr::split_sheet(t.trim());
    let sheet = match sheet {
        Some(n) => sheets.iter().position(|s| addr::same_name(s, &n))? as u16,
        None => at.sheet,
    };
    addr::parse_area(tail).map(|a| a.bind(sheet))
}

fn rect_of(e: &Expr) -> Option<Rect> {
    match e {
        Expr::Cell(c) => Some(Rect::single(*c)),
        Expr::Range(r) => Some(*r),
        _ => None,
    }
}

fn const_num(e: &Expr) -> Option<f64> {
    match e {
        Expr::Const(Value::Number(n)) => Some(*n),
        _ => None,
    }
}

fn offset_const(args: &[Expr]) -> Option<Rect> {
    let base = rect_of(&args[0])?;
    let dr = const_num(&args[1])?.trunc() as i64;
    let dc = const_num(&args[2])?.trunc() as i64;
    let h = match args.get(3) {
        None | Some(Expr::Missing) => i64::from(base.rows()),
        Some(e) => const_num(e)?.trunc() as i64,
    };
    let w = match args.get(4) {
        None | Some(Expr::Missing) => i64::from(base.cols()),
        Some(e) => const_num(e)?.trunc() as i64,
    };
    let (r0, c0) = (i64::from(base.r0) + dr, i64::from(base.c0) + dc);
    let (r1, c1) = (r0 + h - 1, c0 + w - 1);
    if h < 1
        || w < 1
        || r0 < 0
        || c0 < 0
        || r1 >= i64::from(addr::MAX_ROWS)
        || c1 >= i64::from(addr::MAX_COLS)
    {
        return None;
    }
    Some(Rect::new(
        base.sheet, r0 as u32, c0 as u32, r1 as u32, c1 as u32,
    ))
}
