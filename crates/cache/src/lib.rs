//! SQLite cache of bills already extracted by the `extractor` crate.

use common::BillInfo;
use rusqlite::{Connection, params};
use std::collections::HashMap;

/// Open (or create) the cache database and load every previously extracted bill.
///
/// The returned connection should be passed to [`mark_as_processed`] for bills
/// parsed during the current run.
pub fn load_cache(cache_file: &str) -> anyhow::Result<(Connection, HashMap<String, BillInfo>)> {
    let conn = Connection::open(cache_file)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS extracted_bills (
            file TEXT PRIMARY KEY,
            bill_info TEXT,
            exctracted_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    let mut known = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT file, bill_info FROM extracted_bills")?;
        let rows = stmt.query_map([], |row| {
            let file: String = row.get(0)?;
            let bill_info_json: String = row.get(1)?;
            Ok((file, bill_info_json))
        })?;
        for row in rows.flatten() {
            let (file, json_str) = row;
            if let Ok(bill) = serde_json::from_str::<BillInfo>(&json_str) {
                known.insert(file, bill);
            }
        }
    }
    Ok((conn, known))
}

/// Store an extracted bill in the cache, keyed by its PDF file name.
pub fn mark_as_processed(conn: &Connection, bill_info: &BillInfo) -> anyhow::Result<()> {
    let bill_info_ser = serde_json::to_string(bill_info)?;
    conn.execute(
        "INSERT OR IGNORE INTO extracted_bills (file, bill_info) VALUES (?, ?)",
        params![
            bill_info.file.as_ref().unwrap_or(&String::new()),
            bill_info_ser
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bill(file: &str) -> BillInfo {
        serde_json::from_value(serde_json::json!({
            "bill_id": "B1",
            "billing_date": "01/01/2026",
            "billing_period_start": "01/01/2026",
            "billing_period_end": "31/01/2026",
            "cups": "ES0000000000000001AB0F",
            "contract_type": "2.0",
            "billed_amount": 1.0,
            "billed_power": 2.0,
            "billed_energy": 3.0,
            "p1": 1.0, "p2": 2.0, "p3": 3.0,
            "p4": 4.0, "p5": 5.0, "p6": 6.0,
            "cp1": 1.0, "cp2": 2.0, "cp3": 3.0,
            "cp4": 4.0, "cp5": 5.0, "cp6": 6.0,
            "file": file,
        }))
        .unwrap()
    }

    #[test]
    fn round_trips_extracted_bills() {
        let path = std::env::temp_dir().join(format!(
            "rsElectricity-cache-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let path = path.to_str().unwrap();

        let (conn, known) = load_cache(path).unwrap();
        assert!(known.is_empty());

        let bill = sample_bill("bill.pdf");
        mark_as_processed(&conn, &bill).unwrap();
        // Inserting the same file twice must not create a second row.
        mark_as_processed(&conn, &bill).unwrap();
        drop(conn);

        let (conn, known) = load_cache(path).unwrap();
        assert_eq!(known.len(), 1);
        let loaded = known.get("bill.pdf").unwrap();
        assert_eq!(loaded.bill_id, "B1");
        assert_eq!(loaded.cups, bill.cups);
        assert_eq!(loaded.p6, 6.0);

        drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
