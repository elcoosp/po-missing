//! End-to-end tests driving the compiled `po-missing` binary.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_po-missing");

/// A fixture locale tree plus the temp dir that owns it.
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

    fn locale(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir_all(&path).expect("mkdir locale");
        path
    }

    fn write(&self, locale: &str, file: &str, content: &str) {
        fs::write(self.locale(locale).join(file), content).expect("write fixture");
    }

    fn read(&self, locale: &str, file: &str) -> String {
        fs::read_to_string(self.base.join(locale).join(file)).expect("read fixture")
    }

    fn exists(&self, locale: &str, file: &str) -> bool {
        self.base.join(locale).join(file).is_file()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("--base-path")
            .arg(&self.base)
            .args(args)
            .output()
            .expect("spawn binary")
    }
}

const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n";

fn po(translated: &[&str], missing: &[&str]) -> String {
    let mut content = HEADER.to_owned();
    for msgid in translated {
        content.push_str("msgid \"");
        content.push_str(msgid);
        content.push_str("\"\nmsgstr \"trad-");
        content.push_str(msgid);
        content.push_str("\"\n\n");
    }
    for msgid in missing {
        content.push_str("msgid \"");
        content.push_str(msgid);
        content.push_str("\"\nmsgstr \"\"\n\n");
    }
    content
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_and_help_exit_successfully() {
    for flag in ["--version", "-V", "--help", "-h"] {
        let output = Command::new(BIN).arg(flag).output().expect("spawn");
        assert!(output.status.success(), "{flag} must succeed");
    }
}

#[test]
fn extracts_missing_translations_and_preserves_metadata() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let output = fixture.run(&["--verbose"]);
    assert!(output.status.success());
    assert!(fixture.exists("fr", "messages-missing.po"));
    let missing = fixture.read("fr", "messages-missing.po");
    assert!(missing.contains("Content-Type: text/plain; charset=UTF-8"));
    assert!(missing.contains("msgid \"world\""));
    assert!(!missing.contains("msgid \"hello\""));
    let text = stdout(&output);
    assert!(text.contains("✅ fr:"), "verbose line missing: {text}");
    assert!(text.contains("1 locale(s) processed"));
}

#[test]
fn default_output_is_only_the_recap() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let output = fixture.run(&[]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(!text.contains("✅"), "recap only: {text}");
    assert!(text.contains("1 locale(s) processed, 0 skipped, 0 merged, 1 missing, 0 error(s)"));
}

#[test]
fn quiet_prints_nothing() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let output = fixture.run(&["--quiet"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output), "");
}

#[test]
fn json_output_is_machine_readable() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let output = fixture.run(&["--json"]);
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(parsed["results"][0]["locale"], "fr");
    assert_eq!(parsed["results"][0]["status"], "processed");
    assert_eq!(parsed["results"][0]["missing"], 1);
    assert_eq!(parsed["results"][0]["missing_file"], "written");
    assert_eq!(parsed["missing"], 1);
}

#[test]
fn dry_run_never_writes() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let before = fixture.read("fr", "messages.po");
    let output = fixture.run(&["--dry-run", "--verbose"]);
    assert!(output.status.success());
    assert!(!fixture.exists("fr", "messages-missing.po"));
    assert_eq!(fixture.read("fr", "messages.po"), before);
}

#[test]
fn merge_flow_completes_and_cleans_up() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &format!("{HEADER}msgid \"world\"\nmsgstr \"monde\"\n\n"),
    );
    let output = fixture.run(&["--verbose"]);
    assert!(output.status.success());
    assert!(
        fixture
            .read("fr", "messages.po")
            .contains("msgstr \"monde\"")
    );
    assert!(!fixture.exists("fr", "messages-missing.po"));
    assert!(stdout(&output).contains("missing file removed"));
}

#[test]
fn fail_on_missing_exit_code_2() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&["hello"], &["world"]));
    let output = fixture.run(&["--fail-on-missing"]);
    assert_eq!(output.status.code(), Some(2));

    fixture.write(
        "fr",
        "messages-missing.po",
        &format!("{HEADER}msgid \"world\"\nmsgstr \"monde\"\n\n"),
    );
    let output = fixture.run(&["--fail-on-missing"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn missing_base_directory_fails_with_exit_1() {
    let output = Command::new(BIN)
        .arg("--base-path")
        .arg("/definitely/not/here")
        .output()
        .expect("spawn");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("error: base path"));
}

#[test]
fn corrupt_main_file_fails_with_exit_1() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", "msgid \"unescaped\nmsgstr \"\"\n");
    let output = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("❌ fr:"),
        "failed locale is reported: {}",
        stdout(&output)
    );
}

#[test]
fn corrupt_missing_file_is_preserved_and_reported() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&[], &["world"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        "msgid \"broken\\quote\nmsgstr \"\"\n",
    );
    let output = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        fixture.exists("fr", "messages-missing.po"),
        "translator work survives"
    );
}

#[test]
fn custom_filename_flag_is_honored() {
    let fixture = Fixture::new();
    fixture.write("fr", "django.po", &po(&[], &["world"]));
    let output = fixture.run(&["--filename", "django", "--verbose"]);
    assert!(output.status.success());
    assert!(fixture.exists("fr", "django-missing.po"));
    assert!(!fixture.exists("fr", "messages.po"));
}

#[test]
fn hidden_directories_are_ignored() {
    let fixture = Fixture::new();
    fixture.write(".git", "messages.po", &po(&[], &["world"]));
    fixture.write("fr", "messages.po", &po(&["hello"], &[]));
    let output = fixture.run(&["--verbose"]);
    assert!(output.status.success());
    assert!(
        fixture.exists(".git", "messages.po"),
        "hidden dir untouched"
    );
    assert!(!fixture.exists(".git", "messages-missing.po"));
    assert!(stdout(&output).contains("1 locale(s) processed"));
}

#[test]
fn locale_without_messages_file_is_skipped() {
    let fixture = Fixture::new();
    fixture.locale("empty");
    fixture.write("fr", "messages.po", &po(&["hello"], &[]));
    let output = fixture.run(&["--verbose"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("⏭️  empty"));
    assert!(text.contains("1 locale(s) processed, 1 skipped"));
}

#[test]
fn include_fuzzy_merges_fuzzy_translations() {
    let fixture = Fixture::new();
    fixture.write("fr", "messages.po", &po(&[], &["hello"]));
    fixture.write(
        "fr",
        "messages-missing.po",
        &format!("{HEADER}#, fuzzy\nmsgid \"hello\"\nmsgstr \"bonjour\"\n\n"),
    );
    // Default: fuzzy translation stays out of the main file (dry run keeps
    // the translator's file intact for the next phase).
    let default_run = fixture.run(&["--dry-run"]);
    assert!(default_run.status.success());
    assert!(
        fixture.read("fr", "messages.po").contains("msgstr \"\""),
        "fuzzy not merged by default"
    );
    assert!(
        fixture
            .read("fr", "messages-missing.po")
            .contains("bonjour"),
        "translator file untouched"
    );

    let fuzzy_output = fixture.run(&["--include-fuzzy"]);
    assert!(fuzzy_output.status.success());
    assert!(
        fixture
            .read("fr", "messages.po")
            .contains("msgstr \"bonjour\"")
    );
}

#[test]
fn jobs_flag_changes_nothing_semantically() {
    let make_fixture = || {
        let fixture = Fixture::new();
        fixture.write("fr", "messages.po", &po(&["a"], &["b"]));
        fixture.write("de", "messages.po", &po(&["x"], &["y"]));
        fixture
    };
    let one = make_fixture().run(&["--json", "--jobs", "1"]);
    let four = make_fixture().run(&["--json", "--jobs", "4"]);
    assert_eq!(
        stdout(&one),
        stdout(&four),
        "JSON output must be job-independent"
    );
    assert!(one.status.success() && four.status.success());
}

#[test]
fn unknown_flag_exits_with_usage_error() {
    let output = Command::new(BIN).arg("--bogus").output().expect("spawn");
    assert!(!output.status.success());
    assert_ne!(stderr(&output), "");
}

#[test]
fn locale_directories_are_processed_in_sorted_order() {
    let fixture = Fixture::new();
    fixture.write("zz", "messages.po", &po(&["a"], &[]));
    fixture.write("aa", "messages.po", &po(&["b"], &[]));
    fixture.write("mm", "messages.po", &po(&["c"], &[]));
    let output = fixture.run(&["--verbose"]);
    assert!(output.status.success());
    let text = stdout(&output);
    let aa = text.find("✅ aa:").expect("aa line");
    let mm = text.find("✅ mm:").expect("mm line");
    let zz = text.find("✅ zz:").expect("zz line");
    assert!(aa < mm && mm < zz, "sorted output expected: {text}");
}
