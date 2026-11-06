//! Extract structured data from Spanish electricity bill PDFs.
//!
//! The crate detects a supplier-specific bill layout by parsing the PDF text with
//! a TextFSM template, with regex fallbacks for some suppliers, and returns a
//! [`BillInfo`] with the invoice, period, power and energy fields.

mod common;
mod endesa;
mod nufri;
mod plenitude;
mod qener;
mod te;

pub use ::common::BillInfo;
pub use common::{extract_text_from_pdf, normalize_number};
pub use endesa::extract_endesa_bill;
pub use nufri::extract_nufri_bill;
pub use plenitude::extract_plenitude_bill;
pub use qener::extract_qener_bill;
pub use te::extract_te_bill;

#[cfg(test)]
mod tests {
    use textfsm_rust::validate_templates;

    #[test]
    fn validate_textfsm_templates() {
        validate_templates!("assets/templates/");
    }
}
