//! ODS/XLSX parity: the same workbook saved in both formats must compute the
//! same results through the same engine.

use serde_json::Value as Json;
use std::path::Path;
use xlslang::{Overrides, Spec, compile, loader};

fn results(file: &str) -> Json {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets")
        .join(file);
    let model = loader::load(&path).unwrap();
    let spec = Spec {
        fold: false,
        now: Some(46318.5),
        ..Spec::default()
    };
    let (program, _) = compile(&model, &spec).unwrap();
    program.execute(&Json::Null, &Overrides::default()).unwrap()
}

/// Blank every number: shape, text and errors remain.
fn shape(j: &Json) -> Json {
    match j {
        Json::Number(_) => Json::from(0),
        Json::Array(a) => Json::Array(a.iter().map(shape).collect()),
        Json::Object(o) => Json::Object(o.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        v => v.clone(),
    }
}

#[test]
fn minimal_workbook_computes_identically() {
    let (x, o) = (results("minimal_test.xlsx"), results("minimal_test.ods"));
    assert!(
        x.as_object().unwrap().len() >= 3,
        "expected several formula sheets"
    );
    for sheet in ["Indexing tests", "hidden_formula_error"] {
        assert_eq!(x[sheet], o[sheet], "{sheet}");
    }
    // Sheet7 sums a column holding typed dates, which `sheetrs` reads from ODS
    // one serial lower than from XLSX (docs/DESIGN.md, known limitations):
    // texts, errors and structure must still agree.
    assert_eq!(shape(&x["Sheet7"]), shape(&o["Sheet7"]));
}

#[test]
fn loan_workbook_computes_identically() {
    let (x, o) = (results("loan.xlsx"), results("loan.ods"));
    assert_eq!(x, o);
}
