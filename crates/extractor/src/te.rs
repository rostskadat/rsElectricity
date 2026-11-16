use crate::BillInfo;
use crate::common::{get_date, get_f64, get_text, normalize_number, parse_pdf_str};
use std::collections::HashMap;

pub fn extract_te_bill(file: &str, text: &str) -> anyhow::Result<BillInfo> {
    let template_str = include_str!("../assets/templates/es.totalenergies.textfsm");
    let values = parse_pdf_str(text, template_str).unwrap_or_default();

    let mut bill = BillInfo {
        bill_id: get_text(&values, "bill_id"),
        billing_date: get_date(&values, "billing_date")?,
        billing_period_start: get_date(&values, "billing_period_start")?,
        billing_period_end: get_date(&values, "billing_period_end")?,
        cups: get_text(&values, "cups"),
        contract_type: get_text(&values, "contract_type"),
        billed_amount: get_f64(&values, "billed_amount"),
        billed_power: get_f64(&values, "billed_power"),
        billed_energy: get_f64(&values, "billed_energy"),
        p1: get_f64(&values, "p1"),
        p2: get_f64(&values, "p2"),
        p3: get_f64(&values, "p3"),
        p4: get_f64(&values, "p4"),
        p5: get_f64(&values, "p5"),
        p6: get_f64(&values, "p6"),
        cp1: get_f64(&values, "cp1"),
        cp2: get_f64(&values, "cp2"),
        cp3: get_f64(&values, "cp3"),
        cp4: get_f64(&values, "cp4"),
        cp5: get_f64(&values, "cp5"),
        cp6: get_f64(&values, "cp6"),
        file: Some(
            std::path::Path::new(file)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ),
        extra: HashMap::new(),
    };
    bill.billed_energy = (1..=6)
        .map(|i| {
            ["te_energy_access_p", "te_energy_p", "te_energy_charge_p"]
                .iter()
                .map(|prefix| {
                    values
                        .get(&format!("{prefix}{i}"))
                        .map(|s| normalize_number(s))
                        .unwrap_or(0.0)
                })
                .sum::<f64>()
        })
        .sum();
    bill.billed_power = (1..=6)
        .map(|i| {
            ["te_power_access_p", "te_power_p", "te_power_charge_p"]
                .iter()
                .map(|prefix| {
                    values
                        .get(&format!("{prefix}{i}"))
                        .map(|s| normalize_number(s))
                        .unwrap_or(0.0)
                })
                .sum::<f64>()
        })
        .sum();

    let contracted_power = values
        .get("te_contracted_power")
        .cloned()
        .unwrap_or_default();
    let cps: Vec<&str> = contracted_power.split_whitespace().collect();
    // for pair in cps.chunks_exact(2) {
    let (chunks, _) = cps.as_chunks::<2>();
    for chunk in chunks {
        let identifier = chunk[0];
        let value = chunk[1].replace(',', ".");

        if let Ok(value) = value.parse::<f64>() {
            match identifier {
                "P1" => bill.cp1 = value,
                "P2" => bill.cp2 = value,
                "P3" => bill.cp3 = value,
                "P4" => bill.cp4 = value,
                "P5" => bill.cp5 = value,
                "P6" => bill.cp6 = value,
                _ => {}
            }
        }
    }

    Ok(bill)
}
