//! Core engine: locale discovery, merge-back, extraction and statistics.
//!
//! The flow per locale is a straight line: merge completed translations from
//! `<filename>-missing.po` into `<filename>.po`, then (re)extract whatever is
//! still missing — all in memory, touching disk only when a file changes.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::po;

/// Base filename (without extension) processed in every locale directory.
pub const DEFAULT_FILENAME: &str = "messages";

/// Suffix appended to the base filename for the missing-translation file.
pub const MISSING_SUFFIX: &str = "-missing";

/// What happened (or would happen, under [`Options::dry_run`]) to the
/// `*-missing.po` file of a locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingFileAction {
    /// The file was left untouched.
    None,
    /// Written (or rewritten) with the entries that still need work.
    Written,
    /// Removed because nothing is missing anymore.
    Removed,
}

/// Options controlling a full run.
#[derive(Debug, Clone)]
pub struct Options {
    /// Directory containing one subdirectory per locale.
    pub base_path: PathBuf,
    /// Base filename without extension (e.g. `messages`).
    pub filename: String,
    /// Treat fuzzy entries as translated.
    pub include_fuzzy: bool,
    /// Report intended changes without writing anything.
    pub dry_run: bool,
    /// Parallel workers; `0` means "one per CPU core".
    pub jobs: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("frontend/src/locales"),
            filename: DEFAULT_FILENAME.to_owned(),
            include_fuzzy: false,
            dry_run: false,
            jobs: 0,
        }
    }
}

/// Statistics and performed actions for one processed locale.
#[derive(Debug, Clone, Serialize)]
pub struct LocaleStats {
    /// Non-obsolete entries (the translation workload).
    pub total: usize,
    /// Entries counted as translated.
    pub translated: usize,
    /// Entries that still need translation work.
    pub missing: usize,
    /// Percentage of translated entries, `100.0` for empty files.
    pub percent: f64,
    /// Translations merged back from the missing file into the main file.
    pub merged: usize,
    /// Missing-file entries whose `msgid` no longer exists in the main file.
    pub orphaned: usize,
    /// What happened to the missing file.
    pub missing_file: MissingFileAction,
}

/// Outcome of processing a single locale directory.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LocaleOutcome {
    /// The locale had a main PO file and was processed.
    Processed {
        /// Statistics and performed actions.
        #[serde(flatten)]
        stats: LocaleStats,
    },
    /// The directory has no `<filename>.po`, so it was skipped.
    Skipped,
    /// Processing failed; the message is user-facing.
    Failed {
        /// The error message.
        error: String,
    },
}

/// A locale paired with its outcome.
#[derive(Debug, Clone, Serialize)]
pub struct LocaleResult {
    /// Locale directory name.
    pub locale: String,
    /// What happened while processing it.
    #[serde(flatten)]
    pub outcome: LocaleOutcome,
}

/// Aggregated result of a run over all locales.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// One result per locale directory, in sorted directory order.
    pub results: Vec<LocaleResult>,
    /// Locales successfully processed.
    pub processed: usize,
    /// Locales skipped (no main PO file).
    pub skipped: usize,
    /// Locales that failed.
    pub failed: usize,
    /// Total translations merged back.
    pub merged: usize,
    /// Total missing translations remaining after the run.
    pub missing: usize,
}

impl Summary {
    /// Aggregate a list of per-locale results into a summary.
    #[must_use]
    pub fn new(results: Vec<LocaleResult>) -> Self {
        let mut summary = Self {
            results,
            processed: 0,
            skipped: 0,
            failed: 0,
            merged: 0,
            missing: 0,
        };
        for result in &summary.results {
            match &result.outcome {
                LocaleOutcome::Processed { stats } => {
                    summary.processed += 1;
                    summary.merged += stats.merged;
                    summary.missing += stats.missing;
                }
                LocaleOutcome::Skipped => summary.skipped += 1,
                LocaleOutcome::Failed { .. } => summary.failed += 1,
            }
        }
        summary
    }
}

/// Run the full merge + extraction pass over every locale under
/// `options.base_path`.
///
/// # Errors
///
/// Returns an error only when the base path is unusable (missing or not a
/// directory); per-locale failures are reported inside [`Summary::results`].
pub fn run(options: &Options) -> Result<Summary> {
    let dirs = discover_locales(&options.base_path)?;
    Ok(Summary::new(process_all(&dirs, options)))
}

/// Map any I/O failure while listing `base` to a library error.
fn read_dir_error(base: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |source| Error::ReadDir {
        path: base.to_path_buf(),
        source,
    }
}

/// List locale directories under `base`, skipping hidden entries and files,
/// sorted for deterministic output.
///
/// # Errors
///
/// Returns [`Error::NotADirectory`] when `base` is not a directory and
/// [`Error::ReadDir`] when the directory cannot be listed.
pub fn discover_locales(base: &Path) -> Result<Vec<PathBuf>> {
    if !base.is_dir() {
        return Err(Error::NotADirectory {
            path: base.to_path_buf(),
        });
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(base).map_err(read_dir_error(base))? {
        let path = entry.map_err(read_dir_error(base))?.path();
        let hidden = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if path.is_dir() && !hidden {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Process every locale directory, in parallel when more than one worker is
/// configured and more than one locale exists.
///
/// # Panics
///
/// Panics if a worker thread panics. The per-locale logic returns typed
/// errors instead of panicking, so this would indicate a bug.
#[must_use]
pub fn process_all(dirs: &[PathBuf], options: &Options) -> Vec<LocaleResult> {
    if effective_jobs(options.jobs) <= 1 || dirs.len() <= 1 {
        return dirs
            .iter()
            .map(|dir| process_locale(dir, options))
            .collect();
    }
    thread::scope(|scope| {
        // Spawn every worker first: joining lazily would serialize the run.
        let mut handles = Vec::with_capacity(dirs.len());
        for dir in dirs {
            handles.push(scope.spawn(|| process_locale(dir, options)));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("locale worker thread panicked"))
            .collect()
    })
}

/// Resolve the requested worker count; `0` means one thread per CPU core.
#[must_use]
pub fn effective_jobs(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    thread::available_parallelism().map_or(1, usize::from)
}

/// Process a single locale directory.
#[must_use]
pub fn process_locale(dir: &Path, options: &Options) -> LocaleResult {
    let locale = dir.file_name().map_or_else(
        || dir.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let outcome = match process_locale_inner(dir, options) {
        Ok(Some(stats)) => LocaleOutcome::Processed { stats },
        Ok(None) => LocaleOutcome::Skipped,
        Err(error) => LocaleOutcome::Failed {
            error: error.to_string(),
        },
    };
    LocaleResult { locale, outcome }
}

fn process_locale_inner(dir: &Path, options: &Options) -> Result<Option<LocaleStats>> {
    let messages_path = dir.join(format!("{}.po", options.filename));
    if !messages_path.is_file() {
        return Ok(None);
    }
    let missing_path = dir.join(format!("{}{}.po", options.filename, MISSING_SUFFIX));
    let mut main = po::parse_po(&messages_path)?;

    let (merged, orphaned) = merge_back(&mut main, &messages_path, &missing_path, options)?;

    let total = main
        .entries
        .iter()
        .filter(|entry| !entry.msgid.is_empty() && !entry.obsolete)
        .count();
    let translated = main
        .entries
        .iter()
        .filter(|entry| po::is_translated(entry, options.include_fuzzy))
        .count();
    let missing = po::missing_entries(&main, options.include_fuzzy);

    let missing_file = if missing.is_empty() {
        remove_missing_file(&missing_path, options)?
    } else {
        write_missing_file(&main, &missing, &missing_path, options)?
    };

    let percent = if total == 0 {
        100.0
    } else {
        to_f64(translated) * 100.0 / to_f64(total)
    };
    Ok(Some(LocaleStats {
        total,
        translated,
        missing: missing.len(),
        percent,
        merged,
        orphaned,
        missing_file,
    }))
}

/// Convert a count to `f64` without a lossy `as` cast.
fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// Merge completed translations from `missing_path` into `main` (saving
/// `messages_path` when it changed).
///
/// Returns `(merged, orphaned)`. A missing file that cannot be parsed is an
/// error: translator work is never silently discarded or overwritten.
fn merge_back(
    main: &mut rspolib::POFile,
    messages_path: &Path,
    missing_path: &Path,
    options: &Options,
) -> Result<(usize, usize)> {
    if !missing_path.is_file() {
        return Ok((0, 0));
    }
    let missing_po = po::parse_po(missing_path)?;
    let index = po::entry_index(main);
    let mut merged = 0;
    let mut orphaned = 0;
    for entry in &missing_po.entries {
        if entry.msgid.is_empty()
            || entry.obsolete
            || !po::is_translated(entry, options.include_fuzzy)
        {
            continue;
        }
        match po::find_index(&index, entry) {
            Some(position) => {
                po::apply_translation(&mut main.entries[position], entry);
                merged += 1;
            }
            None => orphaned += 1,
        }
    }
    if merged > 0 && !options.dry_run {
        po::write_po(main, messages_path)?;
    }
    Ok((merged, orphaned))
}

/// Write the missing file with the entries that still need work.
fn write_missing_file(
    main: &rspolib::POFile,
    missing: &[rspolib::POEntry],
    missing_path: &Path,
    options: &Options,
) -> Result<MissingFileAction> {
    if !options.dry_run {
        let derived = po::derived_file(main, missing.to_vec());
        po::write_po(&derived, missing_path)?;
    }
    Ok(MissingFileAction::Written)
}

/// Remove the missing file when nothing is missing anymore.
fn remove_missing_file(missing_path: &Path, options: &Options) -> Result<MissingFileAction> {
    if missing_path.is_file() {
        if !options.dry_run {
            fs::remove_file(missing_path).map_err(|source| Error::RemoveFile {
                path: missing_path.to_path_buf(),
                source,
            })?;
        }
        return Ok(MissingFileAction::Removed);
    }
    Ok(MissingFileAction::None)
}

#[cfg(test)]
mod tests;
