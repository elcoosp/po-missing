//! CLI entry point: parse arguments, run the engine, render output.
//!
//! All rendering lives in functions so the unit tests can exercise it
//! directly; the integration tests in `tests/` drive the real binary.

mod cli;

use std::fmt::Write as _;
use std::process::ExitCode;

use clap::Parser as _;

use cli::Cli;
use po_missing::{LocaleOutcome, MissingFileAction, Summary};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = cli.options();
    let summary = match po_missing::run(&options) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if cli.is_json() {
        println!("{}", json_summary(&summary));
    } else if !cli.is_quiet() {
        print!("{}", text_summary(&summary, cli.is_verbose()));
    }
    exit_code(&summary, cli.fail_on_missing())
}

/// Map a run summary to the documented exit code.
///
/// * `0` — success;
/// * `1` — at least one locale failed;
/// * `2` — `--fail-on-missing` was requested and missing translations remain.
fn exit_code(summary: &Summary, fail_on_missing: bool) -> ExitCode {
    if summary.failed > 0 {
        ExitCode::FAILURE
    } else if fail_on_missing && summary.missing > 0 {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

/// Human-readable report (per-locale lines only in verbose mode).
fn text_summary(summary: &Summary, verbose: bool) -> String {
    let mut out = String::new();
    for result in &summary.results {
        match &result.outcome {
            LocaleOutcome::Processed { stats } if verbose => {
                let missing_note = match stats.missing_file {
                    MissingFileAction::Written => format!(", {} missing extracted", stats.missing),
                    MissingFileAction::Removed => String::from(", missing file removed"),
                    MissingFileAction::None => String::new(),
                };
                let _ = writeln!(
                    out,
                    "  ✅ {}: {:.1}% translated, {} merged, {} orphaned{missing_note}",
                    result.locale, stats.percent, stats.merged, stats.orphaned
                );
            }
            LocaleOutcome::Skipped if verbose => {
                let _ = writeln!(out, "  ⏭️  {}: no messages file, skipped", result.locale);
            }
            LocaleOutcome::Failed { error } => {
                let _ = writeln!(out, "  ❌ {}: {error}", result.locale);
            }
            LocaleOutcome::Processed { .. } | LocaleOutcome::Skipped => {}
        }
    }
    let _ = writeln!(
        out,
        "{} locale(s) processed, {} skipped, {} merged, {} missing, {} error(s)",
        summary.processed, summary.skipped, summary.merged, summary.missing, summary.failed
    );
    out
}

/// Machine-readable JSON report.
fn json_summary(summary: &Summary) -> String {
    serde_json::to_string(summary).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{exit_code, json_summary, text_summary};
    use po_missing::{LocaleOutcome, LocaleResult, LocaleStats, MissingFileAction, Summary};
    use std::process::ExitCode;

    fn stats(missing: usize, merged: usize) -> LocaleStats {
        let translated = u32::try_from(10 - missing).unwrap_or(u32::MAX);
        LocaleStats {
            total: 10,
            translated: 10 - missing,
            missing,
            percent: f64::from(translated) * 10.0,
            merged,
            orphaned: 0,
            missing_file: if missing > 0 {
                MissingFileAction::Written
            } else {
                MissingFileAction::None
            },
        }
    }

    fn summary(results: Vec<LocaleResult>) -> Summary {
        Summary::new(results)
    }

    #[test]
    fn text_summary_verbose_lists_locales() {
        let summary = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Processed { stats: stats(3, 1) },
        }]);
        let text = text_summary(&summary, true);
        assert!(
            text.contains("✅ fr: 70.0% translated, 1 merged, 0 orphaned, 3 missing extracted")
        );
        assert!(
            text.ends_with("1 locale(s) processed, 0 skipped, 1 merged, 3 missing, 0 error(s)\n")
        );
    }

    #[test]
    fn text_summary_quiet_only_prints_the_recap() {
        let summary = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Processed { stats: stats(0, 0) },
        }]);
        let text = text_summary(&summary, false);
        assert!(!text.contains("✅"));
        assert!(text.contains("0 missing"));
    }

    #[test]
    fn text_summary_reports_removed_missing_file_and_skips() {
        let mut clean = stats(0, 0);
        clean.missing_file = MissingFileAction::Removed;
        let summary = summary(vec![
            LocaleResult {
                locale: String::from("en"),
                outcome: LocaleOutcome::Processed { stats: clean },
            },
            LocaleResult {
                locale: String::from("zz"),
                outcome: LocaleOutcome::Skipped,
            },
            LocaleResult {
                locale: String::from("broken"),
                outcome: LocaleOutcome::Failed {
                    error: String::from("boom"),
                },
            },
        ]);
        let text = text_summary(&summary, true);
        assert!(text.contains("missing file removed"));
        assert!(text.contains("⏭️  zz: no messages file, skipped"));
        assert!(text.contains("❌ broken: boom"));
        assert!(text.contains("1 skipped"));
    }

    #[test]
    fn json_summary_is_valid_and_complete() {
        let summary = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Processed { stats: stats(2, 4) },
        }]);
        let json = json_summary(&summary);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["results"][0]["locale"], "fr");
        assert_eq!(parsed["results"][0]["status"], "processed");
        assert_eq!(parsed["results"][0]["missing"], 2);
        assert_eq!(parsed["results"][0]["merged"], 4);
        assert_eq!(parsed["results"][0]["missing_file"], "written");
        assert_eq!(parsed["processed"], 1);
    }

    #[test]
    fn exit_codes_follow_the_contract() {
        let clean = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Processed { stats: stats(0, 0) },
        }]);
        let dirty = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Processed { stats: stats(5, 0) },
        }]);
        let failed = summary(vec![LocaleResult {
            locale: String::from("fr"),
            outcome: LocaleOutcome::Failed {
                error: String::from("boom"),
            },
        }]);
        assert_eq!(exit_code(&clean, false), ExitCode::SUCCESS);
        assert_eq!(exit_code(&clean, true), ExitCode::SUCCESS);
        assert_eq!(exit_code(&dirty, false), ExitCode::SUCCESS);
        assert_eq!(exit_code(&dirty, true), ExitCode::from(2));
        assert_eq!(exit_code(&failed, false), ExitCode::FAILURE);
    }
}
