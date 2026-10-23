# Fidelity: where Excel and LibreOffice differ

The engine must give the answer the file's own application gives. Most
formulas behave identically; the rest is modeled explicitly, per **dialect**,
instead of being averaged out. The dialect comes from the file extension
(`.xlsx`/`.xlsm` → `xlsx`, `.ods` → `ods`; `core` = Excel semantics for
hand-built models) and can be forced with `--dialect`. It is stored in the
program and, like the date system, cannot be overridden on a folded program.

How a difference is modeled:

| Mechanism | Used when |
| --------- | --------- |
| `EngineOptions.dialect` consulted by an **operator** or shared code path | the difference is in an operator or in coercion (`^`, `<`, `&`, text conversion) |
| **Function namespace** override (`ods::ISNUMBER` next to `core::ISNUMBER`) | one function behaves differently as a whole |
| **Date system** (`1900`, `1904`, `1899`) | calendars and serial numbers |

Evidence: `scripts/probe_dialect.py` writes ~120 probe formulas;
LibreOffice 24.2 recalculates them into `tests/assets/probe.ods`;
`tests/probe.rs` requires the `ods` dialect to reproduce every one (except the
listed gaps) and pins the Excel-side answers of the divergent cases. Excel
answers come from Excel's documented behavior, not from an Excel run, so they
are the part to re-verify against real Excel files when available.

## Differences that are modeled

| # | Case | Excel | LibreOffice | Why |
| - | ---- | ----- | ----------- | --- |
| 1 | `0^0` | `#NUM!` | `1` | Different definition. |
| 2 | `(-8)^(1/3)` (negative base, exponent `1/n`, `n` odd) | `#NUM!` | `-2` (real root) | LibreOffice special-cases odd roots. |
| 3 | `TRUE&""`, `TEXT(TRUE,"0")` | `"TRUE"` | `"1"` | LibreOffice has no boolean type: `TRUE` is the number 1 with a boolean format. |
| 4 | `"a"<TRUE`, `TRUE=1`, `FALSE<1` | `TRUE`, `FALSE`, `FALSE` (booleans sort above text and never equal numbers) | `FALSE`, `TRUE`, `TRUE` | Same cause: booleans are numbers. |
| 5 | `ISNUMBER(TRUE)` | `FALSE` | `TRUE` | Same cause (`ods::ISNUMBER`). |
| 6 | `SUM("1")` (text typed directly as an argument) | `1` | `#VALUE!` | Excel coerces direct text arguments; LibreOffice does not (applies to every function that aggregates numbers). Text found in references is ignored by both. |
| 7 | Number → text, `1E-10`, `1E+20` | `1E-10`, `1E+20` | `0.0000000001`, `1E+020` | LibreOffice's general format stays positional down to 1e-14 and writes three exponent digits. |
| 8 | Serial numbers before 1900-03-01 | Serial 1 is 1900-01-01; serial 60 is the non-existent 1900-02-29 | Serial 0 is 1899-12-30 on the real calendar: `DATE(1900,1,1)` is 2, `YEAR(0)` is 1899 | Excel inherited a Lotus 1-2-3 leap-year bug; ODS did not. Both agree from 1900-03-01 on. Date system `1899` (`Real1899`) is the ODS default. |
| 9 | Negative serials / `DATE(1850,1,1)` | `DATE` adds 1900 to years 0–1899 (→ 3750); negative serial is `#NUM!` | Real year 1850, serial −18260 | `Real1899` allows serials down to year 0. |
| 10 | `DATE(29,1,1)`, two-digit years | 1929 (1900 + year) | 2029 (0–29 → 20xx, 30–99 → 19xx) | LibreOffice's two-digit-year window. |
| 11 | `1904` documents | `date1904` flag in the workbook | `table:null-date` = 1904-01-01 | Same calendar, different carrier; both map to date system `1904`. |
| 12 | `TEXT(x, "")` | `""` | `""` | Identical, listed because the engine used to return the number. |
| 13 | 14 | Text that looks like a date: `DATEVALUE("1 Mar 2024")`, `DATEVALUE("2024/03/01")` | accepted | `#VALUE!` | LibreOffice 24.2 (en-US) accepts `1-Mar-2024`, `Mar 1, 2024`, `3/1/2024`, ISO; it rejects day-month-name with spaces and year-first with slashes (`DatePatterns::Libre`). |
| 15 
## Document settings (stored in the file)

Read by `sheetrs` as stored (`Workbook::settings`, `None` = not stated) and
resolved into engine options by the loader:

| Setting | XLSX | ODS | Engine |
| ------- | ---- | --- | ------ |
| Iterative calculation | `calcPr iterate/iterateCount/iterateDelta` | `table:iteration status/steps/maximum-difference` | `iteration`, applied when the document switches it on; `--iterate`/`--no-iterate` override. |
| Search criteria: wildcards / regular expressions / literal | — (Excel: wildcards) | `table:use-wildcards`, `table:use-regular-expressions` | `search.mode`. ODF defaults for what the file omits: regular expressions **on**, wildcards off (LibreOffice omits an attribute equal to the default; verified by saving a document with regex on). Wildcards win when both are on. |
| Whole-cell matching | — (Excel: whole cell) | `table:search-criteria-must-apply-to-whole-cell` (default true) | `search.whole_cell`. |
| Case sensitivity | — (Excel: insensitive) | `table:case-sensitive` (default true) | `search.case_sensitive`, **comparison operators only**: with it on, `"a"="A"` is false and `"abc"<"ABD"` is true (letters first, case as tie-break, lowercase first). `COUNTIF`, `MATCH`, `VLOOKUP`, `SEARCH` stay case-insensitive: LibreOffice returns 3 for `COUNTIF(r,"APPLE")` over `apple` cells with the setting on. |
| Two-digit-year window | — | `table:null-year` | `Locale::two_digit_year_start` (default 1930): text dates and LibreOffice `DATE(yy,…)`. |
| Precision as shown | `fullPrecision="0"` | `table:precision-as-shown` | Not applied; the loader adds a diagnostic. |
| Document language | — | default cell style `fo:language`/`fo:country` | Read, **not** used by default (see below). |

Regular-expression behavior reproduced from LibreOffice: criteria are
anchored when whole-cell matching is on; an **empty match is not a hit**
(`COUNTIF(r,"a*")` counts 6 of 8 text cells, those containing an `a`, not 8;
`SEARCH("a*","xabc")` is 2). The expressions use Rust's `regex` syntax, close to
ICU's for common patterns; look-around and back-references are not supported.

Excel files carry none of the search settings, so XLSX uses Excel's behavior
(wildcards, whole cell, case-insensitive).

### Locale and text conversion

Which spelling of a text date or number is valid depends on the user's regional
settings, **not** on the file: LibreOffice, given a document whose default cell
style says `es-ES`, still reads `"3/1/2024"` as March 1st on an en-US system
(verified: recalculating a probe document with its language switched to
`es-ES`). So the engine defaults to `en` and does not take the document's
language; `--locale es|de|fr|en` selects the locale explicitly and
`--locale document` uses the document's language when the author's regional
settings are known to match it.

A locale fixes the date order of numeric text dates (`3/1/2024` is March 1st
under `en`, January 3rd under `es`/`de`/`fr`; a four-digit first component is
always year-first), month names (the locale's and English), decimal and group
separators, boolean names and `TEXT` day/month names. Times (`13:30`,
`1:30 PM`) and date-times parse in every locale.

## Cached values read from the files (reader fidelity)

Fidelity also depends on reading what the file stores, not on a convention:

* **ODS dates and times.** An ODS date cell stores an ISO date; the engine needs
  the number the spreadsheet uses, which is the number of days since the
  document's `table:null-date`. `sheetrs` used to count from 1899-12-31 (one less
  than the spreadsheet's number from 1900-03-01 on) and read `office:time-value`
  as display text. That is fixed upstream in the paired `sheetrs` branch; the
  reader does no further adjustment, and the engine applies the date-system rules
  of the table above to the numbers.
* **Error codes.** LibreOffice stores `Err:5xx` for a family of errors and
  exports several of them as `#VALUE!` to XLSX (`SQRT(-1)`, `LN(0)`, `FACT(171)`
  all become `Err:502`/`#VALUE!`; Excel says `#NUM!`). The engine always
  computes Excel's kind; when comparing with LibreOffice caches only "is an
  error" is checked for those cases.
* **Document settings.** `sheetrs` now exposes the calculation settings above (`DocumentSettings`).
* **Typed cached strings.** XLSX formula results typed `str` that look numeric
  (`"005"`) are read by `sheetrs` as numbers, and a cached `#VALUE!` of a formula
  that contains `:` is read as no value (`sheetrs` issue #62). The oracle avoids
  those shapes; they only affect *cached* values, which the engine does not use.

## Known differences not modeled yet

| Case | Status |
| ---- | ------ |
| `CHAR(160)` and other code-page characters | LibreOffice yields U+FFFD for the 128–255 range; the engine returns the Unicode character. |
| Regular expressions beyond Rust `regex` syntax (look-around, back-references) | LibreOffice uses ICU; such patterns match nothing in the engine. |
| Ordering of booleans in `MATCH`/sorted lookups | Excel ordering in every dialect. |
| Text dates in locales other than the four built in, other date spellings per locale (`d.M.yy` variants, localized month abbreviations with dots) | Not covered; use ISO or a supported pattern. |
| Excel's own `DATEDIF(…,"MD")` result | Pinned to the LibreOffice-verified formula; not checked against Excel. |
| Excel-only dynamic arrays (`FILTER`, `SORT`, spill) and `LET`/`LAMBDA` | Not implemented. |
| `table:precision-as-shown` / `fullPrecision="0"` | Not applied (diagnostic emitted). |

Adding a case: put the formula in `scripts/probe_dialect.py`, regenerate,
decide whether it is modeled (add the behavior, pin the Excel side in
`excel_dialect_differs_where_documented`) or a gap (add it to `NOT_REPRODUCED`
with the reason, and to the table above). Settings scenarios (regex,
case-sensitive, literal, iteration) are produced by rewriting the ODS
calculation settings and dropping the cached values so LibreOffice recalculates
(`scripts/probe_dialect.py`).
