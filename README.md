# xlslang

Compile an XLSX or ODS spreadsheet into a standalone executable (or a library
call) that takes JSON in and returns JSON out.

```text
loan.xlsx ──┐                                ┌─ ./loan   (single file, ~4 MB, ~2 ms/run)
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
* **Faithful to the source application.** Behavior that differs between Excel
  and LibreOffice (booleans as numbers, `0^0`, 1900 leap-year quirk, number
  formatting…) follows the file's dialect; every case is listed and tested in
  [docs/FIDELITY.md](docs/FIDELITY.md).
* **Execution is configurable:** 1899/1900/1904 date system, iterative calculation
  (iterations and tolerance) and locale (separators, month/day names, boolean
  names).

Architecture and decisions: [docs/DESIGN.md](docs/DESIGN.md). Differences between
Excel and LibreOffice: [docs/FIDELITY.md](docs/FIDELITY.md).

## Quick start

Requires Rust ≥ 1.99 (`rust-toolchain.toml` pins it).

```bash
cargo build --release          # target/release/{xlslang, xlslang-rt}

# Compile a model: declare which ranges are inputs and which are outputs.
xlslang build tests/assets/loan.xlsx \
    --in Loan!B1 --in Loan!B2 --in Loan!B3 \
    --out Loan!B4 --out Loan!B6 \
    -o loan
# cells: 1464 total, 4 reachable, 4 kept | formulas: 1086 total, 0 folded, 1 kept ...

echo '{"Loan!B1": 250000, "Loan!B2": 0.045, "Loan!B3": 15}' | ./loan
# {"Loan!B4":1912.4832220336577,"Loan!B6":94246.9799660584}

./loan --info                  # declared inputs/outputs, shapes, diagnostics
./loan --serve                 # one JSON object per line in, one per line out
```

The same works from `.ods`, and without producing a file:

```bash
xlslang run tests/assets/loan.ods --in Loan!B1 --out Loan!B4 <<< '{"Loan!B1": 1000}'
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

### Inputs and outputs (`--in`, `--out`)

Each is a list of cells and ranges: `Sheet1!A1`, `'My Sheet'!B2:C9`, an
unqualified address (first sheet) or a defined name (resolved to its range).
That is all the engine knows: JSON keys are the **canonical range text**
(`Sheet1!B2:C9`, sheet quoted when needed). Friendly names, schemas and
documentation are a layer above the engine. Without any `--out`, every sheet's
formula range is an output.

JSON mapping: a single cell is a scalar, a range is an array of rows (a flat
array is accepted for one-row/one-column ranges). Numbers, strings, booleans and
`null` (blank) map to themselves; an error is `{"error": "#DIV/0!"}` (also
accepted on input). Dates are spreadsheet serial numbers of the file's date
system. An input left out of the JSON keeps the value stored in the file;
unknown keys are rejected.

### Execution options

**The document decides** date system (`date1904` / `table:null-date`),
iterative calculation (`calcPr` / `table:iteration`), search settings (regular
expressions, wildcards, whole cell, case sensitivity: ODS) and dialect (the
file extension). There is no command-line flag for them in `build`, `run` or
compiled executables, and a flag given anyway is an error: the engine is
faithful to what the file says. (`calc`, which has no file, accepts
`--date-system 1899|1900|1904`, `--dialect` and `--iterate[=N[,EPS]]`; so does
the library for models built in memory.)

**The command line decides what a file cannot say:**

| Option | Meaning |
| ------ | ------- |
| `--locale en\|es` | The user's regional settings: decimal/group separators, date order of text dates, month and day names, boolean names. Default `en`. The document's *language* is informational and does not change evaluation (LibreOffice evaluates an `es-ES` document with the system locale). |
| `--now SERIAL` | Fix `NOW()`/`TODAY()`. Default: the system clock at each run. For reproducible CI. |
| `--seed N` | Fix the `RAND`/`RANDBETWEEN` generator. Default: seeded from the clock. For reproducible CI. |
| `--no-fold` | Keep every reachable formula; evaluate nothing at build time. |
| `--runtime-locale` | Leave the locale open: the executable accepts `--locale`. Formulas that may depend on it (text, comparisons, text functions, anything reading a text cell) stay live; the rest still fold. Without it the locale is fixed at build time and a run-time `--locale` is an error. |

`--no-fold` and `--runtime-locale` are independent: all formulas kept with a
static locale, folded formulas with an open locale, or any other combination.

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
no `LET`/`LAMBDA`, and run-time `INDIRECT`/`OFFSET` keep the whole workbook
reachable.

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
```

`sheetrs` (the XLSX/ODS readers) is used as a git dependency (currently its
`claude/xlsx-ods-compiler-d07ic5` branch, which carries the ODS date fix; switch
to `develop` once merged). Cargo features:
`load` (default) enables the file frontend and the `xlslang` binary; without it
the crate is the engine only (`xlslang-rt` is the runtime stub embedded into
compiled executables).
