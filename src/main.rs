mod cli;
mod utils;

use clap::Parser;
use cli::Args;
use common::{BillInfo, Defaults, LoadEntry};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args = Args::parse();

    setup_log(&args);

    // Load defaults
    tracing::debug!("Loading defaults ...");
    let defaults: Defaults = serde_yaml::from_reader(fs::File::open(Path::new("defaults.yaml"))?)?;
    tracing::debug!("defaults={:#?}", defaults);

    let bill_input = args
        .bill_input
        .clone()
        .unwrap_or(defaults.bill_input.clone());
    let bill_input_excludes = args
        .bill_input_excludes
        .clone()
        .unwrap_or(defaults.bill_input_excludes.clone());
    let load_input = args
        .load_input
        .clone()
        .unwrap_or(defaults.load_input.clone());
    let load_input_excludes = args
        .load_input_excludes
        .clone()
        .unwrap_or(defaults.load_input_excludes.clone());
    let load_max_age_in_days = args
        .load_max_age_in_days
        .unwrap_or(defaults.load_max_age_in_days);
    let limit = args.limit.unwrap_or(defaults.limit);

    if !Path::new(&bill_input).exists() && !Path::new(&bill_input).is_file() {
        tracing::error!("Input directory '{}' does not exist.", bill_input);
        return Ok(());
    }
    if !Path::new(&load_input).exists() && !Path::new(&load_input).is_file() {
        tracing::error!("Input directory '{}' does not exist.", load_input);
        return Ok(());
    }

    match args.command {
        cli::Commands::Dump { dump_prefix } => {
            let prefix = dump_prefix.unwrap_or_else(|| defaults.dump_prefix.clone());
            return dump_bills(&bill_input, &bill_input_excludes, limit, &prefix);
        }
        cli::Commands::Process {
            use_cache,
            no_use_cache,
            workbook,
            overwrite,
            no_overwrite,
            append,
            no_append,
            upload,
            no_upload,
            gsheet_id,
            credentials,
            incremental,
            no_incremental,
        } => {
            let cache_enabled = use_cache || no_use_cache;
            let overwrite_enabled = overwrite || no_overwrite;
            let append_enabled = append || no_append;
            let upload_enabled = upload || no_upload;
            let incremental_enabled = incremental || no_incremental;

            tracing::info!("Processing bills ...");
            let bills_by_cups: HashMap<String, Vec<BillInfo>> = process_bills(
                &defaults,
                &bill_input,
                &bill_input_excludes,
                limit,
                cache_enabled,
            )?;

            tracing::info!("Processing loads ...");
            let loads = process_loads(
                &defaults,
                &load_input,
                &load_input_excludes,
                limit,
                load_max_age_in_days,
            )?;

            tracing::info!("Generating workbook ...");
            let workbook_path = &workbook.as_deref().unwrap_or(defaults.workbook.as_str());
            workbook::generate_workbook(
                &defaults,
                &bills_by_cups,
                &loads,
                workbook_path,
                overwrite_enabled,
                append_enabled,
            )?;

            if upload_enabled {
                let gid = gsheet_id.as_deref().unwrap_or(&defaults.gsheet_id);
                let cred = credentials.as_deref().unwrap_or(&defaults.credentials);
                tracing::info!("Uploading to GSheet '{}' ...", gid);
                gsheet::upload_report(
                    &defaults,
                    &bills_by_cups,
                    &loads,
                    gid,
                    cred,
                    incremental_enabled,
                )?;
            }
            tracing::info!("Done");
        }
    };
    Ok(())
}

fn setup_log(args: &Args) {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| args.log_level()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn dump_bills(
    bill_input: &str,
    bill_input_excludes: &[String],
    limit: i32,
    dump_prefix: &str,
) -> anyhow::Result<()> {
    tracing::info!("Dumping bills ...");
    let bill_files = list_files(bill_input, bill_input_excludes, "pdf")?;
    for (i, bill_file) in bill_files.iter().enumerate() {
        if limit > 0 && i >= limit as usize {
            break;
        }
        tracing::info!("{}", bill_file);
        let text = extractor::extract_text_from_pdf(Path::new(bill_file))?;
        let dump_name = format!(
            "{}{}.txt",
            dump_prefix,
            Path::new(&bill_file).file_stem().unwrap().to_string_lossy()
        );
        fs::write(dump_name, text)?;
    }
    tracing::info!("Dumped bills");
    Ok(())
}

fn process_bills(
    defaults: &Defaults,
    bill_input: &str,
    bill_input_excludes: &[String],
    limit: i32,
    cache_enabled: bool,
) -> anyhow::Result<HashMap<String, Vec<BillInfo>>> {
    let bills_to_process = list_files_with_limit(bill_input, bill_input_excludes, "pdf", limit)?;
    let mut bills_by_cups: HashMap<String, Vec<BillInfo>> = HashMap::new();
    let (conn_opt, known_bills) = if cache_enabled {
        match cache::load_cache(&defaults.cache_file) {
            Ok((c, k)) => (Some(c), k),
            Err(_) => (None, HashMap::new()),
        }
    } else {
        (None, HashMap::new())
    };

    for file in bills_to_process {
        let filename = Path::new(&file)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        if cache_enabled && known_bills.contains_key(&filename) {
            tracing::info!(
                "Using cache for bill '{}' ...",
                utils::readable_path(&file, 70)
            );
            if let Some(bill_info) = known_bills.get(&filename).cloned() {
                let cups = bill_info.cups.clone();
                bills_by_cups.entry(cups).or_default().push(bill_info);
            }
            continue;
        }

        tracing::info!(
            "Extracting information from bill '{}' ...",
            utils::readable_path(&file, 70)
        );
        let text = extractor::extract_text_from_pdf(Path::new(&file))?;
        let mut extracted = None;
        for (dispatcher_key, extractor_name) in &defaults.dispatchers {
            if text.contains(dispatcher_key) {
                tracing::debug!(
                    "Detected '{}' string. Using '{}' extractor function ...",
                    dispatcher_key,
                    extractor_name
                );
                extracted = match extractor_name.as_str() {
                    "plenitude" => extractor::extract_plenitude_bill(&file, &text).ok(),
                    "nufri" => extractor::extract_nufri_bill(&file, &text).ok(),
                    "endesa" => extractor::extract_endesa_bill(&file, &text).ok(),
                    "qener" => extractor::extract_qener_bill(&file, &text).ok(),
                    "te" => extractor::extract_te_bill(&file, &text).ok(),
                    _ => None,
                };
                break;
            }
        }

        tracing::debug!("Extracted={:?}", &extracted);
        if let Some(mut bill_info) = extracted {
            if bill_info.cups.is_empty() || bill_info.bill_id.is_empty() {
                tracing::error!("Missing mandatory keys for bill '{}': ...", file);
                continue;
            }
            if !bill_info.cups.ends_with("0F") {
                bill_info.cups.push_str("0F");
            }
            bill_info.file = Some(filename.clone());
            if cache_enabled && let Some(c) = &conn_opt {
                let _ = cache::mark_as_processed(c, &bill_info);
            }
            let cups = bill_info.cups.clone();
            bills_by_cups.entry(cups).or_default().push(bill_info);
        } else {
            tracing::error!("Skipping bill '{}': Unknown type", file);
        }
    }
    drop(conn_opt);

    for bills in bills_by_cups.values_mut() {
        bills.sort_by_key(|bill| bill.billing_period_start);
    }

    Ok(bills_by_cups)
}

fn process_loads(
    _defaults: &Defaults,
    load_input: &str,
    load_input_excludes: &[String],
    limit: i32,
    load_max_age_in_days: i32,
) -> anyhow::Result<Vec<LoadEntry>> {
    let mut loads = Vec::new();
    let load_files = list_files_with_limit(load_input, load_input_excludes, "csv", limit)?;
    let cutoff = chrono::Local::now().naive_local().date()
        - chrono::Duration::days(load_max_age_in_days as i64);
    for load_file in load_files {
        if let Ok(mut reader) = csv::ReaderBuilder::new()
            .delimiter(b';')
            .from_path(&load_file)
        {
            for record in reader.records().flatten() {
                let cups = record.get(0).unwrap_or("").trim().to_string();
                let fecha_text = record.get(1).unwrap_or("").trim();
                let fecha = match chrono::NaiveDate::parse_from_str(fecha_text, "%d/%m/%Y") {
                    Ok(fecha) => fecha,
                    Err(error) => {
                        tracing::warn!(
                            file = %load_file,
                            date = fecha_text,
                            %error,
                            "Skipping load row with invalid date"
                        );
                        continue;
                    }
                };
                if fecha < cutoff {
                    continue;
                }
                let hora = record
                    .get(2)
                    .unwrap_or("")
                    .trim()
                    .to_string()
                    .parse::<u8>()
                    .unwrap_or(0);
                let ae_kwh = record
                    .get(3)
                    .unwrap_or("")
                    .trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                loads.push(LoadEntry {
                    cups,
                    fecha,
                    hora,
                    ae_kwh,
                });
            }
        }
    }
    Ok(loads)
}

fn list_files_with_limit(
    input: &str,
    excludes: &[String],
    extension: &str,
    limit: i32,
) -> anyhow::Result<Vec<String>> {
    let mut files = list_files(input, excludes, extension)?;
    if files.is_empty() {
        tracing::error!(
            "No files found in '{}' with extention '{}'",
            input,
            extension
        );
        return Ok(vec![]);
    }
    tracing::info!(
        "Found {} files in '{}' with extention '{}'",
        files.len(),
        input,
        extension
    );

    if limit > 0 {
        files.truncate(limit as usize);
    }

    let mut files_to_process = files.clone();
    if limit > 0 {
        files_to_process.truncate(limit as usize);
        tracing::info!("Limiting files to process to {}", files_to_process.len());
    }
    files_to_process.sort();
    Ok(files_to_process)
}

fn list_files(input: &str, excludes: &[String], extension: &str) -> anyhow::Result<Vec<String>> {
    let mut files = Vec::new();
    let path = Path::new(input);
    if path.is_file() {
        files.push(input.to_string());
        return Ok(files);
    }
    let exclude_regexes: Vec<regex::Regex> = excludes
        .iter()
        .filter_map(|e| regex::Regex::new(e).ok())
        .collect();
    for entry in walkdir::WalkDir::new(input) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            if file_name.ends_with(&format!(".{}", extension)) {
                let full_path = entry.path().to_string_lossy().to_string();
                let mut excluded = false;
                for re in &exclude_regexes {
                    if re.is_match(&full_path) {
                        excluded = true;
                        break;
                    }
                }
                if !excluded {
                    files.push(full_path);
                }
            }
        }
    }
    files.sort();
    Ok(files)
}
