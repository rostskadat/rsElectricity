use anyhow::{Context, bail};
use common::{BillInfo, Defaults, LoadEntry};
use google_sheets4::Sheets;
use google_sheets4::api::{
    BatchUpdateSpreadsheetRequest, CellData, ExtendedValue, GridProperties, GridRange, Request,
    RowData, SheetProperties, UpdateCellsRequest, UpdateSheetPropertiesRequest,
};
use indexmap::IndexMap;
use std::collections::HashMap;

/// Appends Loads and mapped bill tab values using service-account.json.
pub fn upload_report(
    defaults: &Defaults,
    bills_by_cups: &HashMap<String, Vec<BillInfo>>,
    loads: &[LoadEntry],
    gsheet_id: &str,
    credentials: &str,
    incremental_enabled: bool,
) -> anyhow::Result<()> {
    let sheet_names_by_cups = get_sheet_names(&defaults.sheet_names, bills_by_cups);
    let column_labels = &defaults.column_labels;

    if gsheet_id.trim().is_empty() {
        bail!("Google spreadsheet ID must not be empty");
    }

    let runtime = tokio::runtime::Runtime::new().context("creating Google Sheets runtime")?;
    runtime.block_on(async {
        let hub = authenticated_sheets(credentials).await?;
        let properties = get_sheetproperties(&hub, gsheet_id).await?;

        let sheet_names = sheet_names_by_cups.values().cloned().collect::<Vec<_>>();
        // Init the list of bill ids found in GSheet
        let mut bill_ids_by_sheet_name = sheet_names
            .iter()
            .map(|sheet_name| (sheet_name.clone(), Vec::new()))
            .collect::<HashMap<_, _>>();
        if incremental_enabled {
            tracing::info!("Incremental enabled");
            bill_ids_by_sheet_name =
                get_ids_in_sheets(&hub, &properties, gsheet_id, &sheet_names).await?;
        } else {
            tracing::info!("Incremental disabled");
        }
        let row_count_by_sheet_name = bill_ids_by_sheet_name
            .iter()
            .map(|(sheet_name, values)| (sheet_name.clone(), values.len()))
            .collect::<HashMap<_, _>>();
        let mut sheets = get_bills_row_data(
            &sheet_names_by_cups,
            column_labels,
            bills_by_cups,
            &bill_ids_by_sheet_name,
        )?;
        let load_rows = get_load_row_data(loads)?;
        sheets.push(("Loads".to_owned(), load_rows));
        if sheets.len() == 1 {
            tracing::info!("Updating 'Loads' sheet ...");
        } else {
            tracing::info!("Updating {} sheets in parallel ...", sheets.len());
        }
        let updates = sheets.into_iter().map(|(sheet_name, row_data)| {
            let start_row_index = row_count_by_sheet_name
                .get(&sheet_name)
                .copied()
                .unwrap_or(0);
            update_sheet(
                &hub,
                &properties,
                gsheet_id,
                sheet_name,
                row_data,
                start_row_index,
            )
        });
        for result in futures_util::future::join_all(updates).await {
            result?;
        }
        Ok(())
    })
}

/// Get existing values in column A for given sheets in the spreadsheet
async fn get_ids_in_sheets(
    hub: &Sheets<SheetsConnector>,
    properties: &[SheetProperties],
    gsheet_id: &str,
    sheet_names: &[String],
) -> anyhow::Result<HashMap<String, Vec<String>>> {
    // Filter sheets that exist in the spreadsheet (i.e. for which a SheetProperties exists)
    let existing_sheets = sheet_names
        .iter()
        .filter(|sheet_name| {
            properties
                .iter()
                .any(|sheet| sheet.title.as_deref() == Some(sheet_name.as_str()))
        })
        .collect::<Vec<_>>();

    //
    let requests = existing_sheets.into_iter().map(|sheet_name| async move {
        let range = format!("'{}'!A:A", sheet_name.replace('\'', "''"));
        let (_, values) = hub
            .spreadsheets()
            .values_get(gsheet_id, &range)
            .doit()
            .await
            .with_context(|| format!("reading existing rows from {sheet_name:?} column A"))?;
        let values = values
            .values
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .filter_map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_number().map(ToString::to_string))
            })
            .collect::<Vec<_>>();
        Ok::<_, anyhow::Error>((sheet_name.clone(), values))
    });

    let mut existing_values = HashMap::new();
    for result in futures_util::future::join_all(requests).await {
        let (sheet_name, values) = result?;
        existing_values.insert(sheet_name, values);
    }
    Ok(existing_values)
}

/// Get sheet names corresponding to the CUPS actually found in the list of bills
/// Returns a map of CUPS to sheet names
fn get_sheet_names(
    default_sheet_names: &IndexMap<String, String>,
    bills_by_cups: &HashMap<String, Vec<BillInfo>>,
) -> IndexMap<String, String> {
    default_sheet_names
        .iter()
        .filter(|(cups, _)| bills_by_cups.contains_key(*cups))
        .map(|(cups, sheet_name)| (cups.clone(), sheet_name.clone()))
        .collect()
}

type SheetsConnector =
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>;

async fn authenticated_sheets(credentials: &str) -> anyhow::Result<Sheets<SheetsConnector>> {
    let key = yup_oauth2::read_service_account_key(credentials)
        .await
        .context("reading service-account.json from the current directory")?;
    let auth = yup_oauth2::ServiceAccountAuthenticator::builder(key)
        .build()
        .await
        .context("creating Google service account authenticator")?;
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .context("loading HTTPS trust roots")?
        .https_only()
        .enable_http2()
        .build();
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(connector);
    Ok(Sheets::new(client, auth))
}

async fn get_sheetproperties(
    hub: &Sheets<SheetsConnector>,
    gsheet_id: &str,
) -> anyhow::Result<Vec<SheetProperties>> {
    let (_, spreadsheet) = hub
        .spreadsheets()
        .get(gsheet_id)
        .param("fields", "sheets(properties)")
        .doit()
        .await
        .context("reading spreadsheet; share it with the service account as an editor")?;
    Ok(spreadsheet
        .sheets
        .unwrap_or_default()
        .into_iter()
        .filter_map(|sheet| sheet.properties)
        .collect())
}

async fn update_sheet(
    hub: &Sheets<SheetsConnector>,
    properties: &[SheetProperties],
    gsheet_id: &str,
    sheet_name: String,
    row_data: Vec<RowData>,
    start_row_index: usize,
) -> anyhow::Result<()> {
    if row_data.is_empty() {
        return Ok(());
    }
    let request = report_request(properties, &sheet_name, row_data, start_row_index)?;
    hub.spreadsheets()
        .batch_update(request, gsheet_id)
        .doit()
        .await
        .context(format!("replacing {sheet_name:?} tab values"))?;
    tracing::info!("Updated '{}' sheet", sheet_name);
    Ok(())
}

/// Creates a text cell.
fn text_cell(value: &str) -> CellData {
    CellData {
        user_entered_value: Some(ExtendedValue {
            string_value: Some(value.to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Creates a number cell.
fn number_cell(value: f64) -> CellData {
    CellData {
        user_entered_value: Some(ExtendedValue {
            number_value: Some(value),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Prepares rows for the bill tabs.
fn get_bills_row_data(
    sheet_names: &IndexMap<String, String>,
    column_labels: &IndexMap<String, String>,
    bills_by_cups: &HashMap<String, Vec<BillInfo>>,
    bill_ids_by_sheet: &HashMap<String, Vec<String>>,
) -> anyhow::Result<Vec<(String, Vec<RowData>)>> {
    tracing::debug!("Preparing rows for bill sheets ...");
    let mut targets = Vec::new();
    for (cups, bill_infos) in bills_by_cups {
        let sheet_name = sheet_names
            .get(cups)
            .with_context(|| format!("no defaults.sheet_names mapping for CUPS {cups}"))?;
        let known_bill_ids = bill_ids_by_sheet
            .get(sheet_name)
            .with_context(|| format!("No bill ids found in sheet {sheet_name}"))?;
        let mut rows = bill_rows(sheet_name, column_labels, bill_infos, known_bill_ids)?;
        if rows.is_empty() {
            continue;
        } else if known_bill_ids.is_empty() {
            rows.insert(
                0,
                RowData {
                    values: Some(column_labels.values().map(|l| text_cell(l)).collect()),
                },
            );
        }
        targets.push((sheet_name.clone(), rows));
    }
    Ok(targets)
}

/// Prepares rows for the Loads sheet.
fn get_load_row_data(loads: &[LoadEntry]) -> anyhow::Result<Vec<RowData>> {
    let data_rows = loads
        .iter()
        .map(|entry| {
            if !entry.ae_kwh.is_finite() {
                bail!(
                    "non-finite AE_kWh for CUPS {} on {}",
                    entry.cups,
                    entry.fecha
                );
            }
            Ok(RowData {
                values: Some(vec![
                    text_cell(&entry.cups),
                    text_cell(&entry.fecha.format("%d/%m/%Y").to_string()),
                    number_cell(f64::from(entry.hora)),
                    number_cell(entry.ae_kwh),
                ]),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut rows = vec![RowData {
        values: Some(["CUPS", "Fecha", "Hora", "AE_kWh"].map(text_cell).to_vec()),
    }];
    rows.extend(data_rows);
    Ok(rows)
}

/// Prepares rows for a bill sheet.
fn bill_rows(
    sheet_name: &str,
    column_labels: &IndexMap<String, String>,
    bills: &[BillInfo],
    known_bill_ids: &[String],
) -> anyhow::Result<Vec<RowData>> {
    if column_labels.is_empty() {
        bail!("defaults.column_labels must contain bill columns");
    }
    let mut row_data = vec![];

    // Known bills are first filtered out, and then the resulting
    // list sorted by start date of the billing period.
    let mut sorted_bills = bills
        .iter()
        .filter(|bill| !known_bill_ids.contains(&bill.bill_id))
        .collect::<Vec<_>>();
    sorted_bills.sort_by_key(|bill| bill.billing_period_start);
    for bill in sorted_bills {
        // Prepare row for this bill.
        let cells = column_labels
            .keys()
            .map(|key| bill_cell(bill, key))
            .collect::<anyhow::Result<Vec<_>>>()?;
        row_data.push(RowData {
            values: Some(cells),
        });
    }
    if row_data.is_empty() {
        tracing::info!("No new bills detected for {sheet_name:?}");
    } else if known_bill_ids.is_empty() {
        tracing::info!("Adding {} bills for {sheet_name:?}", row_data.len(),);
    } else {
        tracing::info!(
            "Adding {} new bills out of {} for {sheet_name:?}",
            row_data.len(),
            bills.len()
        );
    }
    Ok(row_data)
}

/// Prepares a cell for a bill column.
fn bill_cell(bill: &BillInfo, key: &str) -> anyhow::Result<CellData> {
    let text = match key {
        "bill_id" => Some(bill.bill_id.as_str()),
        "billing_date" => {
            return Ok(text_cell(&bill.billing_date.format("%d/%m/%Y").to_string()));
        }
        "billing_period_start" => {
            return Ok(text_cell(
                &bill.billing_period_start.format("%d/%m/%Y").to_string(),
            ));
        }
        "billing_period_end" => {
            return Ok(text_cell(
                &bill.billing_period_end.format("%d/%m/%Y").to_string(),
            ));
        }
        "cups" => Some(bill.cups.as_str()),
        "contract_type" => Some(bill.contract_type.as_str()),
        "file" => Some(bill.file.as_deref().unwrap_or("")),
        _ => None,
    };
    if let Some(text) = text {
        return Ok(text_cell(text));
    }
    let number = match key {
        "billed_power" => bill.billed_power,
        "billed_energy" => bill.billed_energy,
        "gross_amount" => bill.billed_power + bill.billed_energy,
        "billed_amount" => bill.billed_amount,
        "p1" => bill.p1,
        "p2" => bill.p2,
        "p3" => bill.p3,
        "p4" => bill.p4,
        "p5" => bill.p5,
        "p6" => bill.p6,
        "cp1" => bill.cp1,
        "cp2" => bill.cp2,
        "cp3" => bill.cp3,
        "cp4" => bill.cp4,
        "cp5" => bill.cp5,
        "cp6" => bill.cp6,
        _ => bail!("unsupported bill column {key:?} in defaults.column_labels"),
    };
    if !number.is_finite() {
        bail!("non-finite {key} for bill {}", bill.bill_id);
    }
    Ok(number_cell(number))
}

fn report_request(
    properties: &[SheetProperties],
    sheet_name: &str,
    rows: Vec<RowData>,
    start_row_index: usize,
) -> anyhow::Result<BatchUpdateSpreadsheetRequest> {
    let sheet = properties
        .iter()
        .find(|sheet| sheet.title.as_deref() == Some(sheet_name))
        .with_context(|| format!("spreadsheet has no tab named {sheet_name:?}"))?;
    let replacement = replacement_request(sheet.clone(), rows, start_row_index)?;
    Ok(BatchUpdateSpreadsheetRequest {
        requests: replacement.requests,
        ..Default::default()
    })
}

fn replacement_request(
    properties: SheetProperties,
    rows: Vec<RowData>,
    start_row_index: usize,
) -> anyhow::Result<BatchUpdateSpreadsheetRequest> {
    let sheet_id = properties.sheet_id.context("target tab has no sheet ID")?;
    let grid = properties
        .grid_properties
        .context("target must be a grid tab")?;
    let row_count = i32::try_from(rows.len()).context("too many rows for Google Sheets")?;
    let start_row_index =
        i32::try_from(start_row_index).context("sheet has too many existing rows")?;
    let end_row_index = start_row_index
        .checked_add(row_count)
        .context("appended row range exceeds Google Sheets limits")?;
    let width = rows
        .iter()
        .filter_map(|row| row.values.as_ref().map(Vec::len))
        .max()
        .unwrap_or(0);
    let column_count = i32::try_from(width).context("too many columns for Google Sheets")?;
    Ok(BatchUpdateSpreadsheetRequest {
        requests: Some(vec![
            Request {
                update_sheet_properties: Some(UpdateSheetPropertiesRequest {
                    properties: Some(SheetProperties {
                        sheet_id: Some(sheet_id),
                        grid_properties: Some(GridProperties {
                            row_count: Some(end_row_index.max(grid.row_count.unwrap_or_default())),
                            column_count: Some(
                                column_count.max(grid.column_count.unwrap_or_default()),
                            ),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    fields: Some("gridProperties.rowCount,gridProperties.columnCount".parse()?),
                }),
                ..Default::default()
            },
            Request {
                update_cells: Some(UpdateCellsRequest {
                    range: Some(GridRange {
                        sheet_id: Some(sheet_id),
                        start_row_index: Some(start_row_index),
                        end_row_index: Some(end_row_index),
                        start_column_index: Some(0),
                        end_column_index: Some(column_count),
                    }),
                    rows: Some(rows),
                    fields: Some("userEnteredValue".parse()?),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ]),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn bill(id: &str, start: &str) -> BillInfo {
        BillInfo {
            bill_id: id.to_owned(),
            billing_date: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
            billing_period_start: NaiveDate::parse_from_str(start, "%d/%m/%Y").unwrap(),
            billing_period_end: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
            cups: "cups".to_owned(),
            contract_type: "2.0TD".to_owned(),
            billed_amount: 30.0,
            billed_power: 10.0,
            billed_energy: 15.0,
            p1: 1.25,
            p2: 0.0,
            p3: 0.0,
            p4: 0.0,
            p5: 0.0,
            p6: 0.0,
            cp1: 0.0,
            cp2: 0.0,
            cp3: 0.0,
            cp4: 0.0,
            cp5: 0.0,
            cp6: 0.0,
            file: None,
            extra: HashMap::new(),
        }
    }

    fn sheet(name: &str, id: i32) -> SheetProperties {
        SheetProperties {
            title: Some(name.to_owned()),
            sheet_id: Some(id),
            grid_properties: Some(GridProperties {
                row_count: Some(100),
                column_count: Some(2),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn bills_follow_configured_columns_and_chronological_order() {
        let labels = IndexMap::from([
            ("gross_amount".to_owned(), "Gross".to_owned()),
            ("bill_id".to_owned(), "Invoice".to_owned()),
            ("p1".to_owned(), "Energy".to_owned()),
            ("cp1".to_owned(), "Power".to_owned()),
            ("file".to_owned(), "File".to_owned()),
        ]);
        let known_bill_ids = HashSet::new();
        let mut rows = bill_rows(
            "Bills",
            &labels,
            &[bill("later", "01/10/2026"), bill("=literal", "02/09/2026")],
            &known_bill_ids,
        )
        .unwrap();
        rows.insert(
            0,
            RowData {
                values: Some(labels.values().map(|label| text_cell(label)).collect()),
            },
        );
        let cells = rows[0].values.as_ref().unwrap();
        assert_eq!(
            cells[0]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("Gross")
        );
        assert_eq!(
            cells[1]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("Invoice")
        );
        let cells = rows[1].values.as_ref().unwrap();
        assert_eq!(
            cells[0].user_entered_value.as_ref().unwrap().number_value,
            Some(25.0)
        );
        assert_eq!(
            cells[1]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("=literal")
        );
        assert!(
            cells[1]
                .user_entered_value
                .as_ref()
                .unwrap()
                .formula_value
                .is_none()
        );
        assert_eq!(
            cells[2].user_entered_value.as_ref().unwrap().number_value,
            Some(1.25)
        );
        assert_eq!(
            cells[3].user_entered_value.as_ref().unwrap().number_value,
            Some(0.0)
        );
        assert_eq!(
            cells[4]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("")
        );
        let request = replacement_request(sheet("Bills", 8), rows, 99).unwrap();
        let requests = request.requests.unwrap();
        let grid = requests[0]
            .update_sheet_properties
            .as_ref()
            .unwrap()
            .properties
            .as_ref()
            .unwrap()
            .grid_properties
            .as_ref()
            .unwrap();
        assert_eq!(grid.column_count, Some(5));
        assert_eq!(grid.row_count, Some(102));
        let range = requests[1]
            .update_cells
            .as_ref()
            .unwrap()
            .range
            .as_ref()
            .unwrap();
        assert_eq!(range.start_row_index, Some(99));
        assert_eq!(range.end_row_index, Some(102));
        assert_eq!(range.start_column_index, Some(0));
        assert_eq!(range.end_column_index, Some(5));
    }

    #[test]
    fn bill_rows_skip_known_ids_in_incremental_mode() {
        let labels = IndexMap::from([("bill_id".to_owned(), "Invoice".to_owned())]);
        let known_bill_ids = HashSet::from(["existing".to_owned()]);
        let rows = bill_rows(
            "Bills",
            &labels,
            &[bill("existing", "01/09/2026"), bill("new", "01/10/2026")],
            &known_bill_ids,
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .values
                .as_ref()
                .and_then(|values| values.first())
                .and_then(|cell| cell.user_entered_value.as_ref())
                .and_then(|value| value.string_value.as_deref()),
            Some("new")
        );
    }

    #[test]
    fn sheet_names_for_bills_excludes_cups_without_bills() {
        let names = IndexMap::from([
            ("cups-a".to_owned(), "Office".to_owned()),
            ("cups-b".to_owned(), "Unused".to_owned()),
        ]);
        let bills = HashMap::from([("cups-a".to_owned(), Vec::new())]);

        assert_eq!(
            get_sheet_names(&names, &bills),
            IndexMap::from([("cups-a".to_owned(), "Office".to_owned())])
        );
    }

    #[test]
    fn report_updates_loads_and_mapped_tabs_in_separate_batches() {
        let bills = HashMap::from([
            ("cups-b".to_owned(), vec![bill("b", "01/10/2026")]),
            ("cups-a".to_owned(), vec![bill("a", "01/10/2026")]),
        ]);
        let names = IndexMap::from([
            ("cups-a".to_owned(), "Office".to_owned()),
            ("cups-b".to_owned(), "Home".to_owned()),
        ]);
        let labels = IndexMap::from([("bill_id".to_owned(), "Invoice".to_owned())]);
        let known_bill_ids = HashSet::new();
        let existing_row_counts = HashMap::new();
        let bill_targets = get_bills_row_data(
            &names,
            &labels,
            &bills,
            &known_bill_ids,
            &existing_row_counts,
        )
        .unwrap();
        assert_eq!(
            bill_targets
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Office", "Home"]
        );
        let properties = [
            sheet("Unrelated", 99),
            sheet("Home", 3),
            sheet("Loads", 1),
            sheet("Office", 2),
        ];
        let loads_request =
            report_request(&properties, "Loads", get_load_row_data(&[]).unwrap(), 0)
                .unwrap()
                .requests
                .unwrap();
        let bill_requests = bill_targets
            .into_iter()
            .map(|(sheet_name, rows)| {
                report_request(&properties, &sheet_name, rows, 0)
                    .unwrap()
                    .requests
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(loads_request.len(), 2);
        assert_eq!(bill_requests.len(), 2);
        let loads_ids = loads_request
            .iter()
            .filter_map(|request| request.update_cells.as_ref())
            .map(|update| update.range.as_ref().unwrap().sheet_id.unwrap())
            .collect::<Vec<_>>();
        let bill_ids = bill_requests
            .iter()
            .map(|requests| {
                requests
                    .iter()
                    .filter_map(|request| request.update_cells.as_ref())
                    .map(|update| update.range.as_ref().unwrap().sheet_id.unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(loads_ids, [1]);
        assert_eq!(bill_ids, [vec![2], vec![3]]);
    }

    #[test]
    fn invalid_bill_configuration_and_targets_are_rejected() {
        let bills = HashMap::from([("cups".to_owned(), vec![bill("id", "01/10/2026")])]);
        let labels = IndexMap::from([("bill_id".to_owned(), "Invoice".to_owned())]);
        let known_bill_ids = HashSet::new();
        let existing_row_counts = HashMap::new();
        assert!(
            get_bills_row_data(
                &IndexMap::new(),
                &labels,
                &bills,
                &known_bill_ids,
                &existing_row_counts
            )
            .is_err()
        );
        let names = IndexMap::from([("cups".to_owned(), "Loads".to_owned())]);
        assert!(
            get_bills_row_data(
                &names,
                &IndexMap::new(),
                &bills,
                &known_bill_ids,
                &existing_row_counts
            )
            .is_err()
        );
        assert!(report_request(&[sheet("Loads", 1)], "Missing", Vec::new(), 0).is_err());
        let mut invalid = bill("id", "01/10/2026");
        invalid.billed_amount = f64::INFINITY;
        assert!(bill_cell(&invalid, "billed_amount").is_err());
        assert!(bill_cell(&invalid, "unknown").is_err());
    }

    #[test]
    fn https_connector_selects_crypto_provider_without_panicking() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let _connector = hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .unwrap()
                .https_only()
                .enable_http2()
                .build();
        });
    }

    #[test]
    fn empty_report_still_has_headers() {
        let rows = get_load_row_data(&[]).unwrap();
        assert_eq!(rows.len(), 1);
        let cells = rows[0].values.as_ref().unwrap();
        let headers: Vec<_> = cells
            .iter()
            .map(|cell| {
                cell.user_entered_value
                    .as_ref()
                    .unwrap()
                    .string_value
                    .as_deref()
                    .unwrap()
            })
            .collect();
        assert_eq!(headers, ["CUPS", "Fecha", "Hora", "AE_kWh"]);
    }

    #[test]
    fn loads_preserve_order_and_literal_text_and_numeric_values() {
        let loads = vec![
            LoadEntry {
                cups: "=literal".to_owned(),
                fecha: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
                hora: 24,
                ae_kwh: 1.25,
            },
            LoadEntry {
                cups: "another-cups".to_owned(),
                fecha: NaiveDate::from_ymd_opt(2026, 11, 6).unwrap(),
                hora: 1,
                ae_kwh: 2.5,
            },
        ];
        let rows = get_load_row_data(&loads).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[2].values.as_ref().unwrap()[0]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("another-cups")
        );
        let cells = rows[1].values.as_ref().unwrap();
        assert_eq!(
            cells[0]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("=literal")
        );
        assert_eq!(
            cells[1]
                .user_entered_value
                .as_ref()
                .unwrap()
                .string_value
                .as_deref(),
            Some("05/11/2026")
        );
        assert_eq!(
            cells[2].user_entered_value.as_ref().unwrap().number_value,
            Some(24.0)
        );
        assert_eq!(
            cells[3].user_entered_value.as_ref().unwrap().number_value,
            Some(1.25)
        );
    }

    #[test]
    fn replacement_appends_within_a_bounded_range_and_expands_grid() {
        let properties = SheetProperties {
            sheet_id: Some(42),
            grid_properties: Some(GridProperties {
                row_count: Some(1),
                column_count: Some(2),
                ..Default::default()
            }),
            ..Default::default()
        };
        let rows = vec![
            RowData {
                values: Some(vec![CellData::default(); 4])
            };
            3
        ];
        let request = replacement_request(properties, rows, 98).unwrap();
        let requests = request.requests.unwrap();
        let grid = requests[0]
            .update_sheet_properties
            .as_ref()
            .unwrap()
            .properties
            .as_ref()
            .unwrap()
            .grid_properties
            .as_ref()
            .unwrap();
        assert_eq!(grid.row_count, Some(101));
        assert_eq!(grid.column_count, Some(4));
        let update = requests[1].update_cells.as_ref().unwrap();
        let range = update.range.as_ref().unwrap();
        assert_eq!(range.sheet_id, Some(42));
        assert_eq!(range.start_row_index, Some(98));
        assert_eq!(range.end_row_index, Some(101));
        assert_eq!(range.start_column_index, Some(0));
        assert_eq!(range.end_column_index, Some(4));
        assert!(update.start.is_none());
    }

    #[test]
    fn invalid_data_and_empty_id_fail_without_network_access() {
        let loads = vec![LoadEntry {
            cups: "cups".to_owned(),
            fecha: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
            hora: 1,
            ae_kwh: f64::NAN,
        }];
        assert!(get_load_row_data(&loads).is_err());
        let defaults = Defaults {
            bill_input: String::new(),
            load_input: String::new(),
            workbook: String::new(),
            gsheet_id: String::new(),
            bill_input_excludes: Vec::new(),
            load_input_excludes: Vec::new(),
            load_max_age_in_days: 0,
            dump_prefix: String::new(),
            locale: String::new(),
            limit: 0,
            cache_file: String::new(),
            credentials: String::new(),
            dispatchers: HashMap::new(),
            column_labels: IndexMap::new(),
            sheet_names: IndexMap::new(),
            contract_types: None,
            tariffs: None,
            loads: None,
        };
        assert!(upload_report(&defaults, &HashMap::new(), &[], " ", "", false).is_err());
    }
}
