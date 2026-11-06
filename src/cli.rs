use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Extract bill information from PDF files and generate a report.",
    long_about = "Extract bill information from PDF files and generate a report.\n\nDefaults are loaded from defaults.yaml (dynamic).",
    term_width = 240
)]
pub struct Args {
    #[arg(long, default_value_t = false, help = "Enable debug logging")]
    pub debug: bool,

    #[arg(long, default_value_t = false, help = "Suppress non-error output")]
    pub quiet: bool,

    #[arg(long, help = "Locale to use for parsing and formatting (e.g., es_ES)")]
    pub locale: Option<String>,

    #[command(subcommand)]
    pub command: Commands,

    // BILL INPUT OPTIONS
    #[arg(
        long,
        value_name = "PATH",
        help_heading = "Bills options",
        help = "Path to bill PDF input files or directory"
    )]
    pub bill_input: Option<String>,

    #[arg(
        long,
        value_name = "PATTERN",
        value_delimiter = ',',
        help_heading = "Bills options",
        help = "Pattern of bills to exclude"
    )]
    pub bill_input_excludes: Option<Vec<String>>,

    // LOAD INPUT OPTIONS
    #[arg(
        long,
        value_name = "PATH",
        help_heading = "Load options",
        help = "Path to load profile input files or directory"
    )]
    pub load_input: Option<String>,

    #[arg(
        value_name = "PATTERN",
        value_delimiter = ',',
        help_heading = "Load options",
        help = "Pattern of load profiles to exclude"
    )]
    pub load_input_excludes: Option<Vec<String>>,

    #[arg(
        long,
        value_name = "DAYS",
        help_heading = "Load options",
        help = "Maximum age of load files in days"
    )]
    pub load_max_age_in_days: Option<i32>,

    // LIMIT OPTIONS
    #[arg(long, help = "Limit number of bills / loads to process")]
    pub limit: Option<i32>,

    #[arg(
        long,
        default_value_t = true,
        help = "Only process new bills incrementally"
    )]
    pub incremental: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Dump {
        #[arg(long, help = "Prefix for dumped JSON files")]
        dump_prefix: Option<String>,
    },
    Process {
        // CACHING OPTIONS
        #[arg(
            long = "use-cache",
            action = clap::ArgAction::SetTrue,
            overrides_with = "no_use_cache",
            help_heading = "Cache options",
            help = "Use the cache of previously processed bills. Negate with '--no-use-cache'",
            conflicts_with = "no_use_cache",
        )]
        use_cache: bool,

        #[arg(
            long = "no-use-cache",
            default_value_t = true,
            action = clap::ArgAction::SetFalse,
            overrides_with = "use_cache",
            help_heading = "Cache options",
            help = "Do not use the cache of previously processed bills",
            conflicts_with = "use_cache",
            hide = true,
        )]
        no_use_cache: bool,

        // WORKBOOK OPTIONS
        #[arg(
            long,
            help_heading = "Workbook options",
            help = "Path to workbook file"
        )]
        workbook: Option<String>,

        #[arg(
            long = "overwrite",
            action = clap::ArgAction::SetTrue,
            overrides_with = "no_overwrite",
            help_heading = "Workbook options",
            help = "Overwrite the workbook if it exists. Negate with '--no-overwrite'",
            conflicts_with = "no_overwrite",
        )]
        overwrite: bool,

        #[arg(
            long = "no-overwrite",
            default_value_t = true,
            action = clap::ArgAction::SetFalse,
            overrides_with = "overwrite",
            help_heading = "Workbook options",
            help = "Do not overwrite the workbook if it exists.",
            conflicts_with = "overwrite",
            hide = true,
        )]
        no_overwrite: bool,

        #[arg(
            long = "append",
            action = clap::ArgAction::SetTrue,
            overrides_with = "no_append",
            help_heading = "Workbook options",
            help = "Append to the workbook. Negate with '--no-append'",
            conflicts_with = "no_append",
        )]
        append: bool,

        #[arg(
            long = "no-append",
            default_value_t = false,
            action = clap::ArgAction::SetFalse,
            overrides_with = "append",
            help_heading = "Workbook options",
            help = "Do not append to the workbook",
            conflicts_with = "append",
            hide = true,
        )]
        no_append: bool,

        // UPLOAD OPTIONS
        #[arg(
            long = "upload",
            action = clap::ArgAction::SetTrue,
            overrides_with = "no_upload",
            help_heading = "Upload options",
            help = "Upload the workbook to Google. Negate with '--no-upload'",
            conflicts_with = "no_upload",
        )]
        upload: bool,

        #[arg(
            long = "no-upload",
            default_value_t = true,
            action = clap::ArgAction::SetFalse,
            overrides_with = "upload",
            help_heading = "Upload options",
            help = "Do not upload the workbook to Google",
            conflicts_with = "upload",
            hide = true,
        )]
        no_upload: bool,

        #[arg(
            long,
            help_heading = "Upload options",
            help = "Google Sheets ID for upload"
        )]
        gsheet_id: Option<String>,

        #[arg(
            long,
            help_heading = "Upload options",
            help = "Path to Google API credentials JSON"
        )]
        credentials: Option<String>,

        #[arg(
            long = "incremental",
            action = clap::ArgAction::SetTrue,
            overrides_with = "no_incremental",
            help_heading = "Upload options",
            help = "Do incremental upload. Only add new bills. Loads are always replaced. Negate with '--no-incremental'",
            conflicts_with = "no_incremental",
        )]
        incremental: bool,

        #[arg(
            long = "no-incremental",
            default_value_t = true,
            action = clap::ArgAction::SetFalse,
            overrides_with = "incremental",
            help_heading = "Upload options",
            help = "Do not do incremental upload",
            conflicts_with = "incremental",
            hide = true,
        )]
        no_incremental: bool,
    },
}

impl Args {
    pub fn log_level(&self) -> String {
        std::env::var("RUST_LOG").unwrap_or_else(|_| {
            if self.debug && !self.quiet {
                "debug".to_string()
            } else if self.quiet {
                "error".to_string()
            } else {
                "info".to_string()
            }
        })
    }
}
