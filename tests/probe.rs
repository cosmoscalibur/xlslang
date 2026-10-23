//! Fidelity probes: formulas on which Excel and LibreOffice are known or
//! suspected to differ (`scripts/probe_dialect.py`, documented in
//! docs/FIDELITY.md). The ODS file holds LibreOffice's answers; the engine in
//! the `ods` dialect must reproduce them. The same formulas under the Excel
//! dialect must differ exactly where docs/FIDELITY.md says so.

mod common;

use serde_json::{Value as Json, json};
use std::path::Path;
use xlslang::addr::CellRef;
use xlslang::ir::Ns;
use xlslang::value::Value;
use xlslang::{BindingSpec, Model, Overrides, Spec, compile, loader};

/// Probes the engine deliberately does not reproduce, with the reason.
const NOT_REPRODUCED: &[(&str, &str)] = &[
    (
        "UPPER(\"ß\")",
        "Unicode case mapping: LibreOffice gives ẞ, the engine SS (Rust `to_uppercase`)",
    ),
    (
        "TRIM(CHAR(160)&\"a\")",
        "CHAR(160) depends on the code page; LibreOffice yields U+FFFD",
    ),
    (
        "DATEDIF(DATE(2024,1,31),DATE(2024,3,1),\"MD\")",
        "DATEDIF unit \"MD\" is not implemented",
    ),
    (
        "DAYS(\"2024-03-01\",\"2024-02-01\")",
        "LibreOffice 24.2 reads DAYS without the _xlfn. prefix as unknown",
    ),
    (
        "DATEVALUE(\"3/1/2024\")",
        "text dates are parsed in ISO form only (locale-bound formats are not implemented)",
    ),
];

/// Error-valued probes where LibreOffice's error kind is not meaningful
/// (every invalid argument is Err:502): only "is an error" is compared.
const ANY_ERROR: &[&str] = &[
    "CEILING(2.1,-2)",
    "FLOOR(2.1,-2)",
    "LOG(1,1)",
    "FACT(171)",
    "SQRT(-1)",
];

fn run(model: &Model, dialect: Option<Ns>, n: u32) -> Vec<Json> {
    let spec = Spec {
        outputs: vec![BindingSpec::parse(&format!("Calc!A1:A{n}"))],
        dialect,
        fold: false,
        ..Spec::default()
    };
    let (p, _) = compile(model, &spec).unwrap();
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    out[&p.outputs[0].name]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].clone())
        .collect()
}

fn label(m: &Model, calc: u16, row: u32) -> String {
    match &m.cells[&CellRef::new(calc, row, 1)].value {
        Value::Text(t) => t.to_string(),
        _ => String::new(),
    }
}

#[test]
fn ods_dialect_reproduces_libreoffice() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/probe.ods");
    let m = loader::load(&path).unwrap();
    assert_eq!(m.dialect, Ns::Ods);
    let calc = m.sheet_id("Calc").unwrap();
    let n = m
        .cells
        .keys()
        .filter(|a| a.sheet == calc && a.col == 0)
        .count() as u32;
    let got = run(&m, None, n);
    let mut bad = Vec::new();
    for row in 0..n {
        let l = label(&m, calc, row);
        if NOT_REPRODUCED.iter().any(|(f, _)| *f == l) {
            continue;
        }
        let expected = &m.cells[&CellRef::new(calc, row, 0)].value;
        let g = &got[row as usize];
        let ok = if ANY_ERROR.contains(&l.as_str()) {
            matches!(expected, Value::Error(_)) && g.get("error").is_some()
        } else {
            common::matches_oracle(expected, g)
        };
        if !ok {
            bad.push(format!("  {l:<46} LibreOffice: {expected:?}   engine: {g}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The documented differences, pinned: under the Excel dialect the same
/// formulas give Excel's answers.
#[test]
fn excel_dialect_differs_where_documented() {
    let calc = |f: &str, d: Ns| common::calc_in(d, f);
    let cases: &[(&str, Json, Json)] = &[
        ("0^0", json!(1), common::err("#NUM!")),
        ("(-8)^(1/3)", json!(-2), common::err("#NUM!")),
        ("TRUE&\"\"", json!("1"), json!("TRUE")),
        ("TEXT(TRUE,\"0\")", json!("1"), json!("TRUE")),
        ("\"a\"<TRUE", json!(false), json!(true)),
        ("TRUE=1", json!(true), json!(false)),
        ("FALSE<1", json!(true), json!(false)),
        ("ISNUMBER(TRUE)", json!(true), json!(false)),
        ("SUM(\"1\")", common::err("#VALUE!"), json!(1)),
        (
            "\"[\"&1E-10&\"]\"",
            json!("[0.0000000001]"),
            json!("[1E-10]"),
        ),
        ("\"[\"&1E+20&\"]\"", json!("[1E+020]"), json!("[1E+20]")),
        ("DATE(1900,1,1)", json!(2), json!(1)),
        ("DATE(1900,2,28)", json!(60), json!(59)),
        ("YEAR(0)", json!(1899), json!(1900)),
        ("DATE(1850,1,1)", json!(-18260), json!(675701)), // Excel: 1850 + 1900 = 3750
        ("DATE(29,1,1)", json!(47119), json!(10594)),
    ];
    for (f, lo, xl) in cases {
        assert_eq!(&calc(f, Ns::Ods), lo, "ods: {f}");
        assert_eq!(&calc(f, Ns::Xlsx), xl, "xlsx: {f}");
    }
}
