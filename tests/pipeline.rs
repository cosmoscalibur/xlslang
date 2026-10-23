mod common;
use common::*;
use serde_json::{Value as Json, json};
use xlslang::ir::CellKind;
use xlslang::options::{Iteration, Locale};
use xlslang::{Overrides, Spec};

fn model() -> xlslang::Model {
    sheet(&[
        ("A1", json!(100)),             // input: price
        ("A2", json!(3)),               // input: qty
        ("B1", json!(0.21)),            // constant: tax rate
        ("B2", json!("=B1*2")),         // folded: depends on constants only
        ("C1", json!("=A1*A2")),        // intermediate (depends on inputs)
        ("C2", json!("=C1*(1+B1)+B2")), // final output
        ("D1", json!("=SUM(1,2,3)")),   // unreachable from the output
        ("D2", json!("=D1*1000")),      // unreachable too
    ])
}

#[test]
fn inputs_outputs_prune_and_fold() {
    let (p, r) = build(&model(), &spec(&["A1", "A2"], &["C2"]));
    // D1/D2 are dead; B2 is folded to a constant; C1 and C2 stay.
    assert_eq!(r.formulas_total, 5);
    assert_eq!(r.formulas_kept, 2);
    assert!(r.formulas_folded >= 1);
    let formulas = p
        .cells
        .iter()
        .filter(|c| matches!(c.kind, CellKind::Formula(_)))
        .count();
    assert_eq!(formulas, 2);
    assert!(
        p.cells.iter().all(|c| !(c.at.col == 3)),
        "column D must be pruned"
    );

    let out = p
        .execute(
            &json!({"Sheet1!A1": 100, "Sheet1!A2": 3}),
            &Overrides::default(),
        )
        .unwrap();
    assert_eq!(out["Sheet1!C2"], json!(300.0 * 1.21 + 0.42));
    // Defaults come from the source file when an input is omitted.
    let out = p
        .execute(&json!({"Sheet1!A2": 1}), &Overrides::default())
        .unwrap();
    assert_eq!(out["Sheet1!C2"], json!(100.0 * 1.21 + 0.42));
}

#[test]
fn unknown_and_malformed_inputs_are_rejected() {
    let (p, _) = build(&model(), &spec(&["A1"], &["C2"]));
    let e = p
        .execute(&json!({"nope": 1}), &Overrides::default())
        .unwrap_err();
    assert!(
        e.0.contains("unknown input `nope`") && e.0.contains("Sheet1!A1"),
        "{e}"
    );
    assert!(p.execute(&json!([1]), &Overrides::default()).is_err());
}

#[test]
fn everything_folds_without_inputs() {
    let (p, r) = build(&model(), &spec(&[], &["C2", "D2"]));
    assert_eq!(r.formulas_kept, 0);
    assert!(p.cells.iter().all(|c| matches!(c.kind, CellKind::Const(_))));
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    assert_eq!(out["Sheet1!D2"], json!(6000));
}

#[test]
fn range_bindings_use_json_matrices() {
    let m = sheet(&[
        ("A1", json!(0)),
        ("A2", json!(0)),
        ("A3", json!(0)),
        ("B1", json!("=A1*2")),
        ("B2", json!("=A2*2")),
        ("B3", json!("=A3*2")),
        ("C1", json!("=SUM(B1:B3)")),
    ]);
    let (p, _) = build(&m, &spec(&["A1:A3"], &["B1:B3", "C1"]));
    for input in [json!([1, 2, 3]), json!([[1], [2], [3]])] {
        let out = p
            .execute(&json!({ "Sheet1!A1:A3": input }), &Overrides::default())
            .unwrap();
        assert_eq!(out["Sheet1!B1:B3"], json!([[2], [4], [6]]));
        assert_eq!(out["Sheet1!C1"], json!(12));
    }
    let e = p
        .execute(&json!({"Sheet1!A1:A3": [1, 2]}), &Overrides::default())
        .unwrap_err();
    assert!(e.0.contains("expects 3"), "{e}");
}

#[test]
fn errors_and_blanks_in_json() {
    let m = sheet(&[
        ("A1", json!(0)),
        ("B1", json!("=A1/0")),
        ("C1", json!("=A2")),
    ]);
    let (p, _) = build(&m, &spec(&["A1"], &["B1", "C1", "D1"]));
    let out = p
        .execute(&json!({"Sheet1!A1": "hi"}), &Overrides::default())
        .unwrap();
    assert_eq!(out["Sheet1!B1"], err("#VALUE!"));
    assert_eq!(out["Sheet1!C1"], Json::Null);
    assert_eq!(out["Sheet1!D1"], Json::Null);
}

#[test]
fn default_outputs_cover_formula_ranges() {
    let (p, _) = build(&model(), &spec(&[], &[]));
    assert_eq!(p.outputs.len(), 1);
    assert_eq!(p.outputs[0].name, "Sheet1!B1:D2");
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    assert_eq!(out["Sheet1!B1:D2"][1][1], json!(300.0 * 1.21 + 0.42)); // C2 in B1:D2
}

#[test]
fn circular_references_and_iteration() {
    let m = sheet(&[
        ("A1", json!("=B1+1")),
        ("B1", json!("=A1+1")),
        ("C1", json!("=A1")),
    ]);
    let (p, r) = build(&m, &spec(&[], &["A1", "C1"]));
    assert_eq!(r.cycles, 1);
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    assert_eq!(out["Sheet1!A1"], err("#CIRC!"));
    assert_eq!(out["Sheet1!C1"], err("#CIRC!"));

    // x = y/2 + 10, y = x  →  x = y = 20.
    let m = sheet(&[
        ("A1", json!("=B1/2+10")),
        ("B1", json!("=A1")),
        ("C1", json!("=A1+B1")),
    ]);
    let mut s = spec(&[], &["A1", "C1"]);
    s.iteration = Some(Iteration {
        max_iterations: 200,
        max_change: 1e-12,
    });
    let (p, _) = build(&m, &s);
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    assert!((out["Sheet1!A1"].as_f64().unwrap() - 20.0).abs() < 1e-9);
    assert!((out["Sheet1!C1"].as_f64().unwrap() - 40.0).abs() < 1e-9);
}

#[test]
fn cycles_through_inputs_survive_to_run_time() {
    // The cycle depends on an input, so it cannot be folded away.
    let m = sheet(&[
        ("A1", json!(1)),
        ("B1", json!("=C1*0.5+A1")),
        ("C1", json!("=B1")),
    ]);
    let mut s = spec(&["A1"], &["B1"]);
    s.iteration = Some(Iteration {
        max_iterations: 500,
        max_change: 1e-12,
    });
    let (p, r) = build(&m, &s);
    assert_eq!(r.formulas_kept, 2);
    assert_eq!(p.cycles.len(), 1);
    let out = p
        .execute(&json!({"Sheet1!A1": 5}), &Overrides::default())
        .unwrap();
    assert!((out["Sheet1!B1"].as_f64().unwrap() - 10.0).abs() < 1e-9);
}

#[test]
fn dynamic_references_resolve_at_run_time() {
    let m = sheet(&[
        ("A1", json!(2)), // input: which row to read
        ("B1", json!(10)),
        ("B2", json!(20)),
        ("B3", json!("=B2*2")), // computed, read dynamically
        ("C1", json!("=INDIRECT(\"B\"&A1)")),
        ("C2", json!("=OFFSET(B1,A1,0)")),
        ("C3", json!("=SUM(OFFSET(B1,0,0,A1,1))")),
        ("C4", json!("=INDIRECT(\"B3\")")), // static: resolved at build time
    ]);
    let (p, r) = build(&m, &spec(&["A1"], &["C1:C4"]));
    assert!(r.dynamic_refs);
    for (row, want) in [
        (1, [10, 20, 10, 40]),
        (2, [20, 40, 30, 40]),
        (3, [40, 0, 70, 40]),
    ] {
        let out = p
            .execute(&json!({"Sheet1!A1": row}), &Overrides::default())
            .unwrap();
        let got: Vec<Json> = out["Sheet1!C1:C4"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r[0].clone())
            .collect();
        // OFFSET(B1,3,0) is B4: empty.
        let want: Vec<Json> = want.iter().map(|v| json!(v)).collect();
        if row == 3 {
            assert_eq!(got[1], Json::Null);
            assert_eq!(got[0], want[0]);
            assert_eq!(got[2], want[2]);
        } else {
            assert_eq!(got, want, "row={row}");
        }
    }
    // The static INDIRECT became a plain reference.
    let (p, r) = build(
        &sheet(&[("A1", json!("=INDIRECT(\"B1\")")), ("B1", json!(4))]),
        &spec(&[], &["A1"]),
    );
    assert!(!r.dynamic_refs);
    assert_eq!(
        p.execute(&Json::Null, &Overrides::default()).unwrap()["Sheet1!A1"],
        json!(4)
    );
}

#[test]
fn run_time_options_and_folding() {
    let m = sheet(&[
        ("A1", json!(1234.5)),
        ("B1", json!("=TEXT(A1,\"#,##0.00\")&\" \"&(1/2)")),
    ]);
    // Folded at build time: overriding the locale later must be refused.
    let (p, _) = build(&m, &spec(&[], &["B1"]));
    let es = Overrides {
        locale: Some(Locale::es_es()),
        ..Default::default()
    };
    assert!(
        p.execute(&Json::Null, &es)
            .unwrap_err()
            .0
            .contains("--no-fold")
    );

    // Built with the locale: baked in.
    let mut s = spec(&[], &["B1"]);
    s.locale = Some(Locale::es_es());
    let (p, _) = build(&m, &s);
    assert_eq!(
        p.execute(&Json::Null, &Overrides::default()).unwrap()["Sheet1!B1"],
        json!("1.234,50 0,5")
    );

    // Not folded: the locale and date system are free at run time.
    let (p, _) = build(
        &m,
        &Spec {
            fold: false,
            ..spec(&[], &["B1"])
        },
    );
    assert_eq!(
        p.execute(&Json::Null, &Overrides::default()).unwrap()["Sheet1!B1"],
        json!("1,234.50 0.5")
    );
    assert_eq!(
        p.execute(&Json::Null, &es).unwrap()["Sheet1!B1"],
        json!("1.234,50 0,5")
    );

    let m = sheet(&[("A1", json!("=DATE(2024,1,1)"))]);
    let (p, _) = build(
        &m,
        &Spec {
            fold: false,
            ..spec(&[], &["A1"])
        },
    );
    let o = Overrides {
        date_system: Some(xlslang::options::DateSystem::Excel1904),
        ..Default::default()
    };
    assert_eq!(
        p.execute(&Json::Null, &Overrides::default()).unwrap()["Sheet1!A1"],
        json!(45292)
    );
    assert_eq!(
        p.execute(&Json::Null, &o).unwrap()["Sheet1!A1"],
        json!(43830)
    );
}

#[test]
fn volatile_functions_are_not_folded() {
    let m = sheet(&[("A1", json!("=RAND()")), ("B1", json!("=TODAY()"))]);
    let (p, r) = build(&m, &spec(&[], &["A1", "B1"]));
    assert_eq!(r.formulas_kept, 2);
    let o = Overrides {
        rand_seed: Some(7),
        now: Some(45292.5),
        ..Default::default()
    };
    let x = p.execute(&Json::Null, &o).unwrap();
    assert_eq!(x, p.execute(&Json::Null, &o).unwrap()); // deterministic when seeded
    assert_eq!(x["Sheet1!B1"], json!(45292));
    let a = x["Sheet1!A1"].as_f64().unwrap();
    assert!((0.0..1.0).contains(&a));
}

#[test]
fn program_roundtrips_through_json() {
    let (p, _) = build(&model(), &spec(&["A1"], &["C2"]));
    let text = serde_json::to_string(&p).unwrap();
    let q: xlslang::Program = serde_json::from_str(&text).unwrap();
    assert_eq!(p, q);
    let o = Overrides::default();
    assert_eq!(
        p.execute(&json!({"Sheet1!A1": 7}), &o).unwrap(),
        q.execute(&json!({"Sheet1!A1": 7}), &o).unwrap()
    );
}

#[test]
fn defined_names_bind_and_parse() {
    let mut m = sheet(&[("A1", json!(5)), ("B1", json!("=Rate*2"))]);
    m.names.insert("Rate".into(), "Sheet1!$A$1".into());
    let (p, _) = build(&m, &spec(&["Rate"], &["B1"]));
    assert_eq!(
        p.execute(&json!({"Sheet1!A1": 4}), &Overrides::default())
            .unwrap()["Sheet1!B1"],
        json!(8)
    );
}

#[test]
fn bad_targets_fail_early() {
    let e = xlslang::compile(&model(), &spec(&["Nope!A1"], &["C2"])).unwrap_err();
    assert!(e.0.contains("cannot resolve"));
    let e = xlslang::compile(&model(), &spec(&["A1", "A1"], &["C2"])).unwrap_err();
    assert!(e.0.contains("declared twice"));
}
