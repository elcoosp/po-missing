use anyhow::{Context, Result};
use clap::Parser;
use ollama_rs::Ollama;
use ollama_rs::generation::completion::request::GenerationRequest;
use rspolib::{FileOptions, POFile, Save, pofile};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "po-translate",
    version = "0.2.0",
    author = "elcoosp",
    about = "Extract and translate missing translations from PO files",
    long_about = "Scans locale directories for PO files, extracts missing translations, and optionally uses Ollama to generate translations"
)]
struct Cli {
    /// Base directory containing locale folders
    #[arg(short, long, default_value = "frontend/src/locales")]
    base_path: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Ollama model to use for translations (e.g., "llama3.2", "mistral")
    #[arg(short, long)]
    model: Option<String>,

    /// Ollama API endpoint
    #[arg(long, default_value = "http://localhost:11434")]
    ollama_url: String,

    /// Source locale for reference (default: en)
    #[arg(short, long, default_value = "en")]
    source_locale: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("Scanning for locales in '{}' directory...", cli.base_path);
        if let Some(ref model) = cli.model {
            println!("Using Ollama model: {}", model);
        }
    }

    process_translations(&cli)
}

fn process_translations(cli: &Cli) -> Result<()> {
    let locales_dir = Path::new(&cli.base_path);
    if !locales_dir.exists() {
        anyhow::bail!("Directory '{}' does not exist", cli.base_path);
    }

    // Step 1: Extract missing translations for all locales
    if cli.verbose {
        println!("\n📋 Phase 1: Extracting missing translations...");
    }
    extract_missing_translations(&cli.base_path, cli.verbose)?;

    // Step 2: If model is specified, translate missing entries
    if let Some(ref model) = cli.model {
        if cli.verbose {
            println!("\n🤖 Phase 2: Translating with Ollama...");
        }
        translate_with_ollama(cli)?;
    }

    Ok(())
}

fn extract_missing_translations(base_path: &str, verbose: bool) -> Result<()> {
    let locales_dir = Path::new(base_path);
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
                    Ok(_) => processed += 1,
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
            "✅ Extraction complete: {} locales processed, {} errors",
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
        return Ok(());
    }

    // Merge back any completed translations from messages-missing.po
    if messages_missing_path.exists() {
        merge_back_translations(&messages_path, &messages_missing_path, locale, verbose)?;
    }

    // Extract current missing translations
    let main_po = pofile(messages_path.as_path())
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e))?;

    extract_current_missing(&main_po, &messages_missing_path, locale, verbose)?;

    Ok(())
}

fn merge_back_translations(
    messages_path: &PathBuf,
    messages_missing_path: &PathBuf,
    locale: &str,
    verbose: bool,
) -> Result<()> {
    let missing_po = pofile(messages_missing_path.as_path())?;
    let mut main_po = pofile(messages_path.as_path())
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", messages_path.display(), e))?;

    let mut updated_count = 0;

    for missing_entry in &missing_po.entries {
        if missing_entry.msgid.is_empty() {
            continue;
        }

        if let Some(msgstr) = &missing_entry.msgstr {
            if !msgstr.trim().is_empty() {
                if let Some(main_entry) = main_po
                    .entries
                    .iter_mut()
                    .find(|e| e.msgid == missing_entry.msgid && e.msgctxt == missing_entry.msgctxt)
                {
                    main_entry.msgstr = Some(msgstr.clone());
                    updated_count += 1;
                }
            }
        }
    }

    if updated_count > 0 {
        main_po.save(messages_path.as_os_str().to_str().unwrap());
        if verbose {
            println!(
                "  🔄 {}: {} translations merged back",
                locale, updated_count
            );
        }
        fs::remove_file(messages_missing_path)?;
    }

    Ok(())
}

fn extract_current_missing(
    main_po: &POFile,
    messages_missing_path: &PathBuf,
    locale: &str,
    verbose: bool,
) -> Result<()> {
    let empty_opts = FileOptions {
        path_or_content: "".into(),
        wrapwidth: 0,
        byte_content: None,
    };

    let mut new_missing_po = POFile::new(empty_opts);

    // Copy header
    if let Some(header_entry) = main_po.entries.iter().find(|e| e.msgid.is_empty()) {
        new_missing_po.entries.push(header_entry.clone());
    }

    let mut missing_count = 0;
    for entry in &main_po.entries {
        if entry.msgid.is_empty() {
            continue;
        }

        let is_missing = match &entry.msgstr {
            None => true,
            Some(s) => s.trim().is_empty(),
        };

        if is_missing {
            new_missing_po.entries.push(entry.clone());
            missing_count += 1;
        }
    }

    if missing_count > 0 {
        new_missing_po.save(messages_missing_path.as_os_str().to_str().unwrap());
        if verbose {
            println!(
                "  ✅ {}: {} missing translations extracted",
                locale, missing_count
            );
        }
    } else {
        if messages_missing_path.exists() {
            fs::remove_file(messages_missing_path)?;
        }
        if verbose {
            println!("  ✅ {}: no missing translations", locale);
        }
    }

    Ok(())
}

fn translate_with_ollama(cli: &Cli) -> Result<()> {
    let locales_dir = Path::new(&cli.base_path);
    let model = cli.model.as_ref().unwrap();

    for entry in fs::read_dir(locales_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let locale = match path.file_name().and_then(|name| name.to_str()) {
            Some(l) => l,
            None => continue,
        };

        // Skip source locale
        if locale == cli.source_locale {
            continue;
        }

        let messages_missing_path = path.join("messages-missing.po");
        if !messages_missing_path.exists() {
            continue;
        }

        if cli.verbose {
            println!("\n🌐 Translating {} locale...", locale);
        }

        translate_locale(
            &messages_missing_path,
            locale,
            model,
            &cli.ollama_url,
            cli.verbose,
        )?;

        // After translation, merge back the translations
        let messages_path = path.join("messages.po");
        merge_back_translations(&messages_path, &messages_missing_path, locale, cli.verbose)?;
    }

    Ok(())
}

fn translate_locale(
    messages_missing_path: &PathBuf,
    locale: &str,
    model: &str,
    ollama_url: &str,
    verbose: bool,
) -> Result<()> {
    let mut missing_po = pofile(messages_missing_path.as_path())?;

    let entries_needing_translation: Vec<(usize, String)> = missing_po
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.msgid.is_empty() == false
                && entry
                    .msgstr
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
        })
        .map(|(idx, entry)| (idx, entry.msgid.clone()))
        .collect();

    let total = entries_needing_translation.len();
    let mut translated_count = 0;

    // Initialize Ollama client
    let ollama = initialize_ollama(ollama_url)?;

    for (idx, msgid) in entries_needing_translation {
        if verbose {
            println!(
                "  Translating {}/{}: {}",
                translated_count + 1,
                total,
                msgid
            );
        }

        match call_ollama(&ollama, &msgid, locale, model) {
            Ok(translation) => {
                missing_po.entries[idx].msgstr = Some(translation);
                translated_count += 1;
            }
            Err(e) => {
                eprintln!("  ❌ Failed to translate '{}': {}", msgid, e);
                // Continue with next translation instead of failing completely
                continue;
            }
        }
    }

    if translated_count > 0 {
        missing_po.save(messages_missing_path.as_os_str().to_str().unwrap());
        if verbose {
            println!(
                "  ✅ {}: {} translations completed",
                locale, translated_count
            );
        }
    }

    Ok(())
}

fn initialize_ollama(ollama_url: &str) -> Result<Ollama> {
    // Parse the URL to extract host and port
    // Bug called `Result::unwrap()` on an `Err` value: RelativeUrlWithoutBase
    // let url = url::Url::parse(ollama_url).context("Failed to parse Ollama URL")?;
    // let host = ollama_url.split(":").collect::<Vec<_>>()[0];
    // let port = url.port().unwrap_or(11434);

    Ok(Ollama::default())
}

fn call_ollama(ollama: &Ollama, text: &str, target_locale: &str, model: &str) -> Result<String> {
    let locale_name = get_locale_name(target_locale);

    let prompt = format!(
        r#"Translate the following text to {locale_name} ({target_locale}).
Provide ONLY the translation, without any explanation, quotes, or additional text.
Do not include phrases like "Here is the translation:" or similar.

Text to translate: {text}

Translation:"#,
        locale_name = locale_name,
        target_locale = target_locale,
        text = text
    );

    // Use tokio runtime to call async ollama-rs
    let runtime = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;

    let response = runtime
        .block_on(async {
            ollama
                .generate(GenerationRequest::new(model.to_string(), prompt))
                .await
        })
        .context("Failed to call Ollama API")?;

    Ok(response.response.trim().to_string())
}

fn get_locale_name(locale: &str) -> &str {
    match locale {
        "fr" | "fr_FR" => "French",
        "es" | "es_ES" => "Spanish",
        "de" | "de_DE" => "German",
        "it" | "it_IT" => "Italian",
        "pt" | "pt_PT" => "Portuguese",
        "pt_BR" => "Brazilian Portuguese",
        "ja" | "ja_JP" => "Japanese",
        "zh" | "zh_CN" => "Chinese (Simplified)",
        "zh_TW" => "Chinese (Traditional)",
        "ko" | "ko_KR" => "Korean",
        "ru" | "ru_RU" => "Russian",
        "ar" | "ar_SA" => "Arabic",
        "nl" | "nl_NL" => "Dutch",
        "pl" | "pl_PL" => "Polish",
        "sv" | "sv_SE" => "Swedish",
        "da" | "da_DK" => "Danish",
        "no" | "nb_NO" => "Norwegian",
        "fi" | "fi_FI" => "Finnish",
        "tr" | "tr_TR" => "Turkish",
        "cs" | "cs_CZ" => "Czech",
        "hu" | "hu_HU" => "Hungarian",
        "ro" | "ro_RO" => "Romanian",
        "el" | "el_GR" => "Greek",
        "th" | "th_TH" => "Thai",
        "vi" | "vi_VN" => "Vietnamese",
        "id" | "id_ID" => "Indonesian",
        "uk" | "uk_UA" => "Ukrainian",
        _ => locale,
    }
}
