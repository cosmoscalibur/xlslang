# XLSLANG

Transpiler for Excel formula like language to Python. Support script and module
mode (formulas are functions).

## Roadmap

- [x] Grammar definition.
- [ ] Parser from string.
- [ ] Support code from file.
- [ ] Run like script (interpreter and useful like basic template for
  transpiler)
  - [ ] Calculator 🖩 mode (one line and basic operations).
  - [ ] Advance calculator 🖩 mode (one line and supported functions that not
    require references).
  - [ ] Multiline and Variable assignation support
  - [ ] external variable definition
  - [ ] Support range references.
- [ ] Begin unit tests.
- [ ] Transpiler Python 🐍.
- [ ] Transpiler JS.
- [ ] Interactive interpreter ❓.
- [ ] Transpiler Rust 🦀 ❓.
- [ ] Crate package
- [ ] PyPI Package.

Public announce in Twitter (spanish):
https://twitter.com/cosmoscalibur/status/1462102290555359237

### XLSCheck

Binary tool to detect invalid formulas in Excel files (error cells). Run as

```bash
cargo run --bin xlschecks file.xlsx
```

- [x] Main binary to detect invalid formulas.
- [ ] Support lib mode.
- [ ] Crate.
- [ ] Trace error mode:
  - [ ] Graph mode.
  - [ ] Filter only parent errors.
  - [ ] Inspect formula and args of parent errors.

## Related Projects and documentation

- [Excel to code](https://github.com/tamc/excel_to_code).
- [Spreadsheet technology](https://studwww.itu.dk/~sestoft/corecalc/spreadsheet-20120115.pdf)
- [Corecalc and Funcalc](https://studwww.itu.dk/~sestoft/funcalc/)

### Compilers

- [Cyclang](https://github.com/lyledean1/cyclang)
- [Create Your Own Programming Language with Rust](https://createlang.rs/)
