# PO Missing

A command-line tool (and library) for managing missing translations in PO
(Portable Object) files. It extracts untranslated strings into
`messages-missing.po` files that translators can fill in, and merges completed
translations back into the main `messages.po` files.

```text
$ po-missing --base-path frontend/src/locales --verbose
  ✅ fr: 66.7% translated, 0 merged, 0 orphaned, 2 missing extracted
  ✅ de: 100.0% translated, 0 merged, 0 orphaned, missing file removed
  ✅ en: 100.0% translated, 0 merged, 0 orphaned, missing file removed
3 locale(s) processed, 0 skipped, 0 merged, 2 missing, 0 error(s)
```

## Features

- 🔍 **Extract missing translations** — scan PO files and extract untranslated
  strings into `<name>-missing.po` files
- 🔄 **Merge completed translations** — merge translations from
  `<name>-missing.po` back into the main file and clean up
- 🌍 **Multi-locale support** — process every locale directory in one pass,
  optionally in parallel (`-j`)
- 🧩 **Full PO semantics** — plural forms (`msgstr[n]`), `msgctxt` context
  keys, fuzzy flags, and obsolete (`#~`) entries are all handled correctly
- 🧪 **Safe by default** — a `*-missing.po` file that cannot be parsed is
  reported as an error and never overwritten or deleted; `--dry-run` previews
  every change without touching disk
- 📊 **Statistics & CI integration** — per-locale translated counts and
  percentages, `--json` output, `--fail-on-missing` gate
- 🧾 **Header preservation** — comments, metadata and wrap width are carried
  over to generated files
- 🦀 **Embeddable** — the engine is a documented library with typed errors

## Installation

### From source

```bash
git clone https://github.com/elcoosp/po-missing
cd po-missing
cargo install --path .
```

### From crates.io

```bash
cargo install po-missing
```

## Usage

### Basic usage

```bash
po-missing
```

This scans the default directory (`frontend/src/locales`) for locale folders
and processes each `messages.po` file.

### Common options

```bash
po-missing --base-path ./locales           # custom directory
po-missing --filename django               # process django.po / django-missing.po
po-missing --dry-run --verbose             # preview without writing
po-missing --json                          # machine-readable summary
po-missing --fail-on-missing               # exit 2 while translations are missing
po-missing --include-fuzzy                 # accept fuzzy translations on merge
po-missing -j 4                            # process locales with 4 workers
```

### All options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--base-path <DIR>` | `-b` | `frontend/src/locales` | Directory containing one subdirectory per locale |
| `--filename <NAME>` | `-f` | `messages` | Base filename; processes `<NAME>.po` and `<NAME>-missing.po` |
| `--verbose` | `-v` | off | Print per-locale details |
| `--quiet` | `-q` | off | Print nothing (errors still go to stderr) |
| `--dry-run` | `-n` | off | Report intended changes without writing any file |
| `--json` | | off | Print a JSON summary instead of text |
| `--fail-on-missing` | | off | Exit with status 2 when missing translations remain |
| `--include-fuzzy` | `-i` | off | Treat fuzzy entries as translated (the fuzzy flag is cleared on merge) |
| `--jobs <N>` | `-j` | `0` | Parallel workers; `0` means one per CPU core |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | At least one locale failed (I/O error or unparseable PO file) |
| `2` | `--fail-on-missing` was requested and missing translations remain |

## How it works

### What counts as "missing"

An entry needs translation work when its `msgstr` (or **every** plural form
`msgstr[n]`) is empty or whitespace-only, or when it carries the `fuzzy`
flag. Obsolete (`#~`) entries and the file header are ignored: they are not
part of the translation workload.

### Merge phase

1. If `<name>-missing.po` exists, its fully translated entries are looked up
   in the main file by `(msgctxt, msgid)` and copied over, including plural
   forms. The `fuzzy` flag is cleared on merge: merging means accepting the
   translation.
2. Entries whose `msgid` no longer exists in the main file are counted as
   *orphaned* and dropped when the missing file is rewritten.
3. The main file is written only if at least one translation was merged.

### Extraction phase

4. The (possibly merged, in-memory) main file is scanned again for missing
   entries.
5. If any remain, a fresh `<name>-missing.po` is written with the original
   header, metadata and wrap width preserved.
6. If none remain, a stale `<name>-missing.po` is removed.

A `*-missing.po` file that cannot be parsed aborts processing of that locale
with an error instead of being silently overwritten — translator work is
never discarded. Use `--dry-run` to preview all of the above without writing
anything.

### Typical workflow

1. **Extract** — `po-missing --verbose` creates the missing files.
2. **Translate** — translators fill in the `messages-missing.po` files.
3. **Merge** — `po-missing --verbose` merges completed work and re-extracts
   whatever is still missing.
4. **Gate** — in CI, run `po-missing --fail-on-missing` to block releases
   with untranslated strings.

## JSON output

`--json` prints one summary object (compact, single line; pretty-printed
here for readability):

```json
{
  "results": [
    {
      "locale": "fr",
      "status": "processed",
      "total": 3,
      "translated": 2,
      "missing": 1,
      "percent": 66.66666666666666,
      "merged": 1,
      "orphaned": 0,
      "missing_file": "written"
    }
  ],
  "processed": 1,
  "skipped": 0,
  "failed": 0,
  "merged": 1,
  "missing": 1
}
```

`status` is `processed`, `skipped` (no `<name>.po` in the directory) or
`failed` (with an `error` message). `missing_file` is `written`, `removed`
or `none`.

## Project structure

Your locale directory should look like this:

```text
frontend/src/locales/
├── en/
│   ├── messages.po
│   └── messages-missing.po   (generated)
├── fr/
│   ├── messages.po
│   └── messages-missing.po   (generated)
└── de/
    ├── messages.po
    └── messages-missing.po   (generated)
```

## Library usage

The CLI is a thin layer over the `po_missing` library:

```rust
use po_missing::{run, Options};

let options = Options {
    base_path: "frontend/src/locales".into(),
    include_fuzzy: false,
    dry_run: false,
    ..Options::default()
};
let summary = run(&options)?;
println!("{} missing across {} locales", summary.missing, summary.processed);
for result in &summary.results {
    println!("{}: {:?}", result.locale, result.outcome);
}
# Ok::<(), po_missing::Error>(())
```

Errors are typed (`po_missing::Error`) with the offending path attached, so
library consumers can match on failures instead of parsing strings.

## Development

```bash
cargo test                      # unit + integration tests
cargo clippy --all-targets      # pedantic + nursery lints (configured in Cargo.toml)
cargo fmt --all --check         # formatting
cargo llvm-cov --fail-under-lines 100   # coverage gate
```

Lint configuration lives in `Cargo.toml` (`[lints]`): `clippy::pedantic`,
`clippy::nursery` and `missing_docs` are enforced with `-D warnings` in CI —
the codebase contains zero `#[allow]` attributes.

### CI/CD

- **CI** (`.github/workflows/ci.yml`): formatting, clippy (warnings as
  errors), tests on Linux/macOS/Windows, MSRV check (1.85), 100% coverage
  gate, and `cargo audit`.
- **Release** (`.github/workflows/release.yml`): pushing a `v*` tag generates
  the release notes with [git-cliff](https://git-cliff.org) (`cliff.toml`),
  builds binaries for Linux (x86_64 + aarch64), macOS (x86_64 + aarch64) and
  Windows with sha256 checksums, and publishes to crates.io when
  `CARGO_REGISTRY_TOKEN` is configured.

Conventional commits (`feat:`, `fix:`, `perf:`, …) drive the changelog.
Dependency and GitHub Actions updates are automated via Dependabot.

## Requirements

- Rust 1.85 or higher (to build from source)
- PO files compliant with the GNU gettext format

## License

MIT
