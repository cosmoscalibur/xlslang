#![allow(dead_code)]
use serde_json::{Value as Json, json};
use xlslang::ir::Ns;
use xlslang::{BindingSpec, Model, Overrides, Program, Report, Spec, Value, compile};

pub fn spec(ins: &[&str], outs: &[&str]) -> Spec {
    Spec {
        inputs: ins.iter().map(|s| BindingSpec::parse(s)).collect(),
        outputs: outs.iter().map(|s| BindingSpec::parse(s)).collect(),
        ..Spec::default()
    }
}

pub fn build(m: &Model, s: &Spec) -> (Program, Report) {
    compile(m, s).unwrap_or_else(|e| panic!("compile failed: {e}"))
}

/// Evaluate one formula in an empty workbook.
pub fn calc(formula: &str) -> Json {
    calc_in(Ns::Xlsx, formula)
}

pub fn calc_in(dialect: Ns, formula: &str) -> Json {
    let mut m = Model::new(dialect);
    let s = m.add_sheet("Sheet1");
    m.set_formula(s, "A1", formula);
    let (p, _) = build(&m, &spec(&[], &["r=A1"]));
    p.execute(&Json::Null, &Overrides::default()).unwrap()["r"].clone()
}

/// A workbook with `Sheet1` filled from `(address, value)` pairs, where text
/// starting with `=` is a formula.
pub fn sheet(cells: &[(&str, Json)]) -> Model {
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("Sheet1");
    for (a, v) in cells {
        match v {
            Json::String(t) if t.starts_with('=') => {
                m.set_formula(s, a, t);
            }
            Json::String(t) => {
                m.set(s, a, Value::text(t));
            }
            Json::Number(n) => {
                m.set(s, a, Value::Number(n.as_f64().unwrap()));
            }
            Json::Bool(b) => {
                m.set(s, a, Value::Bool(*b));
            }
            _ => panic!("unsupported fixture value"),
        }
    }
    m
}

pub fn err(code: &str) -> Json {
    json!({ "error": code })
}
