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
    let (p, _) = build(&m, &spec(&[], &["A1"]));
    p.execute(&Json::Null, &Overrides::default()).unwrap()["Sheet1!A1"].clone()
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

pub fn matches_oracle(expected: &Value, got: &Json) -> bool {
    match (expected, got) {
        (Value::Number(a), Json::Number(b)) => near(*a, b.as_f64().unwrap()),
        (Value::Text(a), Json::String(b)) => &**a == b,
        (Value::Bool(a), Json::Bool(b)) => a == b,
        (Value::Empty, Json::Null) => true,
        (Value::Error(a), Json::Object(o)) => {
            o.get("error").and_then(Json::as_str) == Some(a.as_str())
        }
        // A formula returning an empty string reads back as empty in ODS.
        (Value::Empty, Json::String(s)) => s.is_empty(),
        _ => false,
    }
}
