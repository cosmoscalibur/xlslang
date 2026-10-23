//! xlslang: compile spreadsheets (XLSX/ODS) into standalone executables.
//!
//! ```text
//! file ──loader──▶ Model ──compile──▶ Program (IR) ──bundle──▶ executable
//!                    │                    │
//!                 (format-specific)   (format-independent; run by `Engine`)
//! ```
//!
//! See `docs/DESIGN.md` for the architecture and decisions.

pub mod addr;
pub mod bundle;
pub mod cli;
pub mod coerce;
pub mod compile;
pub mod dates;
pub mod engine;
pub mod format;
pub mod formula;
pub mod functions;
pub mod graph;
pub mod ir;
#[cfg(feature = "load")]
pub mod loader;
pub mod matching;
pub mod model;
pub mod options;
pub mod run;
pub mod value;

pub use compile::{BindingSpec, Report, Spec, compile};
pub use ir::Program;
pub use model::Model;
pub use run::Overrides;
pub use value::Value;
