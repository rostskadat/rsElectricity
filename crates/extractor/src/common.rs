use anyhow::Context;
use chrono::NaiveDate;
use pdfplumber::{Pdf, TextOptions};
use std::collections::HashMap;
use std::path::Path;
use textfsm_rust::Template;

pub(crate) fn get_f64(values: &HashMap<String, String>, field: &str) -> f64 {
    values
        .get(field)
        .map(|s| normalize_number(s))
        .unwrap_or(0.0)
}

pub(crate) fn get_date(values: &HashMap<String, String>, field: &str) -> anyhow::Result<NaiveDate> {
    let value = values
        .get(field)
        .with_context(|| format!("bill is missing {field}"))?;
    NaiveDate::parse_from_str(value, "%d/%m/%Y")
        .with_context(|| format!("invalid {field} {value:?}; expected DD/MM/YYYY"))
}

pub(crate) fn get_text(values: &HashMap<String, String>, field: &str) -> String {
    values.get(field).cloned().unwrap_or_default()
}

pub fn extract_text_from_pdf(path: &Path) -> anyhow::Result<String> {
    let pdf = Pdf::open_path(path.to_str().unwrap(), None)?;
    let mut pages: Vec<String> = Vec::new();
    for page in pdf.pages() {
        let page = page?;
        let text = page.extract_text(&TextOptions::default());
        pages.push(text);
    }
    Ok(pages.join("\n\n"))
}

// pub fn parse_pdf(pdf_path: &Path, template_path: &Path) -> anyhow::Result<HashMap<String, String>> {
//     let template_str = fs::read_to_string(template_path)?;
//     let template = Template::parse_str(template_str.as_str())?;
//     let mut parser = template.parser();
//     let text = extract_text_from_pdf(pdf_path)?;
//     let results = parser.parse_text_to_dicts(&text)?;
//     if results.is_empty() {
//         anyhow::bail!("No records parsed by textfsm template");
//     }
//     Ok(results[0].clone())
// }

pub fn parse_pdf_str(pdf_str: &str, template_str: &str) -> anyhow::Result<HashMap<String, String>> {
    let template = Template::parse_str(template_str)?;
    let mut parser = template.parser();
    let results = parser.parse_text_to_dicts(pdf_str)?;
    if results.is_empty() {
        anyhow::bail!("No records parsed by textfsm template");
    }
    Ok(results[0].clone())
}

pub fn normalize_number(s: &str) -> f64 {
    let mut cleaned = s.trim().to_string();
    // Remove currency symbols
    cleaned = cleaned.replace("€$", "");
    // Spanish format: 1.234,56 -> remove dots, replace comma with dot
    cleaned = cleaned.replace('.', "").replace(',', ".");
    cleaned.trim().parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::get_date;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    #[test]
    fn parses_bill_dates_in_supplier_format() {
        let values = HashMap::from([
            ("billing_date".to_owned(), "05/11/2026".to_owned()),
            ("billing_period_start".to_owned(), "01/10/2026".to_owned()),
            ("billing_period_end".to_owned(), "31/10/2026".to_owned()),
        ]);

        assert_eq!(
            get_date(&values, "billing_date").unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()
        );
        assert_eq!(
            get_date(&values, "billing_period_start").unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        assert_eq!(
            get_date(&values, "billing_period_end").unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 31).unwrap()
        );
    }

    #[test]
    fn rejects_invalid_billing_date() {
        let values = HashMap::from([("billing_date".to_owned(), "31/02/2026".to_owned())]);

        assert!(get_date(&values, "billing_date").is_err());
    }
}
