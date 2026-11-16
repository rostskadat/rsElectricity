# rsElectricity

`rsElectricity` is a Rust command-line tool for turning a folder of Spanish
electricity bill PDFs into a single Excel workbook.

It walks a directory of bills, works out which supplier issued each one, extracts
the invoice number, billing dates, contracted power, energy, amount and the
P1–P6 / CP1–CP6 period values, and groups the results by CUPS (the Spanish
supply-point identifier). Each CUPS becomes its own worksheet, with a friendly
name configured in `defaults.yaml`. Optionally it also merges load profile CSVs
from the distributor, and it caches already-parsed bills in SQLite so that
re-runs are cheap.

Supported bill layouts (one TextFSM template each in
`crates/extractor/assets/templates/`):

| Supplier | Extractor | Dispatch marker |
| --- | --- | --- |
| Endesa | `extract_endesa_bill` | `A81948077` |
| TotalEnergies | `extract_te_bill` | `A87803862` |
| Nufri | `extract_nufri_bill` | `500015114224` |
| Qener | `extract_qener_bill` | `900 101 142` |
| Plenitude | `extract_plenitude_bill` | `B39793294` |

Supplier detection is driven by the `dispatchers` map in `defaults.yaml`: the
first marker string found in the PDF text selects the extractor. New suppliers
can be added by writing a TextFSM template and registering an extractor.

All parsers live in the `extractor` workspace crate under
`crates/extractor`, which exposes `BillInfo` plus one `extract_*_bill`
function per supplier. The `cache` crate (`crates/cache`) persists extracted
bills in SQLite. The CLI crate (`rsElectricity`) depends on both and only
handles file discovery, dispatch and workbook generation.

## Requirements

- Rust 1.85+ (the crate uses the 2024 edition) and Cargo
- A C toolchain, because `rusqlite` compiles the bundled SQLite
- The binary must be run from the repository root, because `defaults.yaml` is
  resolved relative to the current directory

## Setup

```bash
git clone <repo-url> rsElectricity
cd rsElectricity

# Build (release build is much faster for large bill archives)
cargo build --release

# Check the templates still parse
cargo test --workspace
```

The binary is produced at `target/release/rsElectricity`, so the examples below can also be run as
`./target/release/rsElectricity --refresh-workbook`.

Then edit `defaults.yaml` for your installation. The important keys are:

```yaml
bill-input: /path/to/bill/pdfs          # directory (recursively) or single PDF
load-input: /path/to/e-Distribucion     # directory with load profile CSVs
workbook: bills-Comunidad.xlsx          # generated output
bill-input-excludes:                    # regexes, matched against full path
  - "Simulation.*/.*"
include-loads: True
load-input-excludes:
  - filtered.csv
load-max-age-in-days: 730
locale: es_ES.utf8
limit: -1                               # <= 0 means "all bills"
cache-file: cached-bills.db
dispatchers: { ... }                    # supplier marker -> extractor name
column_labels: { ... }                  # worksheet headers (and column order)
sheet_names: { ... }                    # CUPS -> worksheet name
contract_types: { ... }                 # CUPS -> contract type label
```

`defaults.yaml` is required. All of its mandatory keys must be present (the
`tariffs` and `loads` keys are optional and currently reserved for
tariff-simulation work).

## Usage

First run — build the workbook from scratch (`--refresh-workbook` is needed
because otherwise the tool just looks for an existing workbook):

```bash
cargo run --release -- --refresh-workbook
```

Subsequent runs reuse the cached SQLite file and only re-parse bills whose file
name is not already cached:

```bash
cargo run --release -- --refresh-workbook
```

Inspect the raw PDF text without generating a workbook. This dumps every bill
matching the input/exclude settings and writes `dump-<pdf-name>.txt` files to
the current directory (`--limit` is not applied in dump mode):

```bash
cargo run --release -- --dump
```

Point at a different dataset without editing `defaults.yaml`:

```bash
cargo run --release -- --refresh-workbook \
  --bill-input "/path/to/other/bills" \
  --workbook bills-Other.xlsx
```

### Common options

| Option | Purpose |
| --- | --- |
| `--refresh-workbook` | Rebuild the workbook from the bills. Without it, the tool only loads an existing workbook and (optionally) uploads it. |
| `--bill-input <path>` | Bill PDFs: a directory (walked recursively) or a single PDF. |
| `--load-input <path>` | Directory with distributor load-profile CSVs. |
| `--include-loads` | Force load-profile processing even if `include-loads` is `false` in `defaults.yaml`. |
| `--limit <n>` | Process at most `n` bills (`limit` in `defaults.yaml`; `-1` or less means all). |
| `--dump` | Dump extracted PDF text to `dump-<name>.txt` instead of building a workbook. |
| `--dump-prefix <prefix>` | Prefix for the dumped `.txt` files. |
| `--workbook <path>` | Output workbook path. |
| `--no-trim-workbook` | Emit the full fixed column set instead of the `column_labels` subset. |
| `--debug` | Verbose tracing output (also controlled by `RUST_LOG`). |
| `--upload` | Upload the workbook to Google Sheets (currently a stub, see limitations). |

The following flags are accepted but not yet wired to any behaviour: `--quiet`,
`--locale`, `--incremental`, `--gsheet-id`, `--credentials`,
`--bill-input-excludes` and `--load-input-excludes`. Exclusion patterns come from
`defaults.yaml`. Caching is enabled by default, so `--use-cache` adds nothing;
remove the cache file to force re-extraction.

Load profile CSVs are expected to be semicolon-delimited with the columns
`CUPS;Fecha;Hora;AE_kWh`, dates in `dd/mm/yyyy`, and Spanish decimal formatting
(`1.234,56`). Rows older than `load-max-age-in-days` are dropped.

## Output

The generated workbook contains:

- a `Loads` worksheet with the raw load-profile rows (`CUPS`, `Fecha`, `Hora`, `AE_kWh`)
- one worksheet per CUPS, named via `sheet_names` (falling back to the CUPS
  string), whose columns and headers come from `column_labels`, plus the
  `gross_amount` column (power + energy)

The `bills-Comunidad.xlsx` file in the repository is an example result and
`cached-bills.db` is its extraction cache.

### Caching

Parsed bills are stored in the SQLite file named by `cache-file`, keyed by PDF
file name. If a bill's contents change but the file name does not, or the
extraction logic changes, delete the cache file (or point `cache-file` elsewhere)
to force re-extraction.

## Project layout

```
Cargo.toml                 Workspace root and the rsElectricity CLI package
src/main.rs                CLI entry point, file discovery, bill/load orchestration
src/cli.rs                 clap argument definitions
src/models.rs              Defaults, re-exports extractor::BillInfo
crates/cache/              cache library crate
  src/lib.rs               SQLite loading/storing of extracted bills
src/upload.rs              Google Sheets upload (stub)
crates/extractor/          extractor library crate
  src/lib.rs               Public API: BillInfo and the extract_*_bill functions
  src/common.rs            PDF text extraction and number normalization
  src/endesa.rs, te.rs...  One parser per supplier layout
  assets/templates/        TextFSM templates, embedded at compile time
crates/workbook/           XLSX generation (Loads + per-CUPS sheets)
defaults.yaml              Runtime configuration (paths, labels, dispatchers)
```

## Current limitations

- Google Sheets upload is a stub: `--upload` only logs a warning.
- `--quiet`, `--locale`, `--incremental`, `--gsheet-id`, `--credentials`,
  `--bill-input-excludes` and `--load-input-excludes` are parsed but do not
  change behaviour; use the corresponding `defaults.yaml` keys instead.
- `--dump` writes the raw extracted text (`.txt`), not JSON, despite the help
  text.
- Bill text extraction relies on the PDF having a text layer; scanned/image-only
  PDFs cannot be parsed.
