//! Scale check: `cargo run --release --example stress`.
use serde_json::json;
use std::time::Instant;
use xlslang::ir::Ns;
use xlslang::{BindingSpec, Model, Overrides, Spec, Value, compile};

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("  {label:<28} {:>9.1?}", t.elapsed());
    r
}

fn run(title: &str, m: &Model, inputs: &[&str], outputs: &[&str], input: serde_json::Value) {
    println!("{title}");
    let spec = Spec {
        inputs: inputs.iter().map(|s| BindingSpec::parse(s)).collect(),
        outputs: outputs.iter().map(|s| BindingSpec::parse(s)).collect(),
        ..Spec::default()
    };
    let (p, r) = time("compile", || compile(m, &spec).unwrap());
    println!(
        "  kept {} cells, {} formulas (of {} reachable)",
        r.cells_kept, r.formulas_kept, r.cells_reachable
    );
    let out = time("run", || p.execute(&input, &Overrides::default()).unwrap());
    println!(
        "  -> {}",
        out.to_string().chars().take(100).collect::<String>()
    );
}

fn main() {
    let n = 100_000u32;
    // A: a long dependency chain from one input.
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("S");
    m.set(s, "A1", Value::Number(1.0));
    for i in 2..=n {
        m.set_formula(s, &format!("A{i}"), &format!("A{}+1", i - 1));
    }
    run(
        "chain of 100k formulas",
        &m,
        &["x=A1"],
        &[&format!("y=A{n}")],
        json!({"x": 5}),
    );

    // B: running totals anchored at the top (quadratic range reads).
    let k = 5_000u32;
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("S");
    for i in 1..=k {
        m.set(s, &format!("A{i}"), Value::Number(f64::from(i)));
        m.set_formula(s, &format!("B{i}"), &format!("SUM($A$1:A{i})"));
    }
    run(
        "running SUM($A$1:An), 5k rows",
        &m,
        &["a=A1"],
        &[&format!("t=B1:B{k}")],
        json!({"a": 1}),
    );

    // C: many lookups into one table.
    let t = 10_000u32;
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("S");
    for i in 1..=t {
        m.set(s, &format!("A{i}"), Value::Number(f64::from(i)));
        m.set(s, &format!("B{i}"), Value::Number(f64::from(i) * 2.0));
    }
    m.set(s, "D1", Value::Number(7.0));
    for i in 1..=20_000u32 {
        m.set_formula(
            s,
            &format!("E{i}"),
            &format!("VLOOKUP(D$1+{i},A$1:B${t},2,FALSE)"),
        );
    }
    m.set_formula(s, "F1", "SUM(E1:E20000)");
    run(
        "20k VLOOKUPs over a 10k table",
        &m,
        &["d=D1"],
        &["f=F1"],
        json!({"d": 3}),
    );

    // D: wide independent formulas, all folded at build time.
    let mut m = Model::new(Ns::Xlsx);
    let s = m.add_sheet("S");
    for i in 1..=50_000u32 {
        m.set(s, &format!("A{i}"), Value::Number(f64::from(i)));
        m.set_formula(s, &format!("B{i}"), &format!("A{i}*2+ROUND(A{i}/3,2)"));
    }
    m.set_formula(s, "C1", "SUM(B1:B50000)");
    run("50k folded formulas", &m, &[], &["c=C1"], json!(null));
}
