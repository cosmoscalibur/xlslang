//! Frontend for XLSX/ODS files, built on the `sheetrs` readers.
//!
//! `sheetrs` already resolves the container formats (shared strings, shared
//! formulas, ODS table repetition) and normalizes ODS references and argument
//! separators to the Excel syntax, so the only format knowledge left here is
//! the dialect used to resolve function names.

use crate::addr::CellRef;
use crate::ir::Ns;
use crate::model::{Model, ModelCell};
use crate::options::DateSystem;
use crate::value::{ErrorKind, Value};
use sheetrs::reader::{CellValue, Workbook};
use std::path::Path;

pub fn load(path: &Path) -> Result<Model, String> {
    let wb =
        sheetrs::reader::read_workbook(path).map_err(|e| format!("{}: {e:#}", path.display()))?;
    let ods = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ods"));
    Ok(from_workbook(&wb, if ods { Ns::Ods } else { Ns::Xlsx }))
}

pub fn from_workbook(wb: &Workbook, dialect: Ns) -> Model {
    let mut m = Model::new(dialect);
    m.date_system = match (wb.date1904, dialect) {
        (true, _) => DateSystem::Excel1904,
        // OpenDocument's default null date is 1899-12-30 on the real calendar.
        (false, Ns::Ods) => DateSystem::Real1899,
        (false, _) => DateSystem::Excel1900,
    };
    m.names = wb
        .defined_names
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                if dialect == Ns::Ods {
                    ods_name(v)
                } else {
                    v.clone()
                },
            )
        })
        .collect();
    for (i, sheet) in wb.sheets.iter().enumerate() {
        m.sheets.push(sheet.name.clone());
        for cell in sheet.cells.values() {
            let value = convert(&cell.value);
            let formula = cell.formula.as_deref().map(str::to_string);
            if formula.is_none() && value == Value::Empty {
                continue;
            }
            m.cells.insert(
                CellRef::new(i as u16, cell.row, cell.col),
                ModelCell {
                    value,
                    formula,
                    array: cell.is_array,
                },
            );
        }
    }
    m
}

fn convert(v: &CellValue) -> Value {
    match v {
        CellValue::Empty => Value::Empty,
        CellValue::Number(n) => Value::Number(*n),
        CellValue::Text(t) => Value::Text(t.clone()),
        CellValue::Boolean(b) => Value::Bool(*b),
        CellValue::Error(e) => Value::Error(error_kind(&e.to_string())),
    }
}

/// Excel error literal or LibreOffice `Err:NNN` code.
fn error_kind(s: &str) -> ErrorKind {
    if let Some(k) = ErrorKind::parse(s) {
        return k;
    }
    match s.strip_prefix("Err:").and_then(|c| c.parse::<u16>().ok()) {
        Some(522 | 523) => ErrorKind::Circ,
        Some(524) => ErrorKind::Ref,
        Some(525 | 508 | 509 | 510 | 511) => ErrorKind::Name,
        Some(502 | 503) => ErrorKind::Num,
        Some(532) => ErrorKind::DivZero,
        _ => ErrorKind::Value,
    }
}

/// ODS stores a defined range as `Sheet7.C3:Sheet7.C10` or
/// `$'My Sheet'.$A$1:.$B$2`; rewrite it as `Sheet7!C3:C10`. Anything that is
/// not a plain reference is returned unchanged.
fn ods_name(def: &str) -> String {
    let mut sheet: Option<String> = None;
    let mut cells = Vec::new();
    for part in def.split(':') {
        let part = part.strip_prefix('$').unwrap_or(part);
        let (name, cell) = if let Some(rest) = part.strip_prefix('\'') {
            match rest.split_once("'.") {
                Some((n, c)) => (Some(n.replace("''", "'")), c),
                None => return def.to_string(),
            }
        } else {
            match part.rsplit_once('.') {
                Some((n, c)) if !n.is_empty() => (Some(n.to_string()), c),
                Some((_, c)) => (None, c),
                None => return def.to_string(),
            }
        };
        if crate::addr::parse_area(cell).is_none() {
            return def.to_string();
        }
        sheet = sheet.or(name);
        cells.push(cell);
    }
    let area = cells.join(":");
    match sheet {
        Some(n) if n.chars().all(|c| c.is_alphanumeric() || c == '_') => format!("{n}!{area}"),
        Some(n) => format!("'{}'!{area}", n.replace('\'', "''")),
        None => area,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ods_defined_ranges() {
        assert_eq!(ods_name("Sheet7.C3:Sheet7.C10"), "Sheet7!C3:C10");
        assert_eq!(ods_name("$'My Sheet'.$A$1:.$B$2"), "'My Sheet'!$A$1:$B$2");
        assert_eq!(ods_name("$Sheet1.$A$1"), "Sheet1!$A$1");
        assert_eq!(ods_name("SUM(1;2)"), "SUM(1;2)");
    }

    #[test]
    fn libreoffice_error_codes() {
        assert_eq!(error_kind("Err:522"), ErrorKind::Circ);
        assert_eq!(error_kind("#N/A"), ErrorKind::Na);
        assert_eq!(error_kind("Err:999"), ErrorKind::Value);
    }
}
