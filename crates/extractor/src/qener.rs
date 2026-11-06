use crate::BillInfo;
use crate::common::{get_date, get_f64, get_text, parse_pdf_str};
use std::collections::HashMap;

pub fn extract_qener_bill(file: &str, text: &str) -> anyhow::Result<BillInfo> {
    let template_str = include_str!("../assets/templates/es.qener.textfsm");
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
    if bill.contract_type == "2.0TD" {
        let tmp = bill.cp2;
        bill.cp2 = 0.0;
        bill.cp3 = tmp;
    }
    Ok(bill)
}
