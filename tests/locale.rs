//! Locale-dependent behavior: alphabet order, text dates, number text.

mod common;

use serde_json::{Value as Json, json};
use xlslang::ir::Ns;
use xlslang::options::Locale;
use xlslang::{BindingSpec, Model, Overrides, Spec, compile};

fn eval(locale: Locale, formula: &str) -> Json {
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("S");
    m.set_formula(s, "A1", formula);
    let spec = Spec {
        outputs: vec![BindingSpec::parse("S!A1")],
        locale: Some(locale),
        ..Spec::default()
    };
    let (p, _) = compile(&m, &spec).unwrap();
    p.execute(&Json::Null, &Overrides::default()).unwrap()["S!A1"].clone()
}

#[test]
fn spanish_alphabet_puts_enye_between_n_and_o() {
    for (f, en, es) in [
        ("\"ñu\">\"nz\"", false, true),
        ("\"ñu\"<\"oa\"", true, true),
        ("\"nz\"<\"ñ\"", false, true),
        ("\"ñ\"=\"n\"", false, false),
        ("\"Ñ\"=\"ñ\"", true, true),
        ("\"á\"<\"b\"", true, true),
    ] {
        assert_eq!(eval(Locale::en_us(), f), json!(en), "en: {f}");
        assert_eq!(eval(Locale::es_es(), f), json!(es), "es: {f}");
    }
}

#[test]
fn sorted_lookup_follows_the_alphabet() {
    // MATCH(…,1) assumes ascending data: n < ñ < o in Spanish.
    let f = "MATCH(\"ñ\",{\"n\",\"ñ\",\"o\"},1)";
    assert_eq!(eval(Locale::es_es(), f), json!(2));
}

#[test]
fn code_page_characters() {
    let en = Locale::en_us();
    assert_eq!(eval(en.clone(), "CODE(\"€\")"), json!(128));
    assert_eq!(eval(en.clone(), "CHAR(128)"), json!("€"));
    assert_eq!(eval(en.clone(), "CHAR(160)"), json!("\u{a0}"));
    assert_eq!(eval(en.clone(), "CODE(\"ñ\")"), json!(241));
    assert_eq!(eval(en, "CODE(\"漢\")"), json!(63));
}

#[test]
fn text_dates_and_numbers_by_locale() {
    assert_eq!(
        eval(Locale::en_us(), "DATEVALUE(\"3/1/2024\")"),
        json!(45352)
    );
    assert_eq!(
        eval(Locale::es_es(), "DATEVALUE(\"3/1/2024\")"),
        json!(45294)
    );
    assert_eq!(
        eval(Locale::es_es(), "DATEVALUE(\"1 de marzo de 2024\")"),
        json!(45352)
    );
    assert_eq!(eval(Locale::es_es(), "VALUE(\"1.234,5\")"), json!(1234.5));
    assert_eq!(
        eval(Locale::es_es(), "TEXT(1234.5,\"#,##0.00\")"),
        json!("1.234,50")
    );
    assert_eq!(
        eval(Locale::es_es(), "TEXT(DATE(2024,3,1),\"dddd d mmmm\")"),
        json!("viernes 1 marzo")
    );
}
