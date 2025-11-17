# PO Missing

A command-line tool for managing missing translations in PO (Portable Object) files. This tool helps extract untranslated strings and can merge completed translations back into the main translation files.

## Features

- 🔍 **Extract Missing Translations**: Scan PO files and extract untranslated strings into separate `messages-missing.po` files
- 🔄 **Merge Completed Translations**: Automatically merge completed translations from `messages-missing.po` back to the main `messages.po` file
- 🌍 **Multi-locale Support**: Process multiple locale directories simultaneously
- 📊 **Progress Reporting**: Verbose mode provides detailed processing information
- 🧹 **Automatic Cleanup**: Remove empty `messages-missing.po` files when no translations are missing

## Installation

### From Source

```bash
git clone <repository-url>
cd po-missing
cargo install --path .
```

### From Crates.io

```bash
cargo install po-missing
```

## Usage

### Basic Usage

```bash
po-missing
```

This will scan the default directory (`frontend/src/locales`) for locale folders and process each `messages.po` file.

### With Custom Directory

```bash
po-missing --base-path /path/to/your/locales
```

### Verbose Output

```bash
po-missing --verbose
# or
po-missing -v -b /custom/path
```

## How It Works

### Extraction Phase
1. Scans the specified base directory for locale folders (e.g., `en`, `fr`, `de`)
2. For each locale, reads the `messages.po` file
3. Identifies entries with missing translations (empty or whitespace-only `msgstr`)
4. Creates a `messages-missing.po` file containing only the untranslated strings
5. Preserves the PO file header from the original file

### Merge Phase
1. Checks if `messages-missing.po` exists and contains completed translations
2. Merges non-empty translations from `messages-missing.po` back to the main `messages.po`
3. Removes the `messages-missing.po` file after successful merge
4. Re-extracts any remaining missing translations

## Typical Workflow

1. **Initial Extraction**:
   ```bash
   po-missing --verbose
   ```
   This creates `messages-missing.po` files with all untranslated strings.

2. **Translation Work**:
   Translators work on the `messages-missing.po` files, filling in the missing translations.

3. **Merge Completed Work**:
   ```bash
   po-missing --verbose
   ```
   The tool automatically detects completed translations in `messages-missing.po` and merges them back to the main `messages.po`.

4. **Repeat**:
   The process repeats as new strings are added to the codebase.

## Project Structure

Your locale directory should be structured like this:

```
frontend/src/locales/
├── en/
│   ├── messages.po
│   └── messages-missing.po (generated)
├── fr/
│   ├── messages.po
│   └── messages-missing.po (generated)
└── de/
    ├── messages.po
    └── messages-missing.po (generated)
```

## Command Line Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--base-path` | `-b` | `frontend/src/locales` | Base directory containing locale folders |
| `--verbose` | `-v` | `false` | Enable verbose output |

## Example Output

With `--verbose` flag:

```
Scanning for locales in 'frontend/src/locales' directory...
  🔄 fr: 5 translations merged back from messages-missing.po
  ✅ en: 12 missing translations extracted
  ✅ de: no missing translations
Processing complete: 3 locales processed, 0 errors
```

## Integration with CI/CD

You can integrate this tool into your development workflow:

```bash
# Extract missing translations before starting translation work
po-missing --base-path ./locales

# After translations are done, merge them back
po-missing --base-path ./locales
```

## Requirements

- Rust 1.91 or higher
- PO files compliant with GNU gettext format

## License

MIT
