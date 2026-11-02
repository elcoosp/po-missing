//! Typed errors for the [`po_missing`](crate) library.
//!
//! Errors carry the path they relate to so callers (and the CLI) can render
//! precise messages without extra context plumbing.

use std::path::PathBuf;

use thiserror::Error;

use rspolib::errors::SyntaxError;

/// Result alias used throughout the library.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every error the library can surface.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The configured base path is not an existing directory.
    #[error("base path '{}' is not an existing directory", .path.display())]
    NotADirectory {
        /// The offending base path.
        path: PathBuf,
    },
    /// Reading a directory (or one of its entries) failed.
    #[error("failed to read directory '{}'", .path.display())]
    ReadDir {
        /// The directory that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Parsing a PO file failed.
    #[error("failed to parse PO file '{}'", .path.display())]
    ParsePo {
        /// The file that failed to parse.
        path: PathBuf,
        /// The underlying syntax error.
        source: SyntaxError,
    },
    /// Writing a PO file failed.
    #[error("failed to write file '{}'", .path.display())]
    WriteFile {
        /// The file that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Removing a PO file failed.
    #[error("failed to remove file '{}'", .path.display())]
    RemoveFile {
        /// The file that could not be removed.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Untranslated entries remain and `--fail-on-missing` was requested.
    #[error("{missing} missing translation(s) remain across {locales} locale(s)")]
    MissingTranslations {
        /// Total number of missing translations.
        missing: usize,
        /// Number of locales that still have missing translations.
        locales: usize,
    },
    /// A path is not representable as UTF-8, which the PO parser cannot open.
    #[error("path '{}' is not valid UTF-8", .path.display())]
    NonUtf8Path {
        /// The offending path.
        path: PathBuf,
    },
}

impl Error {
    /// Wrap a PO parse failure with the file path that caused it.
    #[must_use]
    pub fn parse_po(path: &std::path::Path, source: SyntaxError) -> Self {
        Self::ParsePo {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Wrap a write failure with the file path that caused it.
    #[must_use]
    pub fn write_file(path: &std::path::Path, source: std::io::Error) -> Self {
        Self::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use std::path::PathBuf;

    fn io_error() -> std::io::Error {
        std::io::Error::other("boom")
    }

    #[test]
    fn display_messages_mention_paths() {
        let cases: Vec<(Error, &str)> = vec![
            (
                Error::NotADirectory {
                    path: PathBuf::from("/tmp/x"),
                },
                "base path '/tmp/x' is not an existing directory",
            ),
            (
                Error::ReadDir {
                    path: PathBuf::from("/tmp/x"),
                    source: io_error(),
                },
                "failed to read directory '/tmp/x'",
            ),
            (
                Error::WriteFile {
                    path: PathBuf::from("/tmp/x.po"),
                    source: io_error(),
                },
                "failed to write file '/tmp/x.po'",
            ),
            (
                Error::RemoveFile {
                    path: PathBuf::from("/tmp/x.po"),
                    source: io_error(),
                },
                "failed to remove file '/tmp/x.po'",
            ),
            (
                Error::MissingTranslations {
                    missing: 3,
                    locales: 2,
                },
                "3 missing translation(s) remain across 2 locale(s)",
            ),
            (
                Error::NonUtf8Path {
                    path: PathBuf::from("/tmp/x"),
                },
                "path '/tmp/x' is not valid UTF-8",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn parse_po_error_keeps_source_and_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.po");
        std::fs::write(&path, "msgid \"unescaped\nmsgstr \"\"\n").expect("write");
        let source = rspolib::pofile(path.as_path()).expect_err("file must fail to parse");
        let error = Error::parse_po(&path, source);
        assert!(error.to_string().contains("bad.po"));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn write_file_error_keeps_source_and_path() {
        let error = Error::write_file(&PathBuf::from("/tmp/y.po"), io_error());
        assert!(error.to_string().contains("/tmp/y.po"));
        assert!(std::error::Error::source(&error).is_some());
    }
}
