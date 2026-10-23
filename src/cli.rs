//! Command-line plumbing shared by the compiler binary and the runtime stub.

use crate::ir::Program;
use crate::options::{DateSystem, Iteration, Locale};
use crate::run::Overrides;
use std::io::{BufRead, Read, Write};

/// Minimal argument bag: flags may be given as `--k v` or `--k=v`.
pub struct Args(pub Vec<String>);

impl Args {
    pub fn flag(&mut self, name: &str) -> bool {
        match self.0.iter().position(|a| a == name) {
            Some(i) => {
                self.0.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        let eq = format!("{name}=");
        if let Some(i) = self.0.iter().position(|a| a.starts_with(&eq)) {
            return Ok(Some(self.0.remove(i)[eq.len()..].to_string()));
        }
        match self.0.iter().position(|a| a == name) {
            Some(i) if i + 1 < self.0.len() => {
                self.0.remove(i);
                Ok(Some(self.0.remove(i)))
            }
            Some(_) => Err(format!("{name} needs a value")),
            None => Ok(None),
        }
    }

    pub fn values(&mut self, name: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        while let Some(v) = self.value(name)? {
            out.push(v);
        }
        Ok(out)
    }
}

pub fn parse_date_system(s: &str) -> Result<DateSystem, String> {
    match s {
        "1900" => Ok(DateSystem::Excel1900),
        "1904" => Ok(DateSystem::Excel1904),
        _ => Err(format!("--date-system must be 1900 or 1904, got `{s}`")),
    }
}

/// `--iterate` (defaults) or `--iterate=N[,EPS]`.
pub fn parse_iteration(args: &mut Args) -> Result<Option<Iteration>, String> {
    if args.flag("--iterate") {
        return Ok(Some(Iteration::default()));
    }
    let Some(v) = args.value("--iterate")? else {
        return Ok(None);
    };
    let (n, eps) = v.split_once(',').unwrap_or((&v, ""));
    let max_iterations = n
        .parse()
        .map_err(|_| format!("bad --iterate count `{n}`"))?;
    let max_change = if eps.is_empty() {
        Iteration::default().max_change
    } else {
        eps.parse()
            .map_err(|_| format!("bad --iterate epsilon `{eps}`"))?
    };
    let it = Iteration {
        max_iterations,
        max_change,
    };
    Ok(Some(it))
}

pub fn parse_locale(s: &str) -> Result<Locale, String> {
    Locale::by_tag(s).ok_or_else(|| format!("unknown locale `{s}` (built in: en, es, de, fr)"))
}

/// Parse the options shared by build time and run time.
pub fn parse_overrides(args: &mut Args) -> Result<Overrides, String> {
    let mut o = Overrides::default();
    if let Some(v) = args.value("--date-system")? {
        o.date_system = Some(parse_date_system(&v)?);
    }
    if args.flag("--no-iterate") {
        o.iteration = Some(None);
    } else if let Some(it) = parse_iteration(args)? {
        o.iteration = Some(Some(it));
    }
    if let Some(v) = args.value("--locale")? {
        o.locale = Some(parse_locale(&v)?);
    }
    if let Some(v) = args.value("--now")? {
        o.now = Some(
            v.parse()
                .map_err(|_| format!("bad --now `{v}` (serial number)"))?,
        );
    }
    if let Some(v) = args.value("--seed")? {
        o.rand_seed = Some(v.parse().map_err(|_| format!("bad --seed `{v}`"))?);
    }
    Ok(o)
}

fn read_input(path: Option<&str>) -> Result<serde_json::Value, String> {
    let mut text = String::new();
    match path {
        Some("-") | None => {
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
        }
        Some(p) => text = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
    }
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("invalid JSON input: {e}"))
}

pub const RUNTIME_HELP: &str = "\
Options:
  --input FILE|-       JSON object with the inputs (default: stdin)
  --serve              read one JSON object per line, answer one per line
  --pretty             indent the JSON output
  --info               describe the inputs and outputs of this program
  --date-system 1900|1904   --iterate[=N[,EPS]] | --no-iterate
  --locale en|es|de|fr      --now SERIAL   --seed N";

/// Execute a program according to command-line arguments. Returns the exit
/// code: 0 on success, 1 on a failed run, 2 on bad arguments.
pub fn run_program(program: &Program, mut args: Args) -> i32 {
    let pretty = args.flag("--pretty");
    let serve = args.flag("--serve");
    let info = args.flag("--info");
    let input = match args.value("--input") {
        Ok(v) => v,
        Err(e) => return usage_error(&e),
    };
    let overrides = match parse_overrides(&mut args) {
        Ok(o) => o,
        Err(e) => return usage_error(&e),
    };
    if let Some(extra) = args.0.first() {
        return usage_error(&format!("unexpected argument `{extra}`\n{RUNTIME_HELP}"));
    }
    if info {
        println!("{}", describe(program));
        return 0;
    }
    let show = |v: &serde_json::Value| {
        if pretty {
            serde_json::to_string_pretty(v)
        } else {
            serde_json::to_string(v)
        }
        .unwrap_or_default()
    };
    if serve {
        let stdin = std::io::stdin();
        let mut out = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let reply = serde_json::from_str::<serde_json::Value>(&line)
                .map_err(|e| format!("invalid JSON input: {e}"))
                .and_then(|j| program.execute(&j, &overrides).map_err(|e| e.0))
                .unwrap_or_else(|e| serde_json::json!({ "error_message": e }));
            let _ = writeln!(out, "{}", serde_json::to_string(&reply).unwrap_or_default());
            let _ = out.flush();
        }
        return 0;
    }
    let result =
        read_input(input.as_deref()).and_then(|j| program.execute(&j, &overrides).map_err(|e| e.0));
    match result {
        Ok(v) => {
            println!("{}", show(&v));
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

pub fn usage_error(msg: &str) -> i32 {
    eprintln!("error: {msg}");
    2
}

fn describe(p: &Program) -> String {
    let shape = |b: &crate::ir::Binding| {
        serde_json::json!({
            "name": b.name,
            "range": format!("{}!{}", p.sheets.get(b.rect.sheet as usize).map_or("?", String::as_str), crate::addr::rect_text(&b.rect)),
            "shape": [b.rect.rows(), b.rect.cols()],
        })
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "inputs": p.inputs.iter().map(shape).collect::<Vec<_>>(),
        "outputs": p.outputs.iter().map(shape).collect::<Vec<_>>(),
        "cells": p.cells.len(),
        "cycles": p.cycles.len(),
        "diagnostics": p.diagnostics,
    }))
    .unwrap_or_default()
}

/// Run `f` on a thread with a large stack: demand-driven evaluation of long
/// dependency chains that the schedule did not order is recursive.
pub fn with_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(f)
        .map_or_else(
            |_| std::process::exit(1),
            |h| h.join().unwrap_or_else(|_| std::process::exit(101)),
        )
}
