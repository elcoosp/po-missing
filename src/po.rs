//! Thin helpers over [`rspolib`] that define what "missing" means.
//!
//! Every phase (merge, extraction, statistics) funnels through these helpers
//! so the semantics live in exactly one place:
//!
//! * the header entry (`msgid == ""`) and obsolete entries are ignored;
//! * a simple entry is translated when `msgstr` is non-blank;
//! * a plural entry is translated when *every* `msgstr[n]` form is non-blank;
//! * fuzzy entries count as untranslated unless fuzzy handling is disabled.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rspolib::{FileOptions, POEntry, POFile};

use crate::error::{Error, Result};

/// Key that uniquely identifies an entry: `msgctxt` + `msgid`.
pub type EntryKey = (Option<String>, String);

/// Build the identity key of an entry.
pub fn entry_key(entry: &POEntry) -> EntryKey {
    (entry.msgctxt.clone(), entry.msgid.clone())
}

/// Whether an entry counts as fully translated.
///
/// Fuzzy entries only count when `include_fuzzy` is set; the header entry and
/// obsolete entries never count.
pub fn is_translated(entry: &POEntry, include_fuzzy: bool) -> bool {
    if entry.msgid.is_empty() || entry.obsolete {
        return false;
    }
    if entry.fuzzy() && !include_fuzzy {
        return false;
    }
    if entry.msgid_plural.is_some() || !entry.msgstr_plural.is_empty() {
        return !entry.msgstr_plural.is_empty()
            && entry
                .msgstr_plural
                .iter()
                .all(|form| !form.trim().is_empty());
    }
    entry
        .msgstr
        .as_ref()
        .is_some_and(|msgstr| !msgstr.trim().is_empty())
}

/// Whether an entry still needs translation work.
///
/// Obsolete entries are dropped strings, so they never need work.
pub fn needs_translation(entry: &POEntry, include_fuzzy: bool) -> bool {
    !entry.msgid.is_empty() && !entry.obsolete && !is_translated(entry, include_fuzzy)
}

/// Entries that need translation, in file order.
pub fn missing_entries(po: &POFile, include_fuzzy: bool) -> Vec<POEntry> {
    po.entries
        .iter()
        .filter(|e| needs_translation(e, include_fuzzy))
        .cloned()
        .collect()
}

/// Build an index from entry key to position in `po.entries`.
pub fn entry_index(po: &POFile) -> HashMap<EntryKey, usize> {
    po.entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry_key(entry), index))
        .collect()
}

/// Look up an entry position by identity key.
pub fn find_index(index: &HashMap<EntryKey, usize>, entry: &POEntry) -> Option<usize> {
    index.get(&entry_key(entry)).copied()
}

/// Copy a completed translation (including plural forms) into a target entry.
///
/// The `fuzzy` flag is cleared: merging means accepting the translation.
pub fn apply_translation(target: &mut POEntry, source: &POEntry) {
    target.msgstr.clone_from(&source.msgstr);
    target.msgstr_plural.clone_from(&source.msgstr_plural);
    target.flags.retain(|flag| flag != "fuzzy");
}

/// Parse a PO file, wrapping failures with the file path.
pub fn parse_po(path: &Path) -> Result<POFile> {
    let Some(path_str) = path.to_str() else {
        return Err(Error::NonUtf8Path {
            path: path.to_path_buf(),
        });
    };
    rspolib::pofile(path_str).map_err(|source| Error::parse_po(path, source))
}

/// Write a PO file, wrapping failures with the file path.
pub fn write_po(po: &POFile, path: &Path) -> Result<()> {
    fs::write(path, po.to_string()).map_err(|source| Error::write_file(path, source))
}

/// Build a new PO file that shares header, metadata and wrap width with
/// `source` but contains exactly `entries`.
pub fn derived_file(source: &POFile, entries: Vec<POEntry>) -> POFile {
    let mut derived = POFile::new(FileOptions {
        path_or_content: String::new(),
        wrapwidth: source.options.wrapwidth,
        byte_content: None,
    });
    derived.header.clone_from(&source.header);
    derived.metadata.clone_from(&source.metadata);
    derived.metadata_is_fuzzy = source.metadata_is_fuzzy;
    derived.entries = entries;
    derived
}

#[cfg(test)]
mod tests {
    use super::{
        apply_translation, derived_file, entry_index, entry_key, find_index, is_translated,
        missing_entries, needs_translation, parse_po, write_po,
    };
    use rspolib::{POEntry, POFile};

    fn entry(msgid: &str, msgstr: Option<&str>) -> POEntry {
        let mut entry = POEntry::new(0);
        entry.msgid = msgid.to_owned();
        entry.msgstr = msgstr.map(str::to_owned);
        entry
    }

    fn plural_entry(forms: Vec<&str>) -> POEntry {
        let mut entry = entry("apples", None);
        entry.msgid_plural = Some("apples".to_owned());
        entry.msgstr_plural = forms.into_iter().map(str::to_owned).collect();
        entry
    }

    #[test]
    fn simple_translation_matrix() {
        assert!(is_translated(&entry("a", Some("b")), false));
        assert!(!is_translated(&entry("a", None), false));
        assert!(!is_translated(&entry("a", Some("")), false));
        assert!(!is_translated(&entry("a", Some("   ")), false));
        assert!(!is_translated(&entry("", Some("header-ish")), false));
        let mut obsolete = entry("a", Some("b"));
        obsolete.obsolete = true;
        assert!(!is_translated(&obsolete, false));
    }

    #[test]
    fn fuzzy_respects_the_flag() {
        let mut fuzzy = entry("a", Some("b"));
        fuzzy.flags = vec!["fuzzy".to_owned()];
        assert!(!is_translated(&fuzzy, false));
        assert!(is_translated(&fuzzy, true));
    }

    #[test]
    fn plural_translation_matrix() {
        assert!(is_translated(
            &plural_entry(vec!["un", "une", "des"]),
            false
        ));
        assert!(!is_translated(&plural_entry(vec!["un", ""]), false));
        assert!(!is_translated(&plural_entry(vec!["  "]), false));
        let mut empty_forms = entry("apples", None);
        empty_forms.msgid_plural = Some("apples".to_owned());
        assert!(!is_translated(&empty_forms, false));
        let mut whitespace_plural = plural_entry(vec!["un", " "]);
        whitespace_plural.msgstr_plural[1] = "\n\t".to_owned();
        assert!(!is_translated(&whitespace_plural, false));
    }

    #[test]
    fn needs_translation_skips_obsolete_and_header() {
        let mut obsolete = entry("a", None);
        obsolete.obsolete = true;
        assert!(!needs_translation(&obsolete, false));
        assert!(!needs_translation(&entry("", None), false));
        assert!(needs_translation(&entry("a", Some(" ")), false));
    }

    #[test]
    fn missing_entries_keeps_order_and_drops_obsolete() {
        let first = entry("first", Some("ok"));
        let second = entry("second", None);
        let third = entry("third", Some("   "));
        let fourth = entry("fourth", Some("ok"));
        let po = POFile::from(vec![&first, &second, &third, &fourth]);
        let missing = missing_entries(&po, false);
        let msgids: Vec<&str> = missing.iter().map(|e| e.msgid.as_str()).collect();
        assert_eq!(msgids, vec!["second", "third"]);
    }

    #[test]
    fn entry_index_and_lookup_with_msgctxt() {
        let mut context_a = entry("save", Some("x"));
        context_a.msgctxt = Some("menu".to_owned());
        let plain = entry("save", Some("x"));
        let po = POFile::from(vec![&plain, &context_a]);
        let index = entry_index(&po);
        assert_eq!(index.len(), 2);

        let mut lookup = entry("save", None);
        assert_eq!(find_index(&index, &lookup), Some(0));
        lookup.msgctxt = Some("menu".to_owned());
        assert_eq!(find_index(&index, &lookup), Some(1));
        lookup.msgctxt = Some("other".to_owned());
        assert_eq!(find_index(&index, &lookup), None);
        assert_eq!(entry_key(&lookup).1, "save");
    }

    #[test]
    fn apply_translation_copies_forms_and_clears_fuzzy() {
        let mut target = entry("a", None);
        target.flags = vec!["fuzzy".to_owned(), "python-format".to_owned()];
        let source = plural_entry(vec!["un", "une"]);
        apply_translation(&mut target, &source);
        assert_eq!(target.msgstr, None);
        assert_eq!(target.msgstr_plural.len(), 2);
        assert_eq!(target.flags, vec!["python-format".to_owned()]);

        let mut plain = entry("a", None);
        apply_translation(&mut plain, &entry("a", Some("trad")));
        assert_eq!(plain.msgstr.as_deref(), Some("trad"));
    }

    #[test]
    fn parse_and_write_round_trip_preserves_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("round.po");
        std::fs::write(
            &path,
            "# a comment header\nmsgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\nmsgid \"hi\"\nmsgstr \"salut\"\n",
        )
        .expect("write");
        let po = parse_po(&path).expect("parse");
        assert_eq!(
            po.metadata.get("Content-Type").map(String::as_str),
            Some("text/plain; charset=UTF-8")
        );
        let out_path = dir.path().join("out.po");
        write_po(&po, &out_path).expect("write");
        let reread = parse_po(&out_path).expect("reparse");
        assert_eq!(
            reread.metadata.get("Content-Type").map(String::as_str),
            Some("text/plain; charset=UTF-8")
        );
        assert_eq!(reread.entries.len(), 1);
        assert_eq!(reread.entries[0].msgstr.as_deref(), Some("salut"));
    }

    #[test]
    fn write_po_wraps_io_errors() {
        let one = entry("a", Some("b"));
        let po = POFile::from(vec![&one]);
        let path = std::path::Path::new("/definitely/missing/dir/out.po");
        let result = write_po(&po, path);
        assert!(
            matches!(result, Err(crate::error::Error::WriteFile { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn parse_po_wraps_syntax_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.po");
        std::fs::write(&path, "msgid \"oops\n").unwrap();
        assert!(matches!(
            parse_po(&path),
            Err(crate::error::Error::ParsePo { .. })
        ));
    }

    #[test]
    fn derived_file_copies_header_metadata_and_width() {
        let seed = entry("a", Some("b"));
        let mut source = POFile::from(vec![&seed]);
        source.header = Some("translator notes".to_owned());
        source
            .metadata
            .insert("Language".to_owned(), "fr".to_owned());
        source.metadata_is_fuzzy = true;
        source.options.wrapwidth = 42;
        let derived = derived_file(&source, vec![entry("c", None)]);
        assert_eq!(derived.header.as_deref(), Some("translator notes"));
        assert_eq!(
            derived.metadata.get("Language").map(String::as_str),
            Some("fr")
        );
        assert!(derived.metadata_is_fuzzy);
        assert_eq!(derived.options.wrapwidth, 42);
        assert_eq!(derived.entries.len(), 1);
    }
}
