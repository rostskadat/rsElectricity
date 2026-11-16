//! Shared configuration and data types for electricity bill processing and reporting.

use chrono::NaiveDate;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Structured data extracted from a single electricity bill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillInfo {
    pub bill_id: String,
    #[serde(with = "date_serde")]
    pub billing_date: NaiveDate,
    #[serde(with = "date_serde")]
    pub billing_period_start: NaiveDate,
    #[serde(with = "date_serde")]
    pub billing_period_end: NaiveDate,
    pub cups: String,
    pub contract_type: String,
    #[serde(default)]
    pub billed_amount: f64,
    #[serde(default)]
    pub billed_power: f64,
    #[serde(default)]
    pub billed_energy: f64,
    #[serde(default)]
    pub p1: f64,
    #[serde(default)]
    pub p2: f64,
    #[serde(default)]
    pub p3: f64,
    #[serde(default)]
    pub p4: f64,
    #[serde(default)]
    pub p5: f64,
    #[serde(default)]
    pub p6: f64,
    #[serde(default)]
    pub cp1: f64,
    #[serde(default)]
    pub cp2: f64,
    #[serde(default)]
    pub cp3: f64,
    #[serde(default)]
    pub cp4: f64,
    #[serde(default)]
    pub cp5: f64,
    #[serde(default)]
    pub cp6: f64,
    pub file: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Serde helpers for NaiveDate in dd/mm/yyyy format.
mod date_serde {
    use chrono::NaiveDate;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &NaiveDate, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&date.format("%d/%m/%Y").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        NaiveDate::parse_from_str(&value, "%d/%m/%Y").map_err(serde::de::Error::custom)
    }
}

/// A single hourly electricity consumption record.
#[derive(Debug)]
pub struct LoadEntry {
    pub cups: String,
    pub fecha: NaiveDate,
    pub hora: u8,
    pub ae_kwh: f64,
}

/// Application defaults deserialized from defaults.yaml.
#[derive(Debug, Clone, Deserialize)]
pub struct Defaults {
    #[serde(rename = "bill-input")]
    pub bill_input: String,
    #[serde(rename = "load-input")]
    pub load_input: String,
    pub workbook: String,
    #[serde(rename = "gsheet-id")]
    pub gsheet_id: String,
    #[serde(rename = "bill-input-excludes")]
    pub bill_input_excludes: Vec<String>,
    #[serde(rename = "load-input-excludes")]
    pub load_input_excludes: Vec<String>,
    #[serde(rename = "load-max-age-in-days")]
    pub load_max_age_in_days: i32,
    #[serde(rename = "dump-prefix")]
    pub dump_prefix: String,
    pub locale: String,
    pub limit: i32,
    #[serde(rename = "cache-file")]
    pub cache_file: String,
    pub credentials: String,
    pub dispatchers: HashMap<String, String>,
    #[serde(rename = "column_labels")]
    pub column_labels: IndexMap<String, String>,
    #[serde(rename = "sheet_names")]
    pub sheet_names: IndexMap<String, String>,
    #[serde(rename = "contract_types")]
    pub contract_types: Option<HashMap<String, String>>,
    pub tariffs: Option<HashMap<String, Vec<f64>>>,
    pub loads: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::{BillInfo, Defaults};

    #[test]
    fn bill_serialization_defaults_missing_period_values() {
        let input = serde_json::json!({
            "bill_id": "invoice-1",
            "billing_date": "05/11/2026",
            "billing_period_start": "01/10/2026",
            "billing_period_end": "31/10/2026",
            "cups": "test-cups",
            "contract_type": "2.0TD",
            "billed_amount": 30.0,
            "billed_power": 10.0,
            "billed_energy": 15.0,
            "p1": 1.25,
            "file": null,
            "supplier_note": "test"
        });
        let bill: BillInfo = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(bill.p1, 1.25);
        assert_eq!(bill.p2, 0.0);
        assert_eq!(
            bill.extra.get("supplier_note"),
            Some(&serde_json::json!("test"))
        );
        let serialized = serde_json::to_value(bill).unwrap();
        assert_eq!(serialized["supplier_note"], "test");
        assert_eq!(serialized["p1"], 1.25);
        for field in [
            "p2", "p3", "p4", "p5", "p6", "cp1", "cp2", "cp3", "cp4", "cp5", "cp6",
        ] {
            assert_eq!(serialized[field], 0.0);
        }
    }

    #[test]
    fn sheet_names_preserve_yaml_key_order() {
        let defaults: Defaults = serde_yaml::from_str(
            r#"bill-input: ''
load-input: ''
workbook: ''
gsheet-id: ''
bill-input-excludes: []
load-input-excludes: []
load-max-age-in-days: 0
dump-prefix: ''
locale: ''
limit: 0
cache-file: ''
credentials: ''
dispatchers: {}
column_labels: {}
sheet_names:
    first-cups: First
    second-cups: Second
contract_types: {}
"#,
        )
        .expect("valid defaults YAML");

        assert_eq!(
            defaults
                .sheet_names
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["first-cups", "second-cups"]
        );
    }
}
