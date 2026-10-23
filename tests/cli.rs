//! End-to-end: compile real XLSX/ODS files into executables and run them.

use serde_json::{Value as Json, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const XLSLANG: &str = env!("CARGO_BIN_EXE_xlslang");
const RUNTIME: &str = env!("CARGO_BIN_EXE_xlslang-rt");

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets")
        .join(name)
}

fn run(cmd: &Path, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn build_loan(file: &str, dir: &Path, extra: &[&str]) -> PathBuf {
    let exe = dir.join(format!("loan-{file}"));
    let mut args = vec![
        "build",
        asset(file).to_str().unwrap().to_owned().leak(),
        "--in",
        "principal=Loan!B1",
        "--in",
        "rate=Loan!B2",
        "--in",
        "years=Loan!B3",
        "--in",
        "month=Loan!B7",
        "--out",
        "payment=Loan!B4",
        "--out",
        "interest=Loan!B6",
        "--out",
        "balance=Loan!B8",
        "--runtime",
        RUNTIME,
        "-o",
        exe.to_str().unwrap().to_owned().leak(),
    ];
    args.extend_from_slice(extra);
    let (code, _, err) = run(Path::new(XLSLANG), &args, "");
    assert_eq!(code, 0, "build failed: {err}");
    exe
}

fn pmt(p: f64, annual: f64, years: f64) -> f64 {
    let r = annual / 12.0;
    let n = years * 12.0;
    p * r / (1.0 - (1.0 + r).powf(-n))
}

fn check_loan(exe: &Path) {
    let (code, out, err) = run(
        exe,
        &[],
        r#"{"principal": 250000, "rate": 0.045, "years": 15, "month": 24}"#,
    );
    assert_eq!(code, 0, "{err}");
    let v: Json = serde_json::from_str(&out).unwrap();
    let want = pmt(250_000.0, 0.045, 15.0);
    assert!((v["payment"].as_f64().unwrap() - want).abs() < 1e-6, "{v}");
    assert!((v["interest"].as_f64().unwrap() - (want * 180.0 - 250_000.0)).abs() < 1e-4);
    // Balance after 24 payments (dynamic INDIRECT in the source workbook).
    let r: f64 = 0.045 / 12.0;
    let bal = 250_000.0 * (1.0 + r).powi(24) - want * (((1.0 + r).powi(24) - 1.0) / r);
    assert!((v["balance"].as_f64().unwrap() - bal).abs() < 1e-4, "{v}");

    // Defaults come from the spreadsheet when an input is omitted.
    let (code, out, _) = run(exe, &[], "{}");
    assert_eq!(code, 0);
    let v: Json = serde_json::from_str(&out).unwrap();
    assert!((v["payment"].as_f64().unwrap() - pmt(200_000.0, 0.05, 30.0)).abs() < 1e-6);
}

#[test]
fn xlsx_and_ods_compile_to_equivalent_executables() {
    let dir = tempfile::tempdir().unwrap();
    for f in ["loan.xlsx", "loan.ods"] {
        let exe = build_loan(f, dir.path(), &[]);
        check_loan(&exe);
    }
}

#[test]
fn executable_is_self_describing_and_strict() {
    let dir = tempfile::tempdir().unwrap();
    let exe = build_loan("loan.xlsx", dir.path(), &[]);
    let (code, out, _) = run(&exe, &["--info"], "");
    assert_eq!(code, 0);
    let info: Json = serde_json::from_str(&out).unwrap();
    assert_eq!(info["inputs"].as_array().unwrap().len(), 4);
    assert_eq!(info["outputs"][0]["name"], "payment");

    let (code, _, err) = run(&exe, &[], r#"{"bogus": 1}"#);
    assert_eq!(code, 1);
    assert!(err.contains("unknown input `bogus`"), "{err}");
    let (code, _, err) = run(&exe, &["--wat"], "{}");
    assert_eq!(code, 2, "{err}");
    let (code, _, err) = run(&exe, &[], "{not json");
    assert_eq!(code, 1);
    assert!(err.contains("invalid JSON"), "{err}");
}

#[test]
fn serve_mode_answers_line_by_line() {
    let dir = tempfile::tempdir().unwrap();
    let exe = build_loan("loan.xlsx", dir.path(), &[]);
    let input = "{\"principal\": 100000}\n\n{\"nope\": 1}\n{\"principal\": 200000}\n";
    let (code, out, _) = run(&exe, &["--serve"], input);
    assert_eq!(code, 0);
    let lines: Vec<Json> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[1]["error_message"]
            .as_str()
            .unwrap()
            .contains("unknown input")
    );
    let (a, b) = (
        lines[0]["payment"].as_f64().unwrap(),
        lines[2]["payment"].as_f64().unwrap(),
    );
    assert!((b / a - 2.0).abs() < 1e-9);
}

#[test]
fn run_time_options_on_a_no_fold_build() {
    let dir = tempfile::tempdir().unwrap();
    // Without --no-fold the date system is fixed at build time and refused.
    let folded = build_loan("loan.xlsx", dir.path(), &[]);
    let (code, _, err) = run(&folded, &["--date-system", "1904"], "{}");
    assert_eq!(code, 1);
    assert!(err.contains("--no-fold"), "{err}");

    let open = build_loan("loan.ods", dir.path(), &["--no-fold"]);
    let (code, _, err) = run(&open, &["--date-system", "1904", "--locale", "es"], "{}");
    assert_eq!(code, 0, "{err}");
}

#[test]
fn run_command_compiles_in_memory() {
    let (code, out, err) = run(
        Path::new(XLSLANG),
        &[
            "run",
            asset("loan.xlsx").to_str().unwrap(),
            "--in",
            "principal=Loan!B1",
            "--out",
            "payment=Loan!B4",
        ],
        r#"{"principal": 1000}"#,
    );
    assert_eq!(code, 0, "{err}");
    let v: Json = serde_json::from_str(&out).unwrap();
    assert!((v["payment"].as_f64().unwrap() - pmt(1000.0, 0.05, 30.0)).abs() < 1e-9);
}

#[test]
fn calc_command() {
    let (code, out, _) = run(Path::new(XLSLANG), &["calc", "=SUM(1,2)*3&\"x\""], "");
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "\"9x\"");
    let (_, out, _) = run(
        Path::new(XLSLANG),
        &["calc", "=TEXT(1234.5,\"#,##0.00\")", "--locale", "es"],
        "",
    );
    assert_eq!(out.trim(), "\"1.234,50\"");
    let (_, out, _) = run(Path::new(XLSLANG), &["calc", "=1/0"], "");
    assert_eq!(out.trim(), "{\"error\":\"#DIV/0!\"}");
}

#[test]
fn bundle_roundtrip_json_matches() {
    let dir = tempfile::tempdir().unwrap();
    let ir = dir.path().join("loan.json");
    let (code, _, err) = run(
        Path::new(XLSLANG),
        &[
            "build",
            asset("loan.xlsx").to_str().unwrap(),
            "--in",
            "principal=Loan!B1",
            "--out",
            "payment=Loan!B4",
            "--runtime",
            RUNTIME,
            "--emit-ir",
            ir.to_str().unwrap(),
            "-o",
            dir.path().join("m").to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    // The emitted IR runs through `run` without the original file.
    let (code, out, err) = run(
        Path::new(XLSLANG),
        &["run", ir.to_str().unwrap()],
        r#"{"principal": 1000}"#,
    );
    assert_eq!(code, 0, "{err}");
    assert!(serde_json::from_str::<Json>(&out).unwrap()["payment"].is_number());
    let _ = json!(null);
}
