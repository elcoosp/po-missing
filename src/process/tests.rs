//! Unit tests for the core engine.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{
    MissingFileAction, Options, Summary, discover_locales, effective_jobs, process_all,
    process_locale, run,
};
use crate::error::Result;

/// A fixture locale tree on disk.
struct Fixture {
    _dir: TempDir,
    base: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let base = dir.path().join("locales");
        fs::create_dir_all(&base).expect("mkdir");
        Self { _dir: dir, base }
    }

    fn options(&self) -> Options {
        Options {
            base_path: self.base.clone(),
            ..Options::default()
        }
    }

    fn locale(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir_all(&path).expect("mkdir locale");
        path
    }

    fn write(&self, locale: &str, file: &str, content: &str) {
        let path = self.locale(locale).join(file);
        fs::write(path, content).expect("write fixture");
    }

    fn read(&self, locale: &str, file: &str) -> String {
        fs::read_to_string(self.base.join(locale).join(file)).expect("read fixture")
    }

    fn exists(&self, locale: &str, file: &str) -> bool {
        self.base.join(locale).join(file).is_file()
    }
}

const PO_WITH_METADATA: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\"Language: fr\\n\"\n\n";
const PO_HEADER_ONLY: &str =
    "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n";

/// A missing-translation candidate file content: header plus raw entries.
fn with_header(entries: &str) -> String {
    format!("{PO_HEADER_ONLY}{entries}")
}

fn po(translated: &[&str], missing: &[&str]) -> String {
    let mut content = PO_WITH_METADATA.to_owned();
    let blocks = translated
        .iter()
        .map(|msgid| format!("msgid \"{msgid}\"\nmsgstr \"trad-{msgid}\"\n\n"))
        .chain(
            missing
                .iter()
                .map(|msgid| format!("msgid \"{msgid}\"\nmsgstr \"\"\n\n")),
        );
    content.extend(blocks);
    content
}

fn run_quiet(fixture: &Fixture) -> Result<Summary> {
    run(&fixture.options())
}

#[test]
fn discover_locales_requires_existing_directory() {
    let error = discover_locales(Path::new("/definitely/not/here")).expect_err("must fail");
    assert!(error.to_string().contains("not an existing directory"));
}

#[test]
fn discover_locales_skips_hidden_dirs_and_files_and_sorts() {
    let fixture = Fixture::new();
    fixture.locale("fr");
    fixture.locale("de");
    fixture.locale(".git");
    fs::write(fixture.base.join("stray-file.txt"), "x").expect("write");
    fs::write(fixture.base.join(".hidden"), "x").expect("write");
    let dirs = discover_locales(&fixture.base).expect("ok");
    let names: Vec<String> = dirs
        .iter()
        .map(|path| {
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec!["de", "fr"]);
}

#[test]
fn discover_locales_empty_directory() {
    let fixture = Fixture::new();
    assert_eq!(
        discover_locales(&fixture.base).expect("ok"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn extract_writes_missing_file_with_header_and_stats() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world", "greeting"]));
    let summary = run_quiet(&fixture).expect("ok");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.missing, 2);
    let stats = match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => stats.clone(),
        other => panic!("expected processed, got {other:?}"),
    };
    assert_eq!(stats.total, 3);
    assert_eq!(stats.translated, 1);
    assert_eq!(stats.missing, 2);
    assert_eq!(stats.merged, 0);
    assert_eq!(stats.orphaned, 0);
    assert_eq!(stats.missing_file, MissingFileAction::Written);
    assert!((stats.percent - 100.0 / 3.0).abs() < 0.1);

    let missing_content = fixture.read("fr", "messages-missing.po");
    assert!(missing_content.contains("Content-Type: text/plain; charset=UTF-8"));
    assert!(missing_content.contains("Language: fr"));
    assert!(missing_content.contains("msgid \"world\""));
    assert!(missing_content.contains("msgid \"greeting\""));
    assert!(!missing_content.contains("msgid \"hello\""));
}

#[test]
fn clean_locale_removes_stale_missing_file() {
    let fixture = Fixture::new();
    fixture.write("en", "messages.po", &po(&["hello"], &[]));
    fixture.write(
        "en",
        "messages-missing.po",
        "msgid \"\"\nmsgstr \"\"\n\nmsgid \"stale\"\nmsgstr \"\"\n",
    );
    let summary = run_quiet(&fixture).expect("ok");
    assert_eq!(summary.missing, 0);
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.missing_file, MissingFileAction::Removed);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert!(!fixture.exists("en", "messages-missing.po"));
}

#[test]
fn merge_back_full_and_partial() {
    // Full merge: every entry translated -> main updated, missing file removed.
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["keep"], &["world"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &with_header("msgid \"world\"\nmsgstr \"monde\"\n\n"),
    );
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 0);
            assert_eq!(stats.missing_file, MissingFileAction::Removed);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert!(
        fixture
            .read("fr", "messages.po")
            .contains("msgstr \"monde\"")
    );
    assert!(!fixture.exists("fr", "messages-missing.po"));

    // Partial merge: one translated, one left empty -> missing file rewritten.
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["keep"], &["world", "greeting"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &with_header("msgid \"world\"\nmsgstr \"monde\"\n\nmsgid \"greeting\"\nmsgstr \"\"\n\n"),
    );
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 1);
            assert_eq!(stats.missing_file, MissingFileAction::Written);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    let main = fixture.read("fr", "messages.po");
    assert!(main.contains("msgstr \"monde\""));
    assert!(!main.contains("msgstr \"trad-greeting\""));
    let missing_content = fixture.read("fr", "messages-missing.po");
    assert!(missing_content.contains("msgid \"greeting\""));
    assert!(!missing_content.contains("msgid \"world\""));
}

#[test]
fn merge_back_drops_orphans_and_rewritten_file_purges_them() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["alive"], &["still-missing"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &with_header(
            "msgid \"removed-from-code\"\nmsgstr \"fantome\"\n\nmsgid \"alive\"\nmsgstr \"vivant\"\n\n",
        ),
    );
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.orphaned, 1);
            assert_eq!(stats.missing, 1);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert!(
        fixture
            .read("fr", "messages.po")
            .contains("msgstr \"vivant\"")
    );
    let missing_content = fixture.read("fr", "messages-missing.po");
    assert!(
        !missing_content.contains("removed-from-code"),
        "orphans must be purged: {missing_content}"
    );
    assert!(missing_content.contains("msgid \"still-missing\""));
}

#[test]
fn merge_back_respects_msgctxt() {
    let fixture = Fixture::new();
    let main = format!(
        "{PO_WITH_METADATA}msgid \"save\"\nmsgstr \"\"\n\nmsgctxt \"menu\"\nmsgid \"save\"\nmsgstr \"\"\n\n"
    );
    fixture.write("fr", "messages.po", &main);
    let missing = with_header("msgctxt \"menu\"\nmsgid \"save\"\nmsgstr \"enregistrer\"\n\n");
    fixture.write("fr", "messages-missing.po", &missing);
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 1);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    let updated = fixture.read("fr", "messages.po");
    assert!(updated.contains("msgctxt \"menu\""));
    assert!(updated.contains("msgstr \"enregistrer\""));
}

#[test]
fn plural_entries_are_missing_until_all_forms_filled() {
    let fixture = Fixture::new();
    let main = format!(
        "{PO_WITH_METADATA}msgid \"apples\"\nmsgid_plural \"apples\"\nmsgstr[0] \"\"\nmsgstr[1] \"\"\n\nmsgid \"done\"\nmsgid_plural \"done\"\nmsgstr[0] \"un\"\nmsgstr[1] \"des\"\n\n"
    );
    fixture.write("fr", "messages.po", &main);
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => assert_eq!(stats.missing, 1),
        other => panic!("expected processed, got {other:?}"),
    }
    let missing_content = fixture.read("fr", "messages-missing.po");
    assert!(missing_content.contains("msgid \"apples\""));
    assert!(!missing_content.contains("msgid \"done\""));

    // Merge back plural forms.
    let plural_translation = with_header(
        "msgid \"apples\"\nmsgid_plural \"apples\"\nmsgstr[0] \"une pomme\"\nmsgstr[1] \"des pommes\"\n\n",
    );
    fixture.write("fr", "messages-missing.po", &plural_translation);
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 0);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert!(
        fixture
            .read("fr", "messages.po")
            .contains("msgstr[1] \"des pommes\"")
    );
}

#[test]
fn fuzzy_entries_are_missing_by_default_and_merged_with_flag() {
    let fixture = Fixture::new();
    let main = format!("{PO_WITH_METADATA}msgid \"hello\"\nmsgstr \"\"\n\n");
    fixture.write("fr", "messages.po", &main);
    let fuzzy = with_header("msgid \"hello\"\nmsgstr \"bonjour\"\n\n");
    let mut fuzzy_missing = rspolib::pofile(fuzzy.as_str()).expect("parse");
    fuzzy_missing.entries[0].flags = vec!["fuzzy".to_owned()];
    fixture.write("fr", "messages-missing.po", &fuzzy_missing.to_string());

    // Default: fuzzy translation is NOT accepted, stays missing (dry run
    // keeps the fuzzy missing file on disk for the next phase).
    let options = Options {
        dry_run: true,
        ..fixture.options()
    };
    let summary = run(&options).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 0);
            assert_eq!(stats.missing, 1);
        }
        other => panic!("expected processed, got {other:?}"),
    }

    // With include_fuzzy: accepted and the fuzzy flag is cleared.
    let options = Options {
        include_fuzzy: true,
        ..fixture.options()
    };
    let summary = run(&options).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 0);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    let main = fixture.read("fr", "messages.po");
    assert!(main.contains("msgstr \"bonjour\""));
    assert!(
        !main.contains("fuzzy"),
        "fuzzy flag must be cleared on merge: {main}"
    );
}

#[test]
fn dry_run_touches_nothing() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["keep"], &["world"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &with_header("msgid \"world\"\nmsgstr \"monde\"\n\n"),
    );
    let before_main = fixture.read("fr", "messages.po");
    let before_missing = fixture.read("fr", "messages-missing.po");

    let options = Options {
        dry_run: true,
        ..fixture.options()
    };
    let summary = run(&options).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.merged, 1);
            assert_eq!(stats.missing, 0);
            assert_eq!(stats.missing_file, MissingFileAction::Removed);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert_eq!(fixture.read("fr", "messages.po"), before_main);
    assert_eq!(fixture.read("fr", "messages-missing.po"), before_missing);
}

#[test]
fn corrupt_main_file_is_reported_and_kept() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", "msgid \"unescaped\nmsgstr \"\"\n");
    let summary = run_quiet(&fixture).expect("run itself succeeds");
    assert_eq!(summary.failed, 1);
    match &summary.results[0].outcome {
        super::LocaleOutcome::Failed { error } => assert!(error.contains("messages.po")),
        other => panic!("expected failed, got {other:?}"),
    }
}

#[test]
fn corrupt_missing_file_is_reported_and_never_deleted() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&[], &["world"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        "msgid \"broken\\quote\nmsgstr \"\"\n",
    );
    let summary = run_quiet(&fixture).expect("run itself succeeds");
    assert_eq!(summary.failed, 1);
    assert!(
        fixture.exists("fr", "messages-missing.po"),
        "translator file must survive"
    );
}

#[test]
fn locale_without_messages_po_is_skipped() {
    let fixture = Fixture::new();
    fixture.locale("empty-locale");
    fixture.write("fr", "messages.po", &po(&["hello"], &[]));
    let summary = run_quiet(&fixture).expect("ok");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.skipped, 1);
    assert!(matches!(
        summary.results[0].outcome,
        super::LocaleOutcome::Skipped
    ));
}

#[test]
fn empty_po_file_counts_as_fully_translated() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", "");
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(stats.total, 0);
            assert_eq!(stats.missing, 0);
            assert!((stats.percent - 100.0).abs() < f64::EPSILON);
            assert_eq!(stats.missing_file, MissingFileAction::None);
        }
        other => panic!("expected processed, got {other:?}"),
    }
    assert!(!fixture.exists("fr", "messages-missing.po"));
}

#[test]
fn custom_filename_is_used() {
    let fixture = Fixture::new();
    fixture.write("fr", "django.po", &po(&[], &["world"]));
    let options = Options {
        filename: "django".to_owned(),
        ..fixture.options()
    };
    let summary = run(&options).expect("ok");
    assert_eq!(summary.missing, 1);
    assert!(fixture.exists("fr", "django-missing.po"));
    assert!(!fixture.exists("fr", "messages.po"));
}

#[test]
fn obsolete_entries_are_ignored_everywhere() {
    let fixture = Fixture::new();
    let main = format!(
        "{PO_WITH_METADATA}#~ msgid \"dead\"\n#~ msgstr \"\"\n\nmsgid \"alive\"\nmsgstr \"\"\n\n"
    );
    fixture.write("fr", "messages.po", &main);
    let summary = run_quiet(&fixture).expect("ok");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Processed { stats } => {
            assert_eq!(
                stats.total, 1,
                "obsolete entries are not part of the workload"
            );
            assert_eq!(stats.missing, 1);
        }
        other => panic!("expected processed, got {other:?}"),
    }
}

#[test]
fn non_utf8_locale_dir_fails_without_panicking() {
    use std::os::unix::ffi::OsStrExt as _;
    let fixture = Fixture::new();
    let weird = std::ffi::OsStr::from_bytes(b"weird-\xff-name");
    let locale_dir = fixture.base.join(weird);
    fs::create_dir_all(&locale_dir).expect("mkdir");
    fs::write(locale_dir.join("messages.po"), po(&["a"], &[])).expect("write");
    let summary = run_quiet(&fixture).expect("ok");
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.results[0].locale, "weird-\u{FFFD}-name");
    match &summary.results[0].outcome {
        super::LocaleOutcome::Failed { error } => {
            assert!(error.contains("not valid UTF-8"), "got: {error}");
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

#[test]
fn parallel_and_serial_runs_agree() {
    let make_fixture = || {
        let fixture = Fixture::new();
        fixture.write("fr", "messages.po", &po(&["a"], &["b", "c"]));
        fixture.write("de", "messages.po", &po(&["x", "y"], &["z"]));
        fixture.write("en", "messages.po", &po(&["q"], &[]));
        fixture
    };
    let serial = run(&Options {
        jobs: 1,
        ..make_fixture().options()
    })
    .expect("ok");
    let parallel = run(&Options {
        jobs: 4,
        ..make_fixture().options()
    })
    .expect("ok");
    assert_eq!(serial.processed, parallel.processed);
    assert_eq!(serial.missing, parallel.missing);
    let serial_names: Vec<&str> = serial.results.iter().map(|r| r.locale.as_str()).collect();
    let parallel_names: Vec<&str> = parallel.results.iter().map(|r| r.locale.as_str()).collect();
    assert_eq!(serial_names, vec!["de", "en", "fr"]);
    assert_eq!(serial_names, parallel_names);
    assert_eq!(serial.merged, parallel.merged);
    assert_eq!(serial.failed, parallel.failed);
}

#[test]
fn process_all_with_multiple_locales_and_threads() {
    let fixture = Fixture::new();
    for name in ["a", "b", "c", "d"] {
        fixture.write(name, "messages.po", &po(&["x"], &["y"]));
    }
    let dirs = discover_locales(&fixture.base).expect("ok");
    let options = fixture.options();
    let results = process_all(&dirs, &options);
    assert_eq!(results.len(), 4);
    assert!(
        results
            .iter()
            .all(|r| matches!(r.outcome, super::LocaleOutcome::Processed { .. }))
    );
}

#[test]
fn effective_jobs_resolves_zero_to_cpus() {
    assert!(effective_jobs(0) >= 1);
    assert_eq!(effective_jobs(3), 3);
}

#[test]
fn single_locale_run_is_serial_even_with_jobs() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&[], &["world"]));
    let options = Options {
        jobs: 8,
        ..fixture.options()
    };
    let summary = run(&options).expect("ok");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.missing, 1);
}

#[test]
fn process_locale_on_missing_directory_fails_gracefully() {
    let options = Options::default();
    let result = process_locale(Path::new("/definitely/not/here"), &options);
    assert!(matches!(result.outcome, super::LocaleOutcome::Skipped));
}

#[cfg(unix)]
#[test]
fn unreadable_directory_maps_to_read_dir_error() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    // Lock the base directory itself: stat still succeeds (so the is_dir
    // guard passes) but listing it fails with EACCES.
    fs::set_permissions(&fixture.base, fs::Permissions::from_mode(0o000)).expect("chmod");
    let result = discover_locales(&fixture.base);
    fs::set_permissions(&fixture.base, fs::Permissions::from_mode(0o755)).expect("restore");
    if fs::read_dir(&fixture.base).is_err() {
        // Non-root: the lock really applied.
        let error = result.expect_err("locked dir must fail");
        assert!(
            error.to_string().contains("failed to read directory"),
            "{error}"
        );
    }
    // Running as root the lock is bypassed; discovery then simply succeeds.
}

#[cfg(unix)]
#[test]
fn unwritable_locale_dir_reports_write_failure() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["keep"], &["world"]));
    let dir = fixture.base.join("fr");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");
    let summary = run_quiet(&fixture);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("restore");
    if fs::read_dir(&dir).is_ok() && fs::write(dir.join("probe.tmp"), "x").is_ok() {
        fs::remove_file(dir.join("probe.tmp")).ok();
        return; // root bypasses the lock; nothing to assert.
    }
    let summary = summary.expect("run itself succeeds");
    assert_eq!(summary.failed, 1);
    match &summary.results[0].outcome {
        super::LocaleOutcome::Failed { error } => {
            assert!(error.contains("failed to write"), "{error}");
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn unwritable_locale_dir_reports_remove_failure() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["done"], &[]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &with_header("msgid \"stale\"\nmsgstr \"\"\n\n"),
    );
    let dir = fixture.base.join("fr");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");
    let summary = run_quiet(&fixture);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("restore");
    if fs::read_dir(&dir).is_ok() && fs::write(dir.join("probe.tmp"), "x").is_ok() {
        fs::remove_file(dir.join("probe.tmp")).ok();
        return; // root bypasses the lock.
    }
    let summary = summary.expect("run itself succeeds");
    assert_eq!(summary.failed, 1);
    match &summary.results[0].outcome {
        super::LocaleOutcome::Failed { error } => {
            assert!(error.contains("failed to remove"), "{error}");
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

#[test]
fn locale_name_falls_back_to_full_path_for_dotdot() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["a"], &[]));
    let dotdot = fixture.base.join("fr").join("..");
    let result = process_locale(&dotdot, &fixture.options());
    assert!(
        result.locale.ends_with(".."),
        "fallback name: {}",
        result.locale
    );
    // The lookup result depends on the filesystem's `..` resolution, but the
    // fallback name computation above is what this test covers.
    assert!(matches!(
        result.outcome,
        super::LocaleOutcome::Processed { .. } | super::LocaleOutcome::Skipped
    ));
}
