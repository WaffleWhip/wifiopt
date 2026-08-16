mod cluster;
mod logger;
mod mysql;
mod optimize;
mod parse;

use anyhow::{Context, Result};
use chrono::{Duration, Local};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::{Duration as StdDuration, Instant};
use tokio::time::sleep;

#[derive(Debug, Deserialize, Clone)]
struct Config {
    cluster: ClusterCfg,
    #[serde(default)]
    optimize: OptimizeCfg,
    #[serde(default)]
    cron: CronCfg,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct ClusterCfg {
    min_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct OptimizeCfg {
    #[serde(default = "default_stale_threshold")]
    stale_threshold: usize,
}

impl Default for OptimizeCfg {
    fn default() -> Self {
        Self {
            stale_threshold: default_stale_threshold(),
        }
    }
}

fn default_stale_threshold() -> usize {
    50
}

#[derive(Debug, Deserialize, Clone)]
struct CronCfg {
    #[serde(default = "default_timezone")]
    timezone: String,
    #[serde(default = "default_cron_hour")]
    hour: u8,
    #[serde(default = "default_cron_minute")]
    minute: u8,
    #[serde(default = "default_date")]
    date: i64,
}

impl Default for CronCfg {
    fn default() -> Self {
        Self {
            timezone: default_timezone(),
            hour: default_cron_hour(),
            minute: default_cron_minute(),
            date: default_date(),
        }
    }
}

fn default_timezone() -> String {
    "Asia/Jakarta".to_string()
}
fn default_cron_hour() -> u8 {
    16
}
fn default_cron_minute() -> u8 {
    30
}
fn default_date() -> i64 {
    0
}

impl Config {
    fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read config {}", path.as_ref().display()))?;
        let cfg: Self = serde_json::from_str(&text).context("parse config.json")?;
        Ok(cfg)
    }
    fn invalid_bssid_set(&self) -> std::collections::HashSet<String> {
        let mut s = std::collections::HashSet::new();
        s.insert("00:00:00:00:00:00".to_string());
        s
    }
    fn band_ok(&self, ch: i32, band: &str) -> bool {
        match band {
            "2g" => ch >= 1 && ch <= 13,
            "5g" => ch >= 36 && ch <= 165,
            _ => false,
        }
    }
}

impl CronCfg {
    pub fn resolve_date(&self, explicit: Option<&str>) -> Option<String> {
        use chrono::{Duration, Local};
        match explicit {
            Some(d) if !d.is_empty() => Some(d.to_string()),
            _ => {
                let target = Local::now() - Duration::days(self.date);
                Some(target.format("%Y%m%d").to_string())
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
dotenvy::dotenv().ok();

    let mut date: Option<String> = None;
    let mut region_filter: Option<u8> = None;
    let mut loop_mode = false;
    let mut iter = env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--date" => date = iter.next(),
            "--region" | "--reg" => {
                if let Some(v) = iter.next() {
                    region_filter = v.parse().ok();
                }
            }
            "--loop" => loop_mode = true,
            _ => {}
        }
    }

    if loop_mode {
        return run_scheduler_loop().await;
    }

    let cfg = Config::load("config.json").context("load config.json")?;
    let target_date = cfg
        .cron
        .resolve_date(date.as_deref())
        .context("resolve date")?;

    let log_date = match region_filter {
        Some(r) => format!("{target_date}_reg{r}"),
        None => target_date.clone(),
    };
    let log = logger::Logger::new(&log_date)?;
    log.log(&format!("=== wifiopt daily run start ==="));
    log.log(&format!("date: {target_date}"));
    if let Some(r) = region_filter {
        log.log(&format!("region filter: reg{r}"));
    }
    log.log(&format!(
        "timezone: {} hour: {} minute: {} date_offset: {}",
        cfg.cron.timezone, cfg.cron.hour, cfg.cron.minute, cfg.cron.date
    ));

    let t_total = Instant::now();
    if let Err(e) = run_daily(&cfg, &target_date, region_filter, &log).await {
        log.log(&format!("FAILED: {e:?}"));
        return Err(e);
    }
    log.log(&format!(
        "=== wifiopt daily run done in {:.2}s ===",
        t_total.elapsed().as_secs_f64()
    ));
    Ok(())
}

async fn run_daily(
    cfg: &Config,
    date: &str,
    region_filter: Option<u8>,
    log: &logger::Logger,
) -> Result<()> {
    let pool = mysql::connect().await.context("connect MySQL")?;

    let regions = mysql::list_regions(date, &pool)
        .await
        .with_context(|| format!("list regions for {date}"))?;
    if regions.is_empty() {
        log.log(&format!("no regions found for date {date}, abort"));
        return Ok(());
    }
    let regions: Vec<u8> = match region_filter {
        Some(r) if regions.contains(&r) => vec![r],
        Some(r) => {
            log.log(&format!("region {r} not found for date {date}"));
            return Ok(());
        }
        None => regions,
    };
    log.log(&format!("regions to process: {regions:?} ({} total)", regions.len()));

    let target_table = mysql::ensure_table(date, &pool)
        .await
        .with_context(|| format!("ensure table wifi_optimize_{date}"))?;
    log.log(&format!("target table: {target_table}"));

    if region_filter.is_some() {
        log.log(&format!(
            "[single-region mode] truncate skipped (other regions may still be running)"
        ));
    } else {
        mysql::truncate_table(&target_table, &pool)
            .await
            .context("truncate target table")?;
    }

    let grand_t = Instant::now();
    let max_idle = cfg.optimize.stale_threshold;
    let total_regions = regions.len();

    let mut tasks = tokio::task::JoinSet::new();
    for (idx, reg_n) in regions.iter().enumerate() {
        let reg = format!("reg{reg_n}");
        let table = format!("discover_interference_reborn_{date}_{reg}");
        let date_s = date.to_string();
        let max_idle = max_idle;
        let total = total_regions;
        let log_path = log.date().to_string();

        tasks.spawn(async move {
            let log_ref = match logger::Logger::new(&log_path) {
                Ok(l) => l,
                Err(e) => return (reg, Err(anyhow::anyhow!("logger init: {e}"))),
            };
            log_ref.log(&format!(
                "--- [{}/{}] {} start ---",
                idx + 1,
                total,
                reg
            ));
            let t_reg = Instant::now();
            let res = process_region(&reg, &table, &date_s, max_idle, &log_ref).await;
            match &res {
                Ok((rows, ch_2g, st_2g, ch_5g, st_5g)) => {
                    log_ref.log(&format!(
                        "[{reg}] optimize 2g: change={} stay={} | 5g: change={} stay={} | rows={} (took {:.2}s)",
                        ch_2g, st_2g, ch_5g, st_5g, rows, t_reg.elapsed().as_secs_f64()
                    ));
                }
                Err(e) => log_ref.log(&format!("[{reg}] FAILED: {e:?}")),
            }
            (reg, res)
        });
    }

    let mut total_rows: u64 = 0;
    let mut total_change_2g: u64 = 0;
    let mut total_change_5g: u64 = 0;
    let mut total_stay_2g: u64 = 0;
    let mut total_stay_5g: u64 = 0;
    while let Some(res) = tasks.join_next().await {
        match res {
            Ok((_reg, Ok((rows, ch_2g, st_2g, ch_5g, st_5g)))) => {
                total_rows += rows;
                total_change_2g += ch_2g;
                total_change_5g += ch_5g;
                total_stay_2g += st_2g;
                total_stay_5g += st_5g;
            }
            Ok((reg, Err(e))) => log.log(&format!("[{reg}] error: {e:?}")),
            Err(e) => log.log(&format!("[task] join error: {e}")),
        }
    }

    log.log(&format!(
        "=== summary === regions={} total rows={} | 2g change={} stay={} | 5g change={} stay={} | elapsed {:.2}s",
        total_regions,
        total_rows,
        total_change_2g,
        total_stay_2g,
        total_change_5g,
        total_stay_5g,
        grand_t.elapsed().as_secs_f64()
    ));

    pool.close().await;
    Ok(())
}

async fn process_region(
    reg: &str,
    table: &str,
    date: &str,
    max_idle: usize,
    log: &logger::Logger,
) -> Result<(u64, u64, u64, u64, u64)> {
    let pool = mysql::connect().await.context("connect MySQL")?;

    let (data_2g, data_5g) = mysql::fetch_and_parse(table, &pool, &std::collections::HashSet::from([
        "00:00:00:00:00:00".to_string(),
    ]))
    .await
    .with_context(|| format!("parse {table}"))?;
    let n = data_2g.len();
    log.log(&format!("[{reg}] parsed rows: {n}"));

    let (cluster_2g, cluster_5g) = run_cluster_distribution(
        &Config {
            cluster: ClusterCfg { min_size: 2 },
            optimize: OptimizeCfg { stale_threshold: max_idle },
            cron: CronCfg {
                timezone: "Asia/Jakarta".to_string(),
                hour: 16,
                minute: 30,
                date: -1,
            },
        },
        reg,
        &data_2g,
        &data_5g,
        n,
        log,
    )
    .await?;
    log.log(&format!(
        "[{reg}] cluster 2g: {} clusters, 5g: {} clusters",
        cluster_2g.valid.len(),
        cluster_5g.valid.len()
    ));

    let rows_2g = optimize::optimize_band_cluster_aware(
        &data_2g,
        &cluster_2g.valid.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        max_idle,
        reg,
        "2g",
    )?;
    let rows_5g = optimize::optimize_band_cluster_aware(
        &data_5g,
        &cluster_5g.valid.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        max_idle,
        reg,
        "5g",
    )?;

    let ch_2g: u64 = rows_2g.iter().filter(|r| r.status == "CHANGE").count() as u64;
    let st_2g: u64 = rows_2g.len() as u64 - ch_2g;
    let ch_5g: u64 = rows_5g.iter().filter(|r| r.status == "CHANGE").count() as u64;
    let st_5g: u64 = rows_5g.len() as u64 - ch_5g;

    let target_table = mysql::ensure_table(date, &pool).await?;
    let joined = mysql::join_bands(rows_2g, rows_5g);
    mysql::upload_optimize(&joined, &target_table, &pool)
        .await
        .context("upload")?;

    pool.close().await;
    Ok((n as u64, ch_2g, st_2g, ch_5g, st_5g))
}

async fn run_cluster_distribution(
    cfg: &Config,
    label: &str,
    data_2g: &[cluster::MemRow],
    data_5g: &[cluster::MemRow],
    total_sn: usize,
    log: &logger::Logger,
) -> Result<(cluster::ClusterResult, cluster::ClusterResult)> {
    let mut out_2g = None;
    let mut out_5g = None;

    for (band, data) in [("2g", data_2g), ("5g", data_5g)] {
        let t = Instant::now();
        let res = cluster::cluster_dist_in_mem(data, band, cfg)?;
        let elapsed = t.elapsed().as_secs_f64();

        let mut dist: HashMap<usize, usize> = HashMap::new();
        for entry in &res.valid {
            *dist.entry(entry.0.len()).or_insert(0) += 1;
        }
        let sn_in_cluster: usize = res.valid.iter().map(|(sns, _)| sns.len()).sum();
        let singleton = total_sn - sn_in_cluster;

        log.log(&format!(
            "[cluster {label}/{band}] components>=min({})={} sn_in_cluster={} singleton={} elapsed={:.2}s",
            res.cluster_min,
            res.valid.len(),
            sn_in_cluster,
            singleton,
            elapsed
        ));

        let mut keys: Vec<usize> = dist.keys().copied().collect();
        keys.sort();
        let mut lines = vec![format!("[cluster {label}/{band}] distribution:")];
        for k in keys {
            lines.push(format!("  size {:>5} : {}", k, dist[&k]));
        }
        let distribution = lines.join("\n");
        log.log(&distribution);

        let threshold = 2000;
        for (idx, (sns, bs)) in res.valid.iter().enumerate().take(5) {
            if sns.len() >= threshold {
                log.log(&format!(
                    "# anomaly rank={} size={} bssids={} sns_sample={:?}",
                    idx + 1,
                    sns.len(),
                    bs.len(),
                    &sns[..sns.len().min(3)]
                ));
            }
        }

        if band == "2g" {
            out_2g = Some(res);
        } else {
            out_5g = Some(res);
        }
    }
    Ok((out_2g.unwrap(), out_5g.unwrap()))
}

async fn run_scheduler_loop() -> Result<()> {
    eprintln!("[scheduler] wifiopt loop mode started (TZ={})", std::env::var("TZ").unwrap_or_default());
    loop {
        let cfg = Config::load("config.json").context("load config.json")?;
        let now = Local::now();
        let today_at = now
            .date_naive()
            .and_hms_opt(cfg.cron.hour as u32, cfg.cron.minute as u32, 0)
            .and_then(|dt| dt.and_local_timezone(Local).single());
        let next_run = match today_at {
            Some(t) if t > now => t,
            _ => {
                let tomorrow = now.date_naive().succ_opt().unwrap();
                tomorrow
                    .and_hms_opt(cfg.cron.hour as u32, cfg.cron.minute as u32, 0)
                    .and_then(|dt| dt.and_local_timezone(Local).single())
                    .unwrap_or_else(|| now + Duration::days(1))
            }
        };
        let sleep_sec = (next_run - now).num_seconds().max(1) as u64;
        eprintln!(
            "[scheduler] next run at {} (sleep {}s = {:.1}h)",
            next_run.format("%Y-%m-%d %H:%M:%S %Z"),
            sleep_sec,
            sleep_sec as f64 / 3600.0
        );

        sleep(StdDuration::from_secs(sleep_sec)).await;

        // Re-read config (schedule/date_offset can change while sleeping).
        let cfg = Config::load("config.json").context("load config.json")?;
        let target_date = cfg
            .cron
            .resolve_date(None)
            .context("resolve date")?;
        let log_date = format!("{target_date}");
        let log = match logger::Logger::new(&log_date) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[scheduler] logger init failed: {e:?}");
                continue;
            }
        };
        log.log("[scheduler] trigger: wifiopt daily run");
        log.log(&format!(
            "config: timezone={} hour={} minute={} date_offset={}",
            cfg.cron.timezone, cfg.cron.hour, cfg.cron.minute, cfg.cron.date
        ));

        match run_daily(&cfg, &target_date, None, &log).await {
            Ok(_) => log.log("[scheduler] daily run completed"),
            Err(e) => log.log(&format!("[scheduler] daily run failed: {e:?}")),
        }

        // Exit so docker restarts the container — releases accumulated heap memory.
        log.log("[scheduler] exiting for container restart");
        std::process::exit(0);
    }
}