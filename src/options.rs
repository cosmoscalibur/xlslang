//! Execution parameters of the engine. Date system, iteration, search settings
//! and dialect are defined by the document; the locale is the user's.

use crate::dates;
use crate::ir::Ns;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateSystem {
    /// Serial 1 is 1900-01-01 (with the historical 1900-02-29 quirk).
    #[default]
    Excel1900,
    /// Serial 0 is 1904-01-01.
    Excel1904,
    /// Serial 0 is 1899-12-30 on the real calendar: no 1900-02-29, negative
    /// serials allowed. The default null date of OpenDocument/LibreOffice.
    Real1899,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Iteration {
    pub max_iterations: u32,
    pub max_change: f64,
}

impl Default for Iteration {
    fn default() -> Self {
        // Excel defaults.
        Self {
            max_iterations: 100,
            max_change: 0.001,
        }
    }
}

/// Order of the numeric components of a text date (`3/1/2024`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
}

/// Which text spellings are accepted as dates. LibreOffice is stricter than
/// Excel: it rejects `1 Mar 2024` and `2024/03/01` (observed on 24.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DatePatterns {
    #[default]
    Excel,
    Libre,
}

/// How text criteria and patterns are interpreted by `COUNTIF`, `MATCH`,
/// `VLOOKUP`, `SEARCH`, … (an ODS document setting; Excel is always
/// `Wildcards`, whole cell, case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    /// `*`, `?` and `~` escapes.
    Wildcards,
    /// Regular expressions.
    Regex,
    /// The text as written.
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    pub mode: SearchMode,
    /// Criteria must match the whole cell, not a part of it.
    pub whole_cell: bool,
    /// Text comparison distinguishes case (comparison operators, criteria and
    /// lookups).
    pub case_sensitive: bool,
}

impl Search {
    pub const EXCEL: Self = Self {
        mode: SearchMode::Wildcards,
        whole_cell: true,
        case_sensitive: false,
    };
}

impl Default for Search {
    fn default() -> Self {
        Self::EXCEL
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    pub tag: String,
    pub date_order: DateOrder,
    #[serde(default)]
    pub date_patterns: DatePatterns,
    /// First year of the 100-year window for two-digit years in text dates and
    /// (LibreOffice) `DATE`: 1930 makes 30–99 → 19xx and 00–29 → 20xx.
    pub two_digit_year_start: u16,
    pub decimal: char,
    pub group: char,
    pub true_text: String,
    pub false_text: String,
    pub currency: String,
    pub months: [String; 12],
    /// Sunday first.
    pub days: [String; 7],
}

fn strs<const N: usize>(a: [&str; N]) -> [String; N] {
    a.map(String::from)
}

impl Locale {
    pub fn en_us() -> Self {
        Self {
            tag: "en-US".into(),
            date_order: DateOrder::Mdy,
            date_patterns: DatePatterns::Excel,
            two_digit_year_start: 1930,
            decimal: '.',
            group: ',',
            true_text: "TRUE".into(),
            false_text: "FALSE".into(),
            currency: "$".into(),
            months: strs([
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ]),
            days: strs([
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ]),
        }
    }

    pub fn es_es() -> Self {
        Self {
            tag: "es-ES".into(),
            date_order: DateOrder::Dmy,
            date_patterns: DatePatterns::Excel,
            two_digit_year_start: 1930,
            decimal: ',',
            group: '.',
            true_text: "VERDADERO".into(),
            false_text: "FALSO".into(),
            currency: "€".into(),
            months: strs([
                "enero",
                "febrero",
                "marzo",
                "abril",
                "mayo",
                "junio",
                "julio",
                "agosto",
                "septiembre",
                "octubre",
                "noviembre",
                "diciembre",
            ]),
            days: strs([
                "domingo",
                "lunes",
                "martes",
                "miércoles",
                "jueves",
                "viernes",
                "sábado",
            ]),
        }
    }

    /// Built-in locale by BCP-47-like tag (`en`, `es`). Only languages whose
    /// conventions are verified against a real application are supported.
    /// (`es`, `es-ES`, `es_ES` …).
    pub fn by_tag(tag: &str) -> Option<Self> {
        let norm = tag.replace('_', "-").to_ascii_lowercase();
        match norm.split('-').next()? {
            "en" => Some(Self::en_us()),
            "es" => Some(Self::es_es()),
            _ => None,
        }
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::en_us()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineOptions {
    pub date_system: DateSystem,
    /// `None`: circular references evaluate to `#CIRC!`.
    pub iteration: Option<Iteration>,
    pub locale: Locale,
    /// Behavioral family: `Ods` follows LibreOffice where it differs from Excel
    /// (booleans are numbers, `0^0` is 1, …); `Xlsx` and `Core` follow Excel.
    /// See docs/FIDELITY.md. Chosen from the file extension by the compiler.
    pub dialect: Ns,
    /// Text criteria and pattern settings.
    pub search: Search,
    /// Fixed value (serial number) for `NOW()`/`TODAY()`; the system clock
    /// when `None`.
    pub now: Option<f64>,
    /// Seed of the `RAND` generator; derived from the clock when `None`.
    pub rand_seed: Option<u64>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            date_system: DateSystem::default(),
            iteration: None,
            locale: Locale::default(),
            dialect: Ns::Core,
            search: Search::EXCEL,
            now: None,
            rand_seed: None,
        }
    }
}

impl EngineOptions {
    /// Whether booleans behave as the numbers 1/0 (LibreOffice).
    pub fn bool_is_number(&self) -> bool {
        self.dialect == Ns::Ods
    }

    /// Current date-time as a serial number in the configured date system.
    pub fn now_serial(&self) -> f64 {
        self.now.unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64());
            dates::unix_to_serial(secs, self.date_system)
        })
    }
}
