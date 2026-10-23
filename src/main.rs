//! `xlslang`: build, run and inspect spreadsheet programs.

use std::path::{Path, PathBuf};
use xlslang::cli::{self, Args, parse_overrides};
use xlslang::{BindingSpec, Model, Program, Spec, bundle, compile, loader};

const HELP: &str = "\
xlslang — compile XLSX/ODS spreadsheets into executables

USAGE
  xlslang build FILE [-o OUT] [--in [NAME=]RANGE]... [--out [NAME=]RANGE]... [options]
      Compile FILE into a standalone executable. RANGE is `Sheet1!A1`,
      `Sheet1!B2:C9` or a defined name. Without --out, every formula range is
      an output. `--emit-ir PATH` also writes the program as JSON.
  xlslang run FILE|PROGRAM.json [--in ...] [--out ...] [runtime options]
      Compile in memory and run immediately (JSON on stdin/--input).
  xlslang calc \"=FORMULA\" [runtime options]
      Evaluate one formula.
  xlslang inspect FILE
      Show what the compiler sees: sizes, unsupported functions, cycles.

BUILD OPTIONS
  --no-fold            keep every reachable formula (no build-time evaluation)
  --runtime-locale     leave the locale open: the executable accepts --locale.
                       Formulas that may depend on it stay live; the rest still
                       fold. Without it the locale is fixed at build time.
  --runtime PATH       runtime stub to embed into (default: xlslang-rt next to
                       this binary, else this binary)
  --locale en|es   --now SERIAL   --seed N
  The date system, iteration, search settings and dialect come from the
  document and are not options. `calc` has no document and accepts
  --date-system 1899|1900|1904, --dialect core|xlsx|ods and --iterate[=N[,EPS]].

Executables accept the runtime options; run them with --help-runtime-style
flags: --input, --serve, --pretty, --info.";

fn main() {
    let args = Args(std::env::args().skip(1).collect());
    let code = cli::with_big_stack(move || real_main(args));
    std::process::exit(code);
}

fn real_main(mut args: Args) -> i32 {
    if let Some(program) = bundle::read_embedded() {
        return cli::run_program(&program, args);
    }
    if args.0.is_empty() || args.flag("--help") || args.flag("-h") {
        println!("{HELP}");
        return 0;
    }
    let cmd = args.0.remove(0);
    let result = match cmd.as_str() {
        "build" => build(args),
        "run" => run(args),
        "calc" => calc(args),
        "inspect" => inspect(args),
        other => Err(Fail::Usage(format!("unknown command `{other}`\n\n{HELP}"))),
    };
    match result {
        Ok(code) => code,
        Err(Fail::Usage(m)) => cli::usage_error(&m),
        Err(Fail::Run(m)) => {
            eprintln!("error: {m}");
            1
        }
    }
}

enum Fail {
    Usage(String),
    Run(String),
}

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Self::Usage(s)
    }
}

fn load_model(path: &str) -> Result<Model, Fail> {
    loader::load(Path::new(path)).map_err(Fail::Run)
}

fn spec_from(args: &mut Args) -> Result<(Spec, xlslang::Overrides), Fail> {
    let inputs = args
        .values("--in")?
        .iter()
        .map(|s| BindingSpec::parse(s))
        .collect();
    let outputs = args
        .values("--out")?
        .iter()
        .map(|s| BindingSpec::parse(s))
        .collect();
    let fold = !args.flag("--no-fold");
    let locale_runtime = args.flag("--runtime-locale");
    let o = parse_overrides(args)?;
    let spec = Spec {
        inputs,
        outputs,
        locale: o.locale.clone(),
        now: o.now,
        rand_seed: o.rand_seed,
        fold,
        locale_runtime,
        ..Spec::default()
    };
    Ok((spec, o))
}

fn print_report(r: &xlslang::Report, p: &Program) {
    eprintln!(
        "cells: {} total, {} reachable, {} kept | formulas: {} total, {} folded at build time, {} kept | cycles: {}",
        r.cells_total,
        r.cells_reachable,
        r.cells_kept,
        r.formulas_total,
        r.formulas_folded,
        r.formulas_kept,
        r.cycles
    );
    if r.dynamic_refs {
        eprintln!("note: run-time references (INDIRECT/OFFSET) keep the whole workbook reachable");
    }
    if !r.unsupported.is_empty() {
        eprintln!(
            "warning: unsupported functions (evaluate to #NAME?): {}",
            r.unsupported.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    for d in p.diagnostics.iter().take(10) {
        eprintln!("diagnostic: {d}");
    }
    if p.diagnostics.len() > 10 {
        eprintln!(
            "... {} more diagnostics (see --emit-ir)",
            p.diagnostics.len() - 10
        );
    }
}

fn build(mut args: Args) -> Result<i32, Fail> {
    let out = args.value("-o")?;
    let emit = args.value("--emit-ir")?;
    let runtime = args.value("--runtime")?;
    let (spec, _) = spec_from(&mut args)?;
    let [file] = args.0.as_slice() else {
        return Err(Fail::Usage("build needs exactly one input file".into()));
    };
    let model = load_model(file)?;
    let (program, report) = compile(&model, &spec).map_err(|e| Fail::Run(e.0))?;
    print_report(&report, &program);
    if let Some(p) = emit {
        std::fs::write(&p, serde_json::to_vec_pretty(&program).unwrap_or_default())
            .map_err(|e| Fail::Run(format!("{p}: {e}")))?;
    }
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model");
    let out = PathBuf::from(out.unwrap_or_else(|| stem.to_string()));
    let runtime = match runtime {
        Some(r) => PathBuf::from(r),
        None => {
            let me = std::env::current_exe().map_err(|e| Fail::Run(e.to_string()))?;
            let rt = me.with_file_name(format!("xlslang-rt{}", std::env::consts::EXE_SUFFIX));
            if rt.exists() { rt } else { me }
        }
    };
    bundle::write_executable(&program, &runtime, &out).map_err(Fail::Run)?;
    eprintln!("wrote {}", out.display());
    Ok(0)
}

fn run(mut args: Args) -> Result<i32, Fail> {
    // Run-time flags go to `run_program` untouched; the rest configures the
    // in-memory build.
    let mut forwarded: Vec<String> = Vec::new();
    for flag in ["--serve", "--pretty", "--info"] {
        if args.flag(flag) {
            forwarded.push(flag.into());
        }
    }
    if let Some(v) = args.value("--input")? {
        forwarded.extend(["--input".into(), v]);
    }
    let (spec, _) = spec_from(&mut args)?;
    let [file] = args.0.as_slice() else {
        return Err(Fail::Usage("run needs exactly one input file".into()));
    };
    let program: Program = if file.ends_with(".json") {
        serde_json::from_slice(&std::fs::read(file).map_err(|e| Fail::Run(format!("{file}: {e}")))?)
            .map_err(|e| Fail::Run(format!("{file}: {e}")))?
    } else {
        let model = load_model(file)?;
        let (p, report) = compile(&model, &spec).map_err(|e| Fail::Run(e.0))?;
        print_report(&report, &p);
        p
    };
    Ok(cli::run_program(&program, Args(forwarded)))
}

fn calc(mut args: Args) -> Result<i32, Fail> {
    let flags = cli::parse_model_flags(&mut args)?;
    let o = parse_overrides(&mut args)?;
    let [formula] = args.0.as_slice() else {
        return Err(Fail::Usage("calc needs exactly one formula".into()));
    };
    let mut m = Model::new(xlslang::ir::Ns::Core);
    let s = m.add_sheet("Sheet1");
    m.set_formula(s, "A1", formula);
    let spec = Spec {
        outputs: vec![BindingSpec::parse("Sheet1!A1")],
        date_system: flags.date_system,
        dialect: flags.dialect,
        iteration: flags.iteration,
        locale: o.locale.clone(),
        now: o.now,
        rand_seed: o.rand_seed,
        ..Spec::default()
    };
    let (program, report) = compile(&m, &spec).map_err(|e| Fail::Run(e.0))?;
    for d in &program.diagnostics {
        eprintln!("diagnostic: {d}");
    }
    let _ = report;
    let out = program
        .execute(&serde_json::Value::Null, &xlslang::Overrides::default())
        .map_err(|e| Fail::Run(e.0))?;
    println!("{}", out["Sheet1!A1"]);
    Ok(0)
}

fn inspect(args: Args) -> Result<i32, Fail> {
    let [file] = args.0.as_slice() else {
        return Err(Fail::Usage("inspect needs exactly one input file".into()));
    };
    let model = load_model(file)?;
    let spec = Spec {
        fold: false,
        ..Spec::default()
    };
    let (program, report) = compile(&model, &spec).map_err(|e| Fail::Run(e.0))?;
    println!("sheets: {}", model.sheets.join(", "));
    println!("date system: {:?}", model.date_system);
    println!(
        "cells: {} ({} formulas)",
        report.cells_total, report.formulas_total
    );
    println!("circular groups: {}", report.cycles);
    println!(
        "run-time references (INDIRECT/OFFSET): {}",
        if report.dynamic_refs { "yes" } else { "no" }
    );
    if report.unsupported.is_empty() {
        println!("unsupported functions: none");
    } else {
        println!(
            "unsupported functions: {}",
            report
                .unsupported
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for d in program.diagnostics.iter().take(20) {
        println!("diagnostic: {d}");
    }
    Ok(0)
}
