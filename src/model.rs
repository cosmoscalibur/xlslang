//! In-memory, format-independent workbook: what every frontend produces and
//! the compiler consumes. Formulas stay as text until compilation, once all
//! sheet and defined names are known.

use crate::addr::{self, CellRef};
use crate::ir::Ns;
use crate::options::{DateSystem, Iteration, Search};
use crate::value::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCell {
    /// Constant value, or the cached result of the formula.
    pub value: Value,
    /// Formula text without the leading `=`, in Excel syntax.
    pub formula: Option<String>,
    /// Array (Ctrl+Shift+Enter) formula: its operators work element-wise.
    pub array: bool,
}

/// Calculation settings stored in the document, resolved into engine terms.
/// `None`: not stated, the engine default applies.
#[derive(Debug, Clone, Default)]
pub struct ModelSettings {
    /// Iterative calculation, when the document switches it on.
    pub iteration: Option<Iteration>,
    pub search: Option<Search>,
    pub two_digit_year_start: Option<u16>,
    /// Document language tag (`es-CO`).
    pub locale: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub sheets: Vec<String>,
    pub cells: HashMap<CellRef, ModelCell>,
    /// Defined names: name → formula text (usually a reference).
    pub names: HashMap<String, String>,
    pub date_system: DateSystem,
    /// Namespace tried first when resolving function names.
    pub dialect: Ns,
    pub settings: ModelSettings,
    pub diagnostics: Vec<String>,
}

impl Model {
    pub fn new(dialect: Ns) -> Self {
        Self {
            sheets: Vec::new(),
            cells: HashMap::new(),
            names: HashMap::new(),
            // OpenDocument numbers dates from 1899-12-30 on the real calendar.
            date_system: if dialect == Ns::Ods {
                DateSystem::Real1899
            } else {
                DateSystem::Excel1900
            },
            dialect,
            settings: ModelSettings::default(),
            diagnostics: Vec::new(),
        }
    }

    pub fn add_sheet(&mut self, name: &str) -> u16 {
        self.sheets.push(name.to_string());
        (self.sheets.len() - 1) as u16
    }

    fn at(&self, sheet: u16, a1: &str) -> CellRef {
        let area = addr::parse_area(a1).unwrap_or_else(|| panic!("bad address `{a1}`"));
        CellRef::new(sheet, area.r0, area.c0)
    }

    /// Set a constant, addressing the cell in A1 notation.
    pub fn set(&mut self, sheet: u16, a1: &str, value: Value) -> &mut Self {
        let at = self.at(sheet, a1);
        self.cells.insert(
            at,
            ModelCell {
                value,
                formula: None,
                array: false,
            },
        );
        self
    }

    pub fn set_formula(&mut self, sheet: u16, a1: &str, formula: &str) -> &mut Self {
        let at = self.at(sheet, a1);
        let formula = formula.strip_prefix('=').unwrap_or(formula).to_string();
        self.cells.insert(
            at,
            ModelCell {
                value: Value::Empty,
                formula: Some(formula),
                array: false,
            },
        );
        self
    }

    pub fn sheet_id(&self, name: &str) -> Option<u16> {
        self.sheets
            .iter()
            .position(|s| s.eq_ignore_ascii_case(name))
            .map(|i| i as u16)
    }
}
