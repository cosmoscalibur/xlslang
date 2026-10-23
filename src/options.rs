//! Execution parameters of the engine: date system, iterative calculation and
//! locale. They can be fixed at build time and overridden at run time (unless
//! they affected values folded by the compiler, see `Program::folded_with`).

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    pub tag: String,
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

    pub fn de_de() -> Self {
        Self {
            tag: "de-DE".into(),
            decimal: ',',
            group: '.',
            true_text: "WAHR".into(),
            false_text: "FALSCH".into(),
            currency: "€".into(),
            months: strs([
                "Januar",
                "Februar",
                "März",
                "April",
                "Mai",
                "Juni",
                "Juli",
                "August",
                "September",
                "Oktober",
                "November",
                "Dezember",
            ]),
            days: strs([
                "Sonntag",
                "Montag",
                "Dienstag",
                "Mittwoch",
                "Donnerstag",
                "Freitag",
                "Samstag",
            ]),
        }
    }

    pub fn fr_fr() -> Self {
        Self {
            tag: "fr-FR".into(),
            decimal: ',',
            group: '\u{202f}',
            true_text: "VRAI".into(),
            false_text: "FAUX".into(),
            currency: "€".into(),
            months: strs([
                "janvier",
                "février",
                "mars",
                "avril",
                "mai",
                "juin",
                "juillet",
                "août",
                "septembre",
                "octobre",
                "novembre",
                "décembre",
            ]),
            days: strs([
                "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
            ]),
        }
    }

    /// Built-in locale by BCP-47-like tag (`es`, `es-ES`, `es_ES` …).
    pub fn by_tag(tag: &str) -> Option<Self> {
        let norm = tag.replace('_', "-").to_ascii_lowercase();
        match norm.split('-').next()? {
            "en" => Some(Self::en_us()),
            "es" => Some(Self::es_es()),
            "de" => Some(Self::de_de()),
            "fr" => Some(Self::fr_fr()),
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
