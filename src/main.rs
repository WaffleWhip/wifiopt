mod cluster;
mod logger;
mod mysql;
mod optimize;
mod parse;

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Deserialize, Clone)]
struct Config {
    cluster: ClusterCfg,
    optimize: OptimizeCfg,
    cron: CronCfg,
}

#[derive(Debug, Deserialize, Clone)]
struct ClusterCfg {
    min_size: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct OptimizeCfg {
    stale_threshold: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct CronCfg {
    timezone: String,
    hour: u8,
    minute: u8,
    date: i64,
}

impl Config {
    fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read config {}", path.as_ref().display()))?;
        if text.trim().is_empty() {
            anyhow::bail!("config {} is empty", path.as_ref().display());
        }
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
            "2g" => (1..=13).contains(&ch),
            "5g" => (36..=165).contains(&ch),
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
    let mut iter = env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--date" => date = iter.next(),
            "--region" | "--reg" => {
                if let Some(v) = iter.next() {
                    region_filter = v.parse().ok();
                }
            }
            _ => {}
        }
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
    log.log("=== wifiopt daily run start ===");
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

    let mut regions = mysql::list_regions(date, &pool)
        .await
        .with_context(|| format!("list regions for {date}"))?;
    if regions.is_empty() {
        log.log(&format!("no regions found for date {date}, abort"));
        return Ok(());
    }
    regions.sort_unstable();
    let regions: Vec<u8> = match region_filter {
        Some(r) if regions.contains(&r) => vec![r],
        Some(r) => {
            log.log(&format!("region {r} not found for date {date}"));
            return Ok(());
        }
        None => regions,
    };
    log.log(&format!(
        "regions to process: {regions:?} ({} total)",
        regions.len()
    ));

    let target_table = mysql::ensure_table(date, &pool)
        .await
        .with_context(|| format!("ensure table wifi_optimize_{date}"))?;
    log.log(&format!("target table: {target_table}"));

    if region_filter.is_some() {
        log.log("[single-region mode] truncate skipped");
    } else {
        mysql::truncate_table(&target_table, &pool)
            .await
            .context("truncate target table")?;
    }

    let grand_t = Instant::now();
    let max_idle = cfg.optimize.stale_threshold;
    let total_regions = regions.len();

    let mut total_rows: u64 = 0;
    let mut total_change_2g: u64 = 0;
    let mut total_change_5g: u64 = 0;
    let mut total_stay_2g: u64 = 0;
    let mut total_stay_5g: u64 = 0;

    let mut prev_upload_task: Option<tokio::task::JoinHandle<Result<()>>> = None;

    for (idx, reg_n) in regions.iter().enumerate() {
        let reg = format!("reg{reg_n}");
        let table = format!("discover_interference_reborn_{date}_{reg}");
        let reg_idx = idx + 1;

        log.log(&format!(
            "--- [{reg_idx}/{total_regions}] {reg} compute start ---"
        ));
        let t_reg = Instant::now();

        let (data_2g, data_5g) = mysql::fetch_and_parse(&table, &pool, &cfg.invalid_bssid_set())
            .await
            .with_context(|| format!("parse {table}"))?;
        let n = data_2g.len();
        log.log(&format!("[{reg}] parsed rows: {n}"));

        let (cluster_2g, cluster_5g) =
            run_cluster_distribution(cfg, &reg, &data_2g, &data_5g, n, log).await?;
        log.log(&format!(
            "[{reg}] cluster 2g: {} clusters, 5g: {} clusters",
            cluster_2g.valid.len(),
            cluster_5g.valid.len()
        ));

        let cluster_sns_2g: Vec<Vec<String>> =
            cluster_2g.valid.into_iter().map(|(s, _)| s).collect();
        let cluster_sns_5g: Vec<Vec<String>> =
            cluster_5g.valid.into_iter().map(|(s, _)| s).collect();

        let rows_2g =
            optimize::optimize_band_cluster_aware(&data_2g, &cluster_sns_2g, max_idle, &reg, "2g")?;
        let rows_5g =
            optimize::optimize_band_cluster_aware(&data_5g, &cluster_sns_5g, max_idle, &reg, "5g")?;

        drop(data_2g);
        drop(data_5g);
        drop(cluster_sns_2g);
        drop(cluster_sns_5g);

        let ch_2g = rows_2g.iter().filter(|r| r.status == "CHANGE").count() as u64;
        let st_2g = rows_2g.len() as u64 - ch_2g;
        let ch_5g = rows_5g.iter().filter(|r| r.status == "CHANGE").count() as u64;
        let st_5g = rows_5g.len() as u64 - ch_5g;

        total_rows += n as u64;
        total_change_2g += ch_2g;
        total_stay_2g += st_2g;
        total_change_5g += ch_5g;
        total_stay_5g += st_5g;

        let joined = mysql::join_bands(rows_2g, rows_5g);

        log.log(&format!(
            "[{reg}] optimize 2g: change={ch_2g} stay={st_2g} | 5g: change={ch_5g} stay={st_5g} | rows={n} (compute took {:.2}s)",
            t_reg.elapsed().as_secs_f64()
        ));

        if let Some(prev) = prev_upload_task.take() {
            prev.await.context("join previous upload task")??;
        }

        let pool_clone = pool.clone();
        let target_table_clone = target_table.clone();
        let reg_name = reg.clone();
        let log_ref = logger::Logger::new(log.date())?;

        prev_upload_task = Some(tokio::spawn(async move {
            let t_up = Instant::now();
            mysql::upload_optimize(&joined, &target_table_clone, &pool_clone)
                .await
                .with_context(|| format!("upload {reg_name}"))?;
            log_ref.log(&format!(
                "[{reg_name}] upload completed in {:.2}s",
                t_up.elapsed().as_secs_f64()
            ));
            Ok(())
        }));
    }

    if let Some(prev) = prev_upload_task.take() {
        prev.await.context("join final upload task")??;
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
        keys.sort_unstable();
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
