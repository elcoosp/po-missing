use anyhow::{Context, Result};
use clap::Parser;
use rspolib::Save;
use rspolib::{FileOptions, POFile, pofile};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "po-missing",
    version = "0.1.0",
    author = "elcoosp",
    about = "Extract missing translations from PO files",
    long_about = "Scans locale directories for PO files and extracts missing translations into messages-missing.po files"
)]
struct Cli {
    /// Base directory containing locale folders
    #[arg(short, long, default_value = "frontend/src/locales")]
    base_path: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Scanning for locales in '{}' directory...", cli.base_path);
    }

    extract_missing_translations(&cli.base_path, cli.verbose)
}

fn extract_missing_translations(base_path: &str, verbose: bool) -> Result<()> {
    let locales_dir = Path::new(base_path);
    if !locales_dir.exists() {
        anyhow::bail!("Directory '{}' does not exist", base_path);
    }

    let mut processed = 0;
    let mut errors = 0;

    for entry in fs::read_dir(locales_dir)
        .with_context(|| format!("Failed to read directory '{}'", base_path))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(locale) = path.file_name().and_then(|name| name.to_str()) {
                match process_locale(base_path, locale, verbose) {
                    Ok(_) => {
                        processed += 1;
                    }
                    Err(e) => {
                        eprintln!("Error processing locale '{}': {}", locale, e);
                        errors += 1;
                    }
                }
            }
        }
    }

    if verbose {
        println!(
            "Processing complete: {} locales processed, {} errors",
            processed, errors
        );
    }

    if errors > 0 {
        anyhow::bail!("Completed with {} errors", errors);
    }

    Ok(())
}

fn process_locale(base_path: &str, locale: &str, verbose: bool) -> Result<()> {
    let messages_path = PathBuf::from(base_path).join(locale).join("messages.po");
    let messages_missing_path = PathBuf::from(base_path)
        .join(locale)
        .join("messages-missing.po");

    if !messages_path.exists() {
        return Ok(()); // Skip if no messages.po exists
    }

    // First, check if there's a messages-missing.po with non-empty translations to merge back
    if messages_missing_path.exists() {
        if let Ok(missing_po) = pofile(messages_missing_path.as_path()) {
            let mut main_po = pofile(messages_path.as_path()).map_err(|e| {
                anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e)
            })?;

            let mut updated_count = 0;
            let mut has_non_empty_translations = false;

            // Look for entries in messages-missing.po that have non-empty translations
            for missing_entry in &missing_po.entries {
                // Skip the header entry (empty msgid)
                if missing_entry.msgid.is_empty() {
                    continue;
                }

                // Check if this entry has a non-empty translation in messages-missing.po
                if let Some(msgstr) = &missing_entry.msgstr {
                    if !msgstr.trim().is_empty() {
                        has_non_empty_translations = true;

                        // Find the corresponding entry in the main PO file
                        if let Some(main_entry) = main_po.entries.iter_mut().find(|e| {
                            e.msgid == missing_entry.msgid && e.msgctxt == missing_entry.msgctxt
                        }) {
                            // Update the translation in the main PO file
                            main_entry.msgstr = Some(msgstr.clone());
                            updated_count += 1;
                        }
                    }
                }
            }

            // If we found non-empty translations, save the updated main PO file
            if has_non_empty_translations {
                main_po.save(messages_path.as_os_str().to_str().unwrap());

                if verbose {
                    println!(
                        "  🔄 {}: {} translations merged back from messages-missing.po",
                        locale, updated_count
                    );
                }

                // After merging, we can remove the messages-missing.po file
                fs::remove_file(&messages_missing_path)?;

                // Re-read the main PO file for the next steps since we just updated it
                let main_po = pofile(messages_path.as_path()).map_err(|e| {
                    anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e)
                })?;

                // Continue to extract current missing translations
                extract_current_missing(
                    &main_po,
                    &messages_path,
                    &messages_missing_path,
                    locale,
                    verbose,
                )?;
            } else {
                // No non-empty translations found, proceed with normal extraction
                extract_current_missing(
                    &main_po,
                    &messages_path,
                    &messages_missing_path,
                    locale,
                    verbose,
                )?;
            }
        } else {
            // If we can't read messages-missing.po, just proceed with normal extraction
            let main_po = pofile(messages_path.as_path()).map_err(|e| {
                anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e)
            })?;
            extract_current_missing(
                &main_po,
                &messages_path,
                &messages_missing_path,
                locale,
                verbose,
            )?;
        }
    } else {
        // No messages-missing.po exists, proceed with normal extraction
        let main_po = pofile(messages_path.as_path())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e))?;
        extract_current_missing(
            &main_po,
            &messages_path,
            &messages_missing_path,
            locale,
            verbose,
        )?;
    }

    Ok(())
}

fn extract_current_missing(
    main_po: &POFile,
    messages_path: &PathBuf,
    messages_missing_path: &PathBuf,
    locale: &str,
    verbose: bool,
) -> Result<()> {
    // Create options for new PO file
    let empty_opts = FileOptions {
        path_or_content: "".into(),
        wrapwidth: 0,
        byte_content: None,
    };

    // Create new missing PO file
    let mut new_missing_po = POFile::new(empty_opts.clone());

    // Copy header from main PO file if it exists
    if let Some(header_entry) = main_po.entries.iter().find(|e| e.msgid.is_empty()) {
        new_missing_po.entries.push(header_entry.clone());
    }

    // Find and add entries with missing translations
    let mut missing_count = 0;
    for entry in &main_po.entries {
        // Skip the header entry (empty msgid)
        if entry.msgid.is_empty() {
            continue;
        }

        // Check if translation is missing (None, empty, or only whitespace)
        let is_missing = match &entry.msgstr {
            None => true,
            Some(s) => s.trim().is_empty(),
        };

        if is_missing {
            new_missing_po.entries.push(entry.clone());
            missing_count += 1;
        }
    }

    // Only create messages-missing.po if there are actual missing translations
    if missing_count > 0 {
        new_missing_po.save(messages_missing_path.as_os_str().to_str().unwrap());
        if verbose {
            println!(
                "  ✅ {}: {} missing translations extracted",
                locale, missing_count
            );
        }
    } else {
        // Remove messages-missing.po if it exists and there are no missing translations
        if messages_missing_path.exists() {
            fs::remove_file(&messages_missing_path)?;
        }
        if verbose {
            println!("  ✅ {}: no missing translations", locale);
        }
    }

    Ok(())
}
