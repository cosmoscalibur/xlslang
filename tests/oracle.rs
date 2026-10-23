//! Cross-check against LibreOffice: `tests/assets/semantics.{xlsx,ods}` hold
//! ~200 formulas whose cached values were computed by LibreOffice (see
//! `scripts/gen_semantics_fixture.py`). The engine must reproduce them from the
//! formulas alone, from both container formats.

use serde_json::Value as Json;
use std::path::Path;
use xlslang::addr::CellRef;
use xlslang::value::Value;
use xlslang::{BindingSpec, Overrides, Spec, compile, loader};

#[derive(Clone, Copy)]
enum Gap {
    /// The oracle value is not the reference: compare nothing.
    Skip,
    /// The oracle conflates error kinds: the engine must still yield an error.
    AnyError,
}

/// Formulas where the oracle itself is not the reference: (label, file suffix
/// or "" for all, handling, reason).
const ORACLE_GAPS: &[(&str, &str, Gap, &str)] = &[
    (
        "_xlfn.XLOOKUP(\"cherry\",Data!B1:B8,Data!A1:A8)",
        "",
        Gap::Skip,
        "LibreOffice 24.2 has no XLOOKUP (added in 24.8)",
    ),
    (
        "_xlfn.XLOOKUP(\"q\",Data!B1:B8,Data!A1:A8,\"none\")",
        "",
        Gap::Skip,
        "same",
    ),
    (
        "0^0",
        "",
        Gap::Skip,
        "LibreOffice gives 1, Excel #NUM!; the engine follows Excel (see docs/DESIGN.md, dialect semantics)",
    ),
    (
        "SQRT(-1)",
        "",
        Gap::AnyError,
        "LibreOffice reports every invalid argument as Err:502 / #VALUE!",
    ),
    ("LN(0)", "", Gap::AnyError, "same"),
    ("VALUE(\"abc\")", "", Gap::AnyError, "same"),
    (
        "Data!A1:A3*2",
        "xlsx",
        Gap::Skip,
        "sheetrs reads a cached #VALUE! of a formula holding `:` as no value (#62)",
    ),
    ("SUM(Data!A1:A3*2)", "xlsx", Gap::Skip, "same"),
    ("SUM(Data!A1:A3>3)", "xlsx", Gap::Skip, "same"),
    (
        "\"[\"&100000000000000000000&\"]\"",
        "",
        Gap::Skip,
        "LibreOffice writes the exponent with three digits (1E+020)",
    ),
];

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn same(expected: &Value, got: &Json) -> bool {
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

fn check(file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets")
        .join(file);
    let model = loader::load(&path).unwrap();
    let calc = model.sheet_id("Calc").unwrap();
    let mut rows: Vec<u32> = model
        .cells
        .iter()
        .filter(|(at, c)| at.sheet == calc && at.col == 0 && c.formula.is_some())
        .map(|(at, _)| at.row)
        .collect();
    rows.sort_unstable();
    assert!(rows.len() > 150, "fixture looks truncated: {}", rows.len());
    let last = *rows.last().unwrap();
    let spec = Spec {
        outputs: vec![BindingSpec::parse(&format!("r=Calc!A1:A{}", last + 1))],
        fold: false,
        ..Spec::default()
    };
    let (program, report) = compile(&model, &spec).unwrap();
    let _ = report;
    let out = program.execute(&Json::Null, &Overrides::default()).unwrap();
    let col = out["r"].as_array().unwrap();

    let mut bad = Vec::new();
    for &row in &rows {
        let cell = &model.cells[&CellRef::new(calc, row, 0)];
        let label =
            model
                .cells
                .get(&CellRef::new(calc, row, 1))
                .map_or(String::new(), |c| match &c.value {
                    Value::Text(t) => t.to_string(),
                    _ => String::new(),
                });
        let got = &col[row as usize][0];
        if let Some((_, _, gap, _)) = ORACLE_GAPS
            .iter()
            .find(|(f, ext, _, _)| *f == label && (ext.is_empty() || file.ends_with(ext)))
        {
            if matches!(gap, Gap::AnyError) && !matches!(got, Json::Object(_)) {
                bad.push(format!(
                    "  A{:<4} {label}\n        oracle: error\n        engine: {got}",
                    row + 1
                ));
            }
            continue;
        }
        if !same(&cell.value, got) {
            bad.push(format!(
                "  A{:<4} {label}\n        oracle: {:?}\n        engine: {got}",
                row + 1,
                cell.value
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{file}: {} mismatches\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn xlsx_matches_libreoffice() {
    check("semantics.xlsx");
}

#[test]
fn ods_matches_libreoffice() {
    check("semantics.ods");
}
