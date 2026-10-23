mod common;
use common::*;
use serde_json::{Value as Json, json};

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(calc("1+2*3"), json!(7));
    assert_eq!(calc("-2^2"), json!(4));
    assert_eq!(calc("2^3^2"), json!(64)); // left associative, like spreadsheets
    assert_eq!(calc("10/4"), json!(2.5));
    assert_eq!(calc("50%+1"), json!(1.5));
    assert_eq!(calc("1/0"), err("#DIV/0!"));
    assert_eq!(calc("0^0"), err("#NUM!"));
    assert_eq!(calc("\"a\"+1"), err("#VALUE!"));
    assert_eq!(calc("\"3\"+1"), json!(4));
    assert_eq!(calc("TRUE+1"), json!(2));
}

#[test]
fn text_and_comparison() {
    assert_eq!(calc("\"a\"&1+1"), json!("a2"));
    assert_eq!(calc("\"abc\"=\"ABC\""), json!(true));
    assert_eq!(calc("1<\"a\""), json!(true)); // numbers sort before text
    assert_eq!(calc("\"a\"<TRUE"), json!(true));
    assert_eq!(calc("LEFT(\"hello\",2)&RIGHT(\"hello\",3)"), json!("hello"));
    assert_eq!(calc("SUBSTITUTE(\"a-b-c\",\"-\",\"+\",2)"), json!("a-b+c"));
    assert_eq!(calc("TRIM(\"  a   b \")"), json!("a b"));
    assert_eq!(calc("PROPER(\"hELLO wORLD\")"), json!("Hello World"));
    assert_eq!(calc("TEXT(0.256,\"0.0%\")"), json!("25.6%"));
    assert_eq!(calc("TEXT(1234.5,\"#,##0.00\")"), json!("1,234.50"));
    assert_eq!(calc("FIND(\"b\",\"abc\")"), json!(2));
    assert_eq!(calc("SEARCH(\"B*\",\"abc\")"), json!(2));
    assert_eq!(calc("TEXTJOIN(\",\",TRUE,\"a\",\"\",\"b\")"), json!("a,b"));
}

#[test]
fn lazy_forms() {
    assert_eq!(calc("IF(TRUE,1,1/0)"), json!(1));
    assert_eq!(calc("IF(FALSE,1)"), json!(false));
    assert_eq!(calc("IFERROR(1/0,\"x\")"), json!("x"));
    assert_eq!(calc("IFERROR(5,1/0)"), json!(5));
    assert_eq!(calc("IFNA(NA(),7)"), json!(7));
    assert_eq!(calc("IFNA(1/0,7)"), err("#DIV/0!"));
    assert_eq!(calc("CHOOSE(2,10,20,1/0)"), json!(20));
    assert_eq!(calc("IFS(1>2,\"a\",2>1,\"b\")"), json!("b"));
    assert_eq!(calc("SWITCH(2,1,\"a\",2,\"b\",\"z\")"), json!("b"));
    assert_eq!(calc("AND(TRUE,1,0)"), json!(false));
    assert_eq!(calc("OR(FALSE,0,2)"), json!(true));
}

#[test]
fn math_functions() {
    assert_eq!(calc("ROUND(1.005,2)"), json!(1.01));
    assert_eq!(calc("ROUND(-2.5,0)"), json!(-3));
    assert_eq!(calc("ROUNDUP(1.21,1)"), json!(1.3));
    assert_eq!(calc("MOD(-1,3)"), json!(2));
    assert_eq!(calc("INT(-1.5)"), json!(-2));
    assert_eq!(calc("SQRT(-1)"), err("#NUM!"));
    assert_eq!(calc("CEILING(2.1,0.5)"), json!(2.5));
    assert_eq!(calc("SUMPRODUCT({1,2,3},{4,5,6})"), json!(32));
    assert_eq!(calc("SUM({1,2;3,4})"), json!(10));
    assert_eq!(calc("MEDIAN(1,3,2,10)"), json!(2.5));
    assert_eq!(calc("LARGE({5,1,9},2)"), json!(5));
    assert_eq!(calc("PMT(0.01,12,-1000)"), json!(88.84878867834166));
}

#[test]
fn ranges_and_aggregates() {
    let m = sheet(&[
        ("A1", json!(1)),
        ("A2", json!(2)),
        ("A3", json!("x")),
        ("A4", json!(4)),
        ("B1", json!("a")),
        ("B2", json!("b")),
        ("B3", json!("a")),
        ("B4", json!("a")),
        ("C1", json!("=SUM(A1:A4)")),
        ("C2", json!("=AVERAGE(A1:A4)")),
        ("C3", json!("=COUNT(A1:A4)")),
        ("C4", json!("=COUNTA(A1:A4)")),
        ("C5", json!("=SUMIF(B1:B4,\"a\",A1:A4)")),
        ("C6", json!("=COUNTIF(A1:A4,\">1\")")),
        ("C7", json!("=SUMIFS(A1:A4,B1:B4,\"a\",A1:A4,\">=1\")")),
        ("C8", json!("=MAX(A:A)")),
        ("C9", json!("=SUMPRODUCT((B1:B4=\"a\")*(A1:A4>0))")),
        ("C10", json!("=SUM(A1:A3)+\"2\"")),
        ("C11", json!("=COUNTIF(B1:B4,\"?\")")),
        ("C12", json!("=COUNTBLANK(A1:A6)")),
    ]);
    let (p, _) = build(&m, &spec(&[], &["C1:C12"]));
    let out = p.execute(&Json::Null, &Default::default()).unwrap();
    let col: Vec<Json> = out["Sheet1!C1:C12"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].clone())
        .collect();
    assert_eq!(
        col,
        vec![
            json!(7),
            json!(7.0 / 3.0),
            json!(3),
            json!(4),
            json!(5), // 1 + 4 (A3 is text, ignored by SUMIF on numbers; B3="a")
            json!(2),
            json!(5),
            json!(4),
            json!(3),
            json!(5),
            json!(4),
            json!(2),
        ]
    );
}

#[test]
fn lookups() {
    let m = sheet(&[
        ("A1", json!("k1")),
        ("B1", json!(10)),
        ("A2", json!("k2")),
        ("B2", json!(20)),
        ("A3", json!("k3")),
        ("B3", json!(30)),
        ("D1", json!("=VLOOKUP(\"k2\",A1:B3,2,FALSE)")),
        ("D2", json!("=INDEX(B1:B3,MATCH(\"k3\",A1:A3,0))")),
        ("D3", json!("=XLOOKUP(\"k1\",A1:A3,B1:B3)")),
        ("D4", json!("=VLOOKUP(\"zz\",A1:B3,2,FALSE)")),
        ("D5", json!("=IFERROR(VLOOKUP(\"zz\",A1:B3,2,FALSE),0)")),
        ("D6", json!("=MATCH(25,B1:B3,1)")),
        ("D7", json!("=SUM(INDEX(B1:B3,0))")),
        ("D8", json!("=ROWS(A1:B3)*COLUMNS(A1:B3)")),
    ]);
    let (p, _) = build(&m, &spec(&[], &["D1:D8"]));
    let out = p.execute(&Json::Null, &Default::default()).unwrap();
    let col: Vec<Json> = out["Sheet1!D1:D8"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].clone())
        .collect();
    assert_eq!(
        col,
        vec![
            json!(20),
            json!(30),
            json!(10),
            err("#N/A"),
            json!(0),
            json!(2),
            json!(60),
            json!(6)
        ]
    );
}

#[test]
fn dates_and_date_systems() {
    assert_eq!(calc("DATE(2024,1,31)"), json!(45322));
    assert_eq!(
        calc("YEAR(45292)&\"-\"&MONTH(45292)&\"-\"&DAY(45292)"),
        json!("2024-1-1")
    );
    assert_eq!(calc("EDATE(DATE(2024,1,31),1)"), json!(45351)); // 2024-02-29
    assert_eq!(calc("EOMONTH(DATE(2024,1,15),0)"), json!(45322));
    assert_eq!(calc("WEEKDAY(DATE(2024,1,1))"), json!(2));
    assert_eq!(
        calc("DATEDIF(DATE(2020,1,15),DATE(2024,1,14),\"Y\")"),
        json!(3)
    );
    assert_eq!(calc("DAYS(DATE(2024,3,1),DATE(2024,2,1))"), json!(29));
    assert_eq!(
        calc("TEXT(DATE(2024,12,25),\"dd/mm/yyyy\")"),
        json!("25/12/2024")
    );
    assert_eq!(calc("DATE(1900,3,1)-DATE(1900,2,28)"), json!(2)); // the 1900-02-29 quirk

    let mut s = spec(&[], &["A1"]);
    s.date_system = Some(xlslang::options::DateSystem::Excel1904);
    let mut m = xlslang::Model::new(xlslang::ir::Ns::Xlsx);
    let sh = m.add_sheet("Sheet1");
    m.set_formula(sh, "A1", "DATE(2024,1,1)");
    let (p, _) = build(&m, &s);
    assert_eq!(
        p.execute(&Json::Null, &Default::default()).unwrap()["Sheet1!A1"],
        json!(43830)
    );
}

#[test]
fn namespaces_split_dialect_specific_functions() {
    use xlslang::ir::Ns;
    assert_eq!(calc_in(Ns::Ods, "EASTERSUNDAY(2024)"), json!(45382));
    assert_eq!(
        calc_in(Ns::Ods, "COM.MICROSOFT.CONCAT(\"a\",\"b\")"),
        json!("ab")
    );
    assert_eq!(calc_in(Ns::Xlsx, "_xlfn.CONCAT(\"a\",\"b\")"), json!("ab"));
    // Excel has no EASTERSUNDAY.
    assert_eq!(calc_in(Ns::Xlsx, "EASTERSUNDAY(2024)"), err("#NAME?"));
    // The IR keeps the namespace it resolved to.
    let mut m = xlslang::Model::new(Ns::Ods);
    let s = m.add_sheet("S");
    m.set_formula(s, "A1", "EASTERSUNDAY(2024)+SUM(1)");
    m.set_formula(s, "B1", "A1");
    let (p, _) = build(
        &m,
        &xlslang::Spec {
            fold: false,
            ..spec(&[], &["B1"])
        },
    );
    let json = serde_json::to_string(&p).unwrap();
    assert!(
        json.contains("ods::EASTERSUNDAY") && json.contains("core::SUM"),
        "{json}"
    );
}

#[test]
fn unsupported_functions_are_reported() {
    let m = sheet(&[("A1", json!("=NOSUCHFN(1)+1"))]);
    let (p, r) = build(&m, &spec(&[], &["A1"]));
    assert!(r.unsupported.contains("NOSUCHFN"));
    assert_eq!(
        p.execute(&Json::Null, &Default::default()).unwrap()["Sheet1!A1"],
        err("#NAME?")
    );
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.contains("Sheet1!A1") && d.contains("NOSUCHFN"))
    );
}
