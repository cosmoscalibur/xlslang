# Changelog

## 0.2.0

Complete rewrite around a format-independent IR and a calculation engine; the
`pest` grammar and Python-transpiler plan were dropped.

### Added

- XLSX/ODS frontend on top of `sheetrs`, namespaced function registry
  (`core`/`xlsx`/`ods`), hand-written formula parser.
- Compiler: output cone pruning, SCC ordering, build-time constant folding,
  static resolution of `INDIRECT`/`OFFSET`.
- Engine: demand-driven evaluation, iterative calculation, array context,
  1900/1904 date systems, locales (`en`, `es`, `de`, `fr`).
- JSON input/output bindings, standalone executables, `--serve` mode.
- `xlslang build|run|calc|inspect`, `xlslang-rt` runtime stub.
- LibreOffice oracle tests for both formats and dialect probes (`docs/FIDELITY.md`).
- Dialects (`xlsx`/`ods`, from the file extension) for operator and coercion
  differences; date system `1899` (real calendar, ODS default).
- Document settings applied: iteration (XLSX/ODS), ODS search mode (regex/wildcards/literal), whole-cell and case sensitivity, two-digit-year window; `--search`, `--case-sensitive`, `--whole-cell`, `--locale document`.
- Text dates parsed by locale (numeric order, month names, times); `DATEDIF` `MD`/`YD`; per-character case mapping (`UPPER("ß")`).
- Inputs and outputs are plain range lists; JSON keys are canonical range text.
