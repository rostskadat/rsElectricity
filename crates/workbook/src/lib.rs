use chrono::{Datelike, NaiveDate};
use common::{BillInfo, Defaults, LoadEntry};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use umya_spreadsheet::Worksheet;
use umya_spreadsheet::new_file;
use umya_spreadsheet::reader::xlsx::read;
use umya_spreadsheet::writer::xlsx::write;

/// Generate the workbook with bills and loads
pub fn generate_workbook(
    defaults: &Defaults,
    bills: &HashMap<String, Vec<BillInfo>>,
    loads: &[LoadEntry],
    workbook_path: &str,
    overwrite_enabled: bool,
    append_enabled: bool,
) -> anyhow::Result<()> {
    // Get sheet names and column labels
    let sheet_names = &defaults.sheet_names;
    let column_labels = &defaults.column_labels;

    // Create or load workbook
    let mut workbook = new_file();
    if Path::new(workbook_path).exists() && append_enabled {
        tracing::warn!(
            "Workbook '{}' exists: appending to existing values.",
            workbook_path
        );
        workbook = read(workbook_path)?;
    } else {
        workbook.remove_sheet(0)?;
    }

    // Generate loads sheet first
    generate_loads_sheet(&mut workbook, loads, append_enabled)?;

    let mut ordered_cups: Vec<&String> = sheet_names
        .keys()
        .filter(|cups| bills.contains_key(*cups))
        .collect();
    let mut unconfigured_cups: Vec<&String> = bills
        .keys()
        .filter(|cups| !sheet_names.contains_key(*cups))
        .collect();
    unconfigured_cups.sort();
    ordered_cups.extend(unconfigured_cups);

    // Generate sheets for each CUPS
    for cups in ordered_cups {
        let Some(bill_infos) = bills.get(cups) else {
            continue;
        };
        generate_cups_sheet(
            &mut workbook,
            sheet_names,
            column_labels,
            cups,
            bill_infos,
            append_enabled,
        )?;
    }

    if Path::new(workbook_path).exists() && !overwrite_enabled {
        tracing::warn!("Workbook '{}' exists: not overwriting.", workbook_path);
    } else {
        let start = Instant::now();
        write(&workbook, workbook_path)?;
        tracing::info!(
            "Workbook written to '{}' in {:?}",
            workbook_path,
            start.elapsed()
        );
    }
    Ok(())
}

/// Generate the loads sheet
fn generate_loads_sheet(
    workbook: &mut umya_spreadsheet::Workbook,
    loads: &[LoadEntry],
    append_enabled: bool,
) -> anyhow::Result<()> {
    let sheet_name = "Loads";
    tracing::info!("Creating sheet for Loads");
    if workbook.sheet_by_name_mut(sheet_name).is_err() {
        workbook.new_sheet(sheet_name)?;
    } else if !append_enabled && let Ok(sheet) = workbook.sheet_by_name_mut(sheet_name) {
        clear_sheet(sheet);
    }
    let loads_sheet = workbook.sheet_by_name_mut(sheet_name)?;
    loads_sheet
        .column_dimension_mut("B")
        .style_mut()
        .number_format_mut()
        .set_format_code("dd/mm/yyyy");
    // TODO: the column labels should be parametrized (from defauls.yaml)
    set_text_cell(loads_sheet, 0, 0, "CUPS")?;
    set_text_cell(loads_sheet, 0, 1, "Fecha")?;
    set_text_cell(loads_sheet, 0, 2, "Hora")?;
    set_text_cell(loads_sheet, 0, 3, "AE_kWh")?;
    let row = loads_sheet.highest_row() as usize;
    for (i, load) in loads.iter().enumerate() {
        set_text_cell(loads_sheet, (row + i).try_into()?, 0, &load.cups)?;
        set_date_cell(loads_sheet, (row + i).try_into()?, 1, &load.fecha)?;
        set_number_cell(loads_sheet, (row + i).try_into()?, 2, load.hora as f64)?;
        set_number_cell(loads_sheet, (row + i).try_into()?, 3, load.ae_kwh)?;
    }

    Ok(())
}

fn generate_cups_sheet(
    workbook: &mut umya_spreadsheet::Workbook,
    sheet_names: &IndexMap<String, String>,
    column_labels: &IndexMap<String, String>,
    cups: &str,
    bill_infos: &[BillInfo],
    append_enabled: bool,
) -> anyhow::Result<()> {
    let sheet_name = sheet_names
        .get(cups)
        .cloned()
        .unwrap_or_else(|| cups.to_owned());
    tracing::info!(
        "Creating sheet for CUPS: {}, sheet name: {}",
        cups,
        sheet_name
    );
    if workbook.sheet_by_name_mut(sheet_name.as_str()).is_err() {
        workbook.new_sheet(sheet_name.as_str())?;
    } else if !append_enabled && let Ok(sheet) = workbook.sheet_by_name_mut(sheet_name.as_str()) {
        clear_sheet(sheet);
    }
    let sheet = workbook.sheet_by_name_mut(sheet_name.as_str())?;
    let column_keys: Vec<String> = column_labels.keys().cloned().collect();
    for (i, column_key) in column_keys.iter().enumerate() {
        if let Some(label) = column_labels.get(column_key) {
            set_text_cell(sheet, 0, i as u32, label)?;
        } else {
            set_text_cell(sheet, 0, i as u32, column_key.as_str())?;
        }
    }

    for date_column in ["B", "C", "D"] {
        sheet
            .column_dimension_mut(date_column)
            .style_mut()
            .number_format_mut()
            .set_format_code("dd/mm/yyyy");
    }

    for (row, bill) in (sheet.highest_row()..).zip(bill_infos.iter()) {
        for (col, key) in column_keys.iter().enumerate() {
            let urow: u32 = row;
            let ucol: u32 = col as u32;
            match key.as_str() {
                "bill_id" => set_text_cell(sheet, urow, ucol, &bill.bill_id)?,
                "billing_date" => set_date_cell(sheet, urow, ucol, &bill.billing_date)?,
                "billing_period_start" => {
                    set_date_cell(sheet, urow, ucol, &bill.billing_period_start)?
                }
                "billing_period_end" => set_date_cell(sheet, urow, ucol, &bill.billing_period_end)?,
                "billed_power" => set_number_cell(sheet, urow, ucol, bill.billed_power)?,
                "billed_energy" => set_number_cell(sheet, urow, ucol, bill.billed_energy)?,
                "gross_amount" => {
                    set_number_cell(sheet, urow, ucol, bill.billed_power + bill.billed_energy)?
                }
                "billed_amount" => set_number_cell(sheet, urow, ucol, bill.billed_amount)?,
                "p1" => set_number_cell(sheet, urow, ucol, bill.p1)?,
                "p2" => set_number_cell(sheet, urow, ucol, bill.p2)?,
                "p3" => set_number_cell(sheet, urow, ucol, bill.p3)?,
                "p4" => set_number_cell(sheet, urow, ucol, bill.p4)?,
                "p5" => set_number_cell(sheet, urow, ucol, bill.p5)?,
                "p6" => set_number_cell(sheet, urow, ucol, bill.p6)?,
                "cp1" => set_number_cell(sheet, urow, ucol, bill.cp1)?,
                "cp2" => set_number_cell(sheet, urow, ucol, bill.cp2)?,
                "cp3" => set_number_cell(sheet, urow, ucol, bill.cp3)?,
                "cp4" => set_number_cell(sheet, urow, ucol, bill.cp4)?,
                "cp5" => set_number_cell(sheet, urow, ucol, bill.cp5)?,
                "cp6" => set_number_cell(sheet, urow, ucol, bill.cp6)?,
                "contract_type" => set_text_cell(sheet, urow, ucol, &bill.contract_type)?,
                "file" => set_text_cell(sheet, urow, ucol, bill.file.as_deref().unwrap_or(""))?,
                _ => {}
            }
        }
    }

    Ok(())
}

/// Convert 0-based index to spreadsheet index
fn spreadsheet_index(index: u32) -> anyhow::Result<u32> {
    index
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("worksheet coordinate exceeds supported limits"))
}

/// Set a text cell in the sheet
fn set_text_cell(sheet: &mut Worksheet, row: u32, col: u32, value: &str) -> anyhow::Result<()> {
    sheet
        .cell_mut((spreadsheet_index(col)?, spreadsheet_index(row)?))
        .set_value_string(value);
    Ok(())
}

/// Set a number cell in the sheet
fn set_number_cell(sheet: &mut Worksheet, row: u32, col: u32, value: f64) -> anyhow::Result<()> {
    sheet
        .cell_mut((spreadsheet_index(col)?, spreadsheet_index(row)?))
        .set_value_number(value);
    Ok(())
}

fn set_date_cell(
    sheet: &mut umya_spreadsheet::Worksheet,
    row: u32,
    col: u32,
    date: &NaiveDate,
) -> anyhow::Result<()> {
    let excel_date = umya_spreadsheet::helper::date::convert_date(
        date.year(),
        date.month() as i32,
        date.day() as i32,
        0,
        0,
        0,
    );
    sheet
        .cell_mut((spreadsheet_index(col)?, spreadsheet_index(row)?))
        .set_value_number(excel_date);
    Ok(())
}

/// Clear all cells in the sheet
fn clear_sheet(sheet: &mut umya_spreadsheet::Worksheet) {
    sheet.cells_mut().iter_mut().for_each(|c| {
        c.set_value("");
    });
}
