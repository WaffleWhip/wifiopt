use chrono::Local;
use clap::Parser;

#[derive(Debug, Clone)]
pub struct Options {
    pub date: String,
    pub regs: Vec<String>,
    pub out: String,
    pub db_url: String,
    pub stale: usize,
    /// Write results to wifi_optimize_<date>. Off by default (fetch-only).
    pub upload: bool,
    /// Override upload table name (scratch testing). Empty = wifi_optimize_<date>.
    pub table: String,
    /// Max cluster size for exhaustive joint search (larger falls back
    /// to coordinate descent inside the joint optimizer).
    pub bruteforce_max_k: usize,
}

#[derive(Parser, Debug)]
#[command(name = "wifiopt", about = "Fetch-only WiFi channel optimizer")]
struct Cli {
    #[arg(long, default_value = "")]
    date: String,
    #[arg(long, default_value = "all")]
    reg: String,
    #[arg(long)]
    out: String,
    #[arg(long, env = "DATABASE_URL", default_value = "mysql://127.0.0.1:3306/wifi_optimization")]
    db_url: String,
    #[arg(long, default_value_t = 50)]
    stale: usize,
    /// Upload results to MySQL wifi_optimize_<date> (default: fetch-only CSV).
    #[arg(long, default_value_t = false)]
    upload: bool,
    /// Override upload table (scratch testing). Must be [A-Za-z0-9_]+.
    #[arg(long, default_value = "")]
    table: String,
    /// Max cluster size for exhaustive joint search (larger falls back
    /// to coordinate descent inside the joint optimizer).
    #[arg(long, default_value_t = 4)]
    bruteforce_max_k: usize,
}

pub fn parse_args() -> Options {
    let cli = Cli::parse();
    from_cli(cli)
}

pub fn parse_from<I, S>(args: I) -> Options
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| e.exit());
    from_cli(cli)
}

fn from_cli(cli: Cli) -> Options {
    let date = if cli.date.is_empty() {
        Local::now().format("%Y%m%d").to_string()
    } else {
        cli.date
    };
    let regs = expand_regs(&cli.reg);
    Options {
        date,
        regs,
        out: cli.out,
        db_url: cli.db_url,
        stale: cli.stale,
        upload: cli.upload,
        table: cli.table,
        bruteforce_max_k: cli.bruteforce_max_k,
    }
}

fn expand_regs(raw: &str) -> Vec<String> {
    if raw.trim().eq_ignore_ascii_case("all") {
        return (1..=7).map(|i| format!("reg{i}")).collect();
    }
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
