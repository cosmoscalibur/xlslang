# xlslang

Compile an XLSX or ODS spreadsheet into a standalone executable (or a library
call) that takes JSON in and returns JSON out.

```text
loan.xlsx ──┐                                ┌─ ./loan   (single file, ~0.9 MB, ~2 ms/run)
            ├─▶ IR ──▶ fold · prune · order ─┼─ program.json  (the IR, portable)
loan.ods  ──┘  (format independent)          └─ Program::execute() (library)
```

* **One IR, one engine.** Both formats are lowered to the same intermediate
  representation; there is no second execution engine to keep in sync.
* **Inputs and outputs are declared.** Cells that do not depend on an input are
  evaluated at build time and disappear; cells that do not feed an output are
  dropped. What remains is the minimal computation.
* **Hybrid compile/run-time.** Static references are resolved and ordered at
  build time (topological order, cycles isolated). `INDIRECT`, `OFFSET` and
  similar functions resolve their targets lazily at run time, on the same
  engine.
* **Execution is configurable:** 1900/1904 date system, iterative calculation
  (iterations and tolerance) and locale (separators, month/day names, boolean
  names).

Architecture, decisions and benchmarks: [docs/DESIGN.md](docs/DESIGN.md).

## Quick start

Requires Rust ≥ 1.99 (`rust-toolchain.toml` pins it).

```bash
cargo build --release          # target/release/{xlslang, xlslang-rt}

# Compile a model: choose what goes in and what comes out.
xlslang build tests/assets/loan.xlsx \
    --in principal=Loan!B1 --in rate=Loan!B2 --in years=Loan!B3 \
    --out payment=Loan!B4 --out interest=Loan!B6 \
    -o loan
# cells: 1464 total, 4 reachable, 4 kept | formulas: 1086 total, 0 folded, 1 kept ...

echo '{"principal": 250000, "rate": 0.045, "years": 15}' | ./loan
# {"payment":1912.4832220336577,"interest":94246.9799660584}

./loan --info                  # declared inputs/outputs, shapes, diagnostics
./loan --serve                 # one JSON object per line in, one per line out
```

The same works from `.ods`, and without producing a file:

```bash
xlslang run tests/assets/loan.ods --in principal=Loan!B1 --out payment=Loan!B4 <<< '{"principal": 1000}'
xlslang calc '=TEXT(1234.5,"#,##0.00")' --locale es          # "1.234,50"
xlslang inspect model.xlsx      # sizes, unsupported functions, cycles, run-time refs
```

### Commands

| Command | Purpose |
| ------- | ------- |
| `build FILE [-o OUT]` | Compile to an executable. `--emit-ir PATH` also writes the IR as JSON. |
| `run FILE\|PROGRAM.json` | Compile in memory (or load an IR file) and run once. |
| `calc "=FORMULA"` | Evaluate a single formula. |
| `inspect FILE` | Report what the compiler sees. |

### Binding inputs and outputs (`--in`, `--out`)

`[NAME=]TARGET`, where `TARGET` is `Sheet1!A1`, `'My Sheet'!B2:C9`, an
unqualified address (first sheet) or a defined name. `NAME` is the JSON key
(default: the target text). Without any `--out`, every sheet's formula range is
an output.

JSON mapping: a single cell is a scalar, a range is an array of rows (a flat
array is accepted for one-row/one-column ranges). Numbers, strings, booleans and
`null` (blank) map to themselves; an error is `{"error": "#DIV/0!"}` (also
accepted on input). Dates are spreadsheet serial numbers. An input left out of
the JSON keeps the value stored in the file; unknown keys are rejected.

### Execution options

| Option | Build (`build`/`run`) | Run (executable) |
| ------ | --------------------- | ---------------- |
| `--date-system 1900\|1904` | default: from the file | only with `--no-fold` builds |
| `--iterate[=N[,EPS]]` / `--no-iterate` | default: off (`#CIRC!`) | only with `--no-fold` builds |
| `--locale en\|es\|de\|fr` | default: `en` | only with `--no-fold` builds |
| `--now SERIAL`, `--seed N` | fixes `NOW()/TODAY()` and `RAND()` | always |
| `--no-fold` | keep every reachable formula | — |

Build-time folding consumes the date system, iteration settings and locale, so
overriding them on a folded program is refused with an explanation instead of
silently mixing configurations.

## Function support

About 130 functions, grouped in namespaces (`core::`, `ods::`, `xlsx::`):
aggregates and conditional aggregates (`SUM`…`SUMIFS`, `COUNTIFS`, `MAXIFS`,
`SUMPRODUCT`), math and rounding (spreadsheet-exact `ROUND`), logic and special
forms (`IF`, `IFS`, `IFERROR`, `SWITCH`, `CHOOSE`, lazy), text (`TEXT` with
number/date format codes, `TEXTJOIN`, `SEARCH` with wildcards…), lookup
(`VLOOKUP`, `HLOOKUP`, `XLOOKUP`, `INDEX`, `MATCH`, `OFFSET`, `INDIRECT`),
dates (`DATE`, `EDATE`, `EOMONTH`, `DATEDIF`, `WEEKDAY`…) and finance (`PMT`,
`FV`, `PV`, `NPV`, `IRR`). Dialect-only functions live in their namespace
(`ods::EASTERSUNDAY`). Unsupported functions evaluate to `#NAME?` and are listed
by `build`/`inspect`.

Correctness is checked against LibreOffice: `tests/oracle.rs` recomputes ~200
formulas from `tests/assets/semantics.{xlsx,ods}` and compares them with the
values LibreOffice cached (see `scripts/gen_semantics_fixture.py`).

## Limitations

See [docs/DESIGN.md](docs/DESIGN.md#known-limitations) for the full list. The
ones that matter first: no external-workbook references (cached values are
kept), no dynamic-array spilling (array results take their top-left value),
no `LET`/`LAMBDA`, run-time `INDIRECT`/`OFFSET` keep the whole workbook
reachable, and ODS date cells carry the serial the `sheetrs` reader derives.

## Roadmap

1. **Calculation engine** (this release): IR, parser, graph ordering, folding,
   pruning, iteration, locale, JSON I/O, executables.
2. **Data validation**: read validations from the files and compile them into
   input checks (list, range, whole/decimal, custom formula) that run before
   evaluation and report which input broke which rule.
3. **Spreadsheet as source**: embed programs in Rust crates
   (`include_bytes!` + typed accessors generated from the declared bindings),
   native code generation for the static part, WASM runtime, language bindings,
   and an HTTP wrapper around `--serve`.

Details of each step are in the design document.

## Development

```bash
cargo test                      # unit, integration, LibreOffice oracle, CLI
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run --release --example stress        # scale check
cargo run --release --example graph_bench   # petgraph/ultragraph comparison
```

`sheetrs` (the XLSX/ODS readers) is used as a git dependency. Cargo features:
`load` (default) enables the file frontend and the `xlslang` binary; without it
the crate is the engine only (`xlslang-rt` is the runtime stub embedded into
compiled executables).
