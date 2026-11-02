//! `po-missing` — extract and merge missing translations in PO files.
//!
//! The crate is usable as a library (this API) and as a CLI binary.
//!
//! # Library example
//!
//! ```no_run
//! use po_missing::{Options, run};
//!
//! let options = Options {
//!     base_path: "frontend/src/locales".into(),
//!     ..Options::default()
//! };
//! let summary = run(&options).expect("run failed");
//! for result in &summary.results {
//!     println!("{}: {:?}", result.locale, result.outcome);
//! }
//! ```
//!
//! # How it works
//!
//! For every locale directory under `base_path` the tool looks for
//! `<filename>.po` and `<filename>-missing.po`:
//!
//! 1. **Merge back** — completed translations found in the missing file are
//!    written into the main file (fuzzy entries are only merged when
//!    [`Options::include_fuzzy`] is set; the fuzzy flag is cleared on merge).
//! 2. **Extract** — entries that still need work (blank `msgstr`, incomplete
//!    plural forms, fuzzy) are written to a fresh missing file. When nothing
//!    is missing, a stale missing file is removed.
//!
//! Obsolete (`#~`) entries and the file header are never touched; header
//! comments and metadata are carried over to generated missing files.

mod error;
mod po;
mod process;

pub use error::{Error, Result};
pub use process::{
    DEFAULT_FILENAME, LocaleOutcome, LocaleResult, LocaleStats, MISSING_SUFFIX, MissingFileAction,
    Options, Summary, discover_locales, effective_jobs, process_all, process_locale, run,
};
