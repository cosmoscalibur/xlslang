//! Settings stored in the documents: iterative calculation, search mode,
//! two-digit-year window, document language.

use serde_json::{Value as Json, json};
use std::path::Path;
use xlslang::ir::Ns;
use xlslang::options::{Iteration, SearchMode};
use xlslang::{BindingSpec, Overrides, Spec, compile, loader};

fn load(name: &str) -> xlslang::Model {
    loader::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/assets")
            .join(name),
    )
    .unwrap()
}

fn loop_values(model: &xlslang::Model, spec: Spec) -> Vec<Json> {
    let spec = Spec {
        outputs: vec![BindingSpec::parse("Loop!A1:C1")],
        ..spec
    };
    let (p, _) = compile(model, &spec).unwrap();
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    out["Loop!A1:C1"][0].as_array().unwrap().clone()
}

#[test]
fn iteration_stored_in_the_document_is_applied_without_flags() {
    for file in ["circular_on.xlsx", "circular_on.ods"] {
        let m = load(file);
        assert_eq!(
            m.settings.iteration,
            Some(Iteration {
                max_iterations: 50,
                max_change: 0.0001
            }),
            "{file}"
        );
        // LibreOffice (which computed the cached 19.9999…) stops at the
        // tolerance; the engine must land on the same fixed point.
        let v = loop_values(&m, Spec::default());
        for (got, want) in v.iter().zip([20.0, 20.0, 40.0]) {
            assert!((got.as_f64().unwrap() - want).abs() < 1e-3, "{file}: {v:?}");
        }
    }
}

#[test]
fn iteration_off_in_the_document_means_circular_errors() {
    for file in ["circular_off.xlsx", "circular_off.ods"] {
        let m = load(file);
        assert_eq!(m.settings.iteration, None, "{file}");
        let v = loop_values(&m, Spec::default());
        assert!(v.iter().all(|x| x["error"] == "#CIRC!"), "{file}: {v:?}");
    }
}

#[test]
fn explicit_options_win_over_the_document() {
    let on = load("circular_on.ods");
    let spec = Spec {
        iteration: Some(Iteration {
            max_iterations: 1,
            max_change: 0.0,
        }),
        ..Spec::default()
    };
    let one = loop_values(&on, spec);
    // A single iteration is far from the fixed point.
    assert!((one[0].as_f64().unwrap() - 20.0).abs() > 1.0, "{one:?}");
    // And iteration can be forced on a document that has it off.
    let off = load("circular_off.xlsx");
    let spec = Spec {
        iteration: Some(Iteration::default()),
        ..Spec::default()
    };
    assert!((loop_values(&off, spec)[0].as_f64().unwrap() - 20.0).abs() < 1e-2);
}

#[test]
fn search_settings_come_from_ods_and_not_from_xlsx() {
    let ods = load("probe.ods");
    let s = ods.settings.search.unwrap();
    assert_eq!(
        (s.mode, s.case_sensitive, s.whole_cell),
        (SearchMode::Wildcards, false, true)
    );
    assert!(load("semantics.xlsx").settings.search.is_none());
}

#[test]
fn document_language_is_read_but_only_used_on_request() {
    let mut m = load("probe.ods");
    assert_eq!(m.settings.locale.as_deref(), Some("en-US"));
    m.settings.locale = Some("es-CO".into());
    let mut model = xlslang::Model::new(Ns::Xlsx);
    model.settings = m.settings.clone();
    let s = model.add_sheet("S");
    model.set_formula(s, "A1", "DATEVALUE(\"3/1/2024\")");
    let eval = |spec: Spec| {
        let spec = Spec {
            outputs: vec![BindingSpec::parse("S!A1")],
            ..spec
        };
        let (p, _) = compile(&model, &spec).unwrap();
        p.execute(&Json::Null, &Overrides::default()).unwrap()["S!A1"].clone()
    };
    // Default locale: March 1st. The file's language does not change that…
    assert_eq!(eval(Spec::default()), json!(45352));
    // …unless asked: es → January 3rd.
    assert_eq!(
        eval(Spec {
            locale_from_document: true,
            ..Spec::default()
        }),
        json!(45294)
    );
}

#[test]
fn two_digit_year_window_follows_the_document() {
    let mut model = xlslang::Model::new(Ns::Xlsx);
    model.settings.two_digit_year_start = Some(1950);
    let s = model.add_sheet("S");
    model.set_formula(s, "A1", "YEAR(DATEVALUE(\"3/1/49\"))");
    model.set_formula(s, "A2", "YEAR(DATEVALUE(\"3/1/50\"))");
    let spec = Spec {
        outputs: vec![BindingSpec::parse("S!A1:A2")],
        ..Spec::default()
    };
    let (p, _) = compile(&model, &spec).unwrap();
    let out = p.execute(&Json::Null, &Overrides::default()).unwrap();
    assert_eq!(out["S!A1:A2"], json!([[2049], [1950]]));
}
