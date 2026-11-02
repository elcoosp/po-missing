//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;

use po_missing::{DEFAULT_FILENAME, Options};

/// Flags controlling how results are reported.
#[derive(Parser, Debug)]
pub struct OutputFlags {
    /// Print per-locale details
    #[arg(short, long)]
    pub verbose: bool,

    /// Print nothing (errors still go to stderr)
    #[arg(short, long)]
    pub quiet: bool,

    /// Print a machine-readable JSON summary instead of text
    #[arg(long)]
    pub json: bool,
}

/// Flags controlling processing behavior.
#[derive(Parser, Debug)]
pub struct BehaviorFlags {
    /// Report intended changes without writing any file
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Exit with status 2 when missing translations remain
    #[arg(long)]
    pub fail_on_missing: bool,

    /// Treat fuzzy entries as translated (the fuzzy flag is cleared on merge)
    #[arg(short = 'i', long)]
    pub include_fuzzy: bool,
}

/// Extract missing translations from PO files and merge completed ones back.
#[derive(Parser, Debug)]
#[command(name = "po-missing", version, about, long_about = None)]
pub struct Cli {
    /// Base directory containing one subdirectory per locale
    #[arg(
        short,
        long,
        default_value = "frontend/src/locales",
        value_name = "DIR"
    )]
    pub base_path: PathBuf,

    /// Base filename without extension; processes `<NAME>.po` and
    /// `<NAME>-missing.po` inside every locale directory
    #[arg(short, long, default_value = DEFAULT_FILENAME, value_name = "NAME")]
    pub filename: String,

    /// Reporting options
    #[command(flatten)]
    pub output: OutputFlags,

    /// Behavior options
    #[command(flatten)]
    pub behavior: BehaviorFlags,

    /// Parallel workers for locale processing, 0 means one per CPU core
    #[arg(short = 'j', long, default_value_t = 0, value_name = "N")]
    pub jobs: usize,
}

impl Cli {
    /// Convert the CLI arguments into library [`Options`].
    #[must_use]
    pub fn options(&self) -> Options {
        Options {
            base_path: self.base_path.clone(),
            filename: self.filename.clone(),
            include_fuzzy: self.behavior.include_fuzzy,
            dry_run: self.behavior.dry_run,
            jobs: self.jobs,
        }
    }

    /// Whether per-locale details should be printed.
    #[must_use]
    pub const fn is_verbose(&self) -> bool {
        self.output.verbose
    }

    /// Whether all output should be suppressed.
    #[must_use]
    pub const fn is_quiet(&self) -> bool {
        self.output.quiet
    }

    /// Whether the summary should be printed as JSON.
    #[must_use]
    pub const fn is_json(&self) -> bool {
        self.output.json
    }

    /// Whether the run should fail when missing translations remain.
    #[must_use]
    pub const fn fail_on_missing(&self) -> bool {
        self.behavior.fail_on_missing
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser as _;

    #[test]
    fn defaults_match_the_documented_cli() {
        let cli = Cli::try_parse_from(["po-missing"]).expect("valid defaults");
        assert_eq!(cli.base_path.to_string_lossy(), "frontend/src/locales");
        assert_eq!(cli.filename, "messages");
        assert!(!cli.is_verbose());
        assert!(!cli.is_quiet());
        assert!(!cli.behavior.dry_run);
        assert!(!cli.is_json());
        assert!(!cli.fail_on_missing());
        assert!(!cli.behavior.include_fuzzy);
        assert_eq!(cli.jobs, 0);
    }

    #[test]
    fn options_maps_every_flag() {
        let cli = Cli::try_parse_from([
            "po-missing",
            "-b",
            "/tmp/locales",
            "-f",
            "django",
            "-v",
            "-n",
            "-i",
            "-j",
            "3",
        ])
        .expect("valid args");
        let options = cli.options();
        assert_eq!(options.base_path.to_string_lossy(), "/tmp/locales");
        assert_eq!(options.filename, "django");
        assert!(options.include_fuzzy);
        assert!(options.dry_run);
        assert_eq!(options.jobs, 3);
    }

    #[test]
    fn long_flags_parse() {
        let cli = Cli::try_parse_from([
            "po-missing",
            "--base-path",
            "l",
            "--filename",
            "ui",
            "--verbose",
            "--quiet",
            "--dry-run",
            "--json",
            "--fail-on-missing",
            "--include-fuzzy",
            "--jobs",
            "2",
        ])
        .expect("valid args");
        assert!(cli.is_verbose() && cli.is_quiet() && cli.is_json() && cli.fail_on_missing());
        assert!(cli.behavior.dry_run && cli.behavior.include_fuzzy);
        assert_eq!(cli.jobs, 2);
        assert_eq!(cli.filename, "ui");
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(Cli::try_parse_from(["po-missing", "--nope"]).is_err());
    }
}
