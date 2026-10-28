//! server: control API for the wifiopt optimizer (replaces the old Next.js UI).
//!
//! std-only HTTP (no web framework). JSON everywhere, even for errors.
//!
//! Endpoints:
//!   GET  /                        index of endpoints
//!   GET  /api/health              binary, out dir, cron, jobs, recent errors, db ping
//!   GET  /api/runs                csv outputs + summary.json + jobs
//!   POST /api/runs                {date, regs, stale} -> start optimizer job (202)
//!   GET  /api/jobs/:id            job status + last 60 log lines
//!   GET  /api/options             available dates+regs (mysql, disk fallback)
//!   GET  /api/trends?days=N       daily aggregates, ours (disk) vs production (mysql)
//!   GET  /api/compare?date=YYYYMMDD  per-reg ours-vs-mysql aggregates
//!   GET  /api/cron                installed wifiopt cron line
//!   PUT  /api/cron                {minute, hour}
//!   GET  /api/settings            mysql pointing (password redacted)
//!   PUT  /api/settings            update mysql pointing

use anyhow::Result;
use chrono::{Datelike, Local, Timelike};
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Cfg {
    pub port: u16,
    pub out_dir: PathBuf,
    pub bin: PathBuf,
    pub sched_hour: u32,
    pub sched_minute: u32,
    pub sched_stale: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub hour: u32,
    pub minute: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScheduledRun {
    date: String,
    #[serde(rename = "jobId")]
    job_id: String,
}

fn env(k: &str, dflt: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| dflt.to_string())
}

/// Uniform true/false parsing ("true"/"1"/"yes", case-insensitive).
fn env_bool(k: &str, dflt: bool) -> bool {
    match std::env::var(k) {
        Ok(v) => matches!(v.trim().to_lowercase().as_str(), "true" | "1" | "yes"),
        Err(_) => dflt,
    }
}

pub fn load_cfg() -> Cfg {
    Cfg {
        port: env("PORT", "3000").parse().unwrap_or(3000),
        out_dir: PathBuf::from(env("WIFIPT_OUT", "/root/wifiopt/out")),
        bin: PathBuf::from(env("WIFIPT_BIN", "/root/wifiopt/target/release/wifiopt")),
        sched_hour: env("SCHEDULE_HOUR", "20").parse().unwrap_or(20),
        sched_minute: env("SCHEDULE_MINUTE", "0").parse().unwrap_or(0),
        sched_stale: env("SCHEDULE_STALE", "50").parse().unwrap_or(50),
    }
}

fn schedule_path(cfg: &Cfg) -> PathBuf {
    cfg.out_dir.join("schedule.json")
}

pub fn load_schedule(cfg: &Cfg) -> Schedule {
    if let Ok(s) = std::fs::read_to_string(schedule_path(cfg)) {
        if let Ok(v) = serde_json::from_str::<Schedule>(&s) {
            if v.hour <= 23 && v.minute <= 59 {
                return v;
            }
        }
    }
    Schedule {
        hour: cfg.sched_hour.min(23),
        minute: cfg.sched_minute.min(59),
    }
}

// ---------- mysql ----------

fn mysql_opts() -> Option<mysql::Opts> {
    let user = std::env::var("MYSQL_USER").ok()?;
    if user.is_empty() {
        return None;
    }
    let opts = mysql::OptsBuilder::new()
        .ip_or_hostname(Some(env("MYSQL_HOST", "192.168.41.115")))
        .tcp_port(env("MYSQL_PORT", "3306").parse().unwrap_or(3306))
        .user(Some(user))
        .pass(std::env::var("MYSQL_PASSWORD").ok())
        .db_name(Some(env("MYSQL_DATABASE", "wifi_optimization")));
    Some(mysql::Opts::from(opts))
}

fn mysql_conn() -> Result<mysql::PooledConn> {
    let opts = mysql_opts().ok_or_else(|| anyhow::anyhow!("MYSQL_USER not set"))?;
    let pool = mysql::Pool::new(opts)?;
    Ok(pool.get_conn()?)
}

// ---------- jobs ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    id: String,
    date: String,
    regs: Vec<String>,
    stale: usize,
    #[serde(default)]
    upload: bool,
    #[serde(default)]
    bruteforce: bool,
    status: String, // running | done | failed
    #[serde(rename = "startedAt")]
    started_at: String,
    #[serde(rename = "finishedAt")]
    finished_at: Option<String>,
}

pub struct State {
    pub cfg: Cfg,
    pub jobs: Mutex<HashMap<String, Job>>,
    pub schedule: Mutex<Schedule>,
    pub last_scheduled: Mutex<Option<ScheduledRun>>,
    pub file_cache: Mutex<HashMap<String, FileCache>>,
}

fn all_regs() -> Vec<String> {
    (1..=7).map(|i| format!("reg{i}")).collect()
}

fn next_run_iso(sched: &Schedule) -> String {
    let now = Local::now();
    let mut day = now.date_naive();
    let today_run = day
        .and_hms_opt(sched.hour, sched.minute, 0)
        .map(|t| t.and_local_timezone(Local).unwrap())
        .map(|t| t.to_utc());
    if today_run.is_some_and(|t| t <= chrono::Utc::now()) {
        day = day.checked_add_days(chrono::Days::new(1)).unwrap_or(day);
    }
    day.and_hms_opt(sched.hour, sched.minute, 0)
        .map(|t| t.and_local_timezone(Local).unwrap())
        .map(|t| t.to_utc().to_rfc3339())
        .unwrap_or_default()
}

// Internal daily scheduler: replaces the Linux crontab entry.
// Ticks every 20s; fires once per day inside the scheduled minute.
// No catch-up: if the process was down at 20:00, that day is skipped.
pub fn spawn_scheduler(state: Arc<State>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(20));
        let now = Local::now();
        let sched = state.schedule.lock().unwrap().clone();
        if now.hour() != sched.hour || now.minute() != sched.minute {
            continue;
        }
        let today = now.format("%Y%m%d").to_string();
        {
            let last = state.last_scheduled.lock().unwrap();
            if last.as_ref().is_some_and(|r| r.date == today) {
                continue;
            }
        }
        let stale = state.cfg.sched_stale;
        let upload = env_bool("SCHEDULE_UPLOAD", false);
        let bruteforce = env_bool("SCHEDULE_BRUTEFORCE", false);
        match start_job(&state, &today, &all_regs(), stale, upload, bruteforce) {
            Ok(j) => {
                println!("[scheduler] daily run started: {}", j.id);
                *state.last_scheduled.lock().unwrap() = Some(ScheduledRun {
                    date: today,
                    job_id: j.id,
                });
            }
            Err(e) => eprintln!("[scheduler] daily run failed to start: {e:#}"),
        }
    });
}

fn jobs_dir(cfg: &Cfg) -> PathBuf {
    cfg.out_dir.join("jobs")
}

fn read_job_file(cfg: &Cfg, id: &str) -> Option<Job> {
    if id.contains('/') || id.contains('\\') || id.contains('.') {
        return None;
    }
    let p = jobs_dir(cfg).join(format!("{id}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn persist_job(cfg: &Cfg, job: &Job) {
    let _ = std::fs::create_dir_all(jobs_dir(cfg));
    let p = jobs_dir(cfg).join(format!("{}.json", job.id));
    if let Ok(s) = serde_json::to_string(job) {
        let _ = std::fs::write(p, s);
    }
}

fn list_jobs(state: &Arc<State>) -> Vec<Job> {
    let mut out: Vec<Job> = state.jobs.lock().unwrap().values().cloned().collect();
    if let Ok(entries) = std::fs::read_dir(jobs_dir(&state.cfg)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") {
                continue;
            }
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                if let Ok(j) = serde_json::from_str::<Job>(&s) {
                    if !out.iter().any(|x| x.id == j.id) {
                        out.push(j);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}

fn get_job(state: &Arc<State>, id: &str) -> Option<Job> {
    if let Some(j) = state.jobs.lock().unwrap().get(id) {
        return Some(j.clone());
    }
    read_job_file(&state.cfg, id)
}

fn valid_date(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
}

fn valid_reg(s: &str) -> bool {
    s.starts_with("reg")
        && s.len() > 3
        && s[3..].bytes().all(|b| b.is_ascii_digit())
}

pub fn start_job(
    state: &Arc<State>,
    date: &str,
    regs: &[String],
    stale: usize,
    upload: bool,
    bruteforce: bool,
) -> Result<Job> {
    if !valid_date(date) {
        anyhow::bail!("bad date");
    }
    if regs.is_empty() || !regs.iter().all(|r| valid_reg(r)) {
        anyhow::bail!("bad regs");
    }
    let id = format!(
        "{date}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string()
    );
    let _ = std::fs::create_dir_all(jobs_dir(&state.cfg));
    let log_path = jobs_dir(&state.cfg).join(format!("{id}.log"));
    let job = Job {
        id: id.clone(),
        date: date.to_string(),
        regs: regs.to_vec(),
        stale,
        upload,
        bruteforce,
        status: "running".to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
    };
    state.jobs.lock().unwrap().insert(id.clone(), job.clone());
    persist_job(&state.cfg, &job);

    let log_file = std::fs::File::create(&log_path)?;
    let log_err = log_file.try_clone()?;
    let bin = state.cfg.bin.clone();
    let out = state.cfg.out_dir.clone();
    let regs_arg = regs.join(",");
    let jid = job.id.clone();
    let jdate = job.date.clone();
    let jstale = job.stale;
    let jupload = job.upload;
    let jbruteforce = job.bruteforce;
    let st = Arc::clone(state);
    std::thread::spawn(move || {
        let mut cmd = Command::new(&bin);
        // Mirror the old launcher: derive DATABASE_URL from MYSQL_* when unset.
        if std::env::var("DATABASE_URL").is_err() {
            if let Ok(user) = std::env::var("MYSQL_USER") {
                if !user.is_empty() {
                    let url = format!(
                        "mysql://{}:{}@{}:{}/{}",
                        user,
                        std::env::var("MYSQL_PASSWORD").unwrap_or_default(),
                        env("MYSQL_HOST", "192.168.41.115"),
                        env("MYSQL_PORT", "3306"),
                        env("MYSQL_DATABASE", "wifi_optimization")
                    );
                    cmd.env("DATABASE_URL", url);
                }
            }
        }
        let mut args = vec![
            "--date".to_string(),
            jdate,
            "--reg".to_string(),
            regs_arg,
            "--out".to_string(),
            out.to_string_lossy().to_string(),
            "--stale".to_string(),
            jstale.to_string(),
        ];
        if jupload {
            args.push("--upload".to_string());
        }
        if jbruteforce {
            args.push("--bruteforce".to_string());
        }
        let res = cmd
            .args(&args)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err))
            .status();
        let mut jobs = st.jobs.lock().unwrap();
        if let Some(j) = jobs.get_mut(&jid) {
            j.status = if res.map(|s| s.success()).unwrap_or(false) {
                "done"
            } else {
                "failed"
            }
            .to_string();
            j.finished_at = Some(chrono::Utc::now().to_rfc3339());
            persist_job(&st.cfg, j);
        }
    });
    Ok(job)
}

// ---------- disk outputs ----------

fn parse_csv_name(name: &str) -> Option<(String, String)> {
    let rest = name
        .strip_prefix("wifi_optimize_")?
        .strip_suffix(".csv")?;
    let (date, reg) = rest.rsplit_once('_')?;
    if !valid_date(date) || !valid_reg(reg) {
        return None;
    }
    Some((date.to_string(), reg.to_string()))
}

#[derive(Clone)]
pub struct FileCache {
    mtime: u64,
    len: u64,
    rows: usize,
    agg: Agg,
}

fn file_key(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((mtime, meta.len()))
}

/// One streaming pass: row count + aggregates. Never loads the whole file,
/// so /api/runs and /api/trends stay cheap with hundreds of 100MB CSVs.
fn scan_csv(path: &Path) -> Option<(usize, Agg)> {
    let f = std::fs::File::open(path).ok()?;
    let mut r = std::io::BufReader::new(f);
    let mut a = Agg::default();
    let mut data_lines = 0usize;
    use std::io::BufRead as _;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            break;
        }
        let t = line.trim_end();
        if t.is_empty() || t.starts_with("reg,") {
            continue;
        }
        data_lines += 1;
        let v: Vec<&str> = t.split(',').collect();
        if v.len() < 14 {
            continue;
        }
        a.n += 1;
        // header order: reg,sn,clusterid_2g,ch_before_2g,cost_before_2g,ch_after_2g,
        // cost_after_2g,status_2g,clusterid_5g,ch_before_5g,cost_before_5g,
        // ch_after_5g,cost_after_5g,status_5g,...
        if let Some(x) = num(v[4]) {
            a.cb2 += x;
            a.c2 += 1;
        }
        if let Some(x) = num(v[6]) {
            a.ca2 += x;
        }
        if let Some(x) = num(v[10]) {
            a.cb5 += x;
            a.c5 += 1;
        }
        if let Some(x) = num(v[12]) {
            a.ca5 += x;
        }
        if v[7] == "CHANGE" {
            a.ch2 += 1;
        }
        if v[13] == "CHANGE" {
            a.ch5 += 1;
        }
    }
    Some((data_lines, a))
}

/// mtime+len keyed cache: re-scan only when the file actually changed.
fn cached_scan(state: &Arc<State>, path: &Path) -> Option<(usize, Agg)> {
    let meta = std::fs::metadata(path).ok()?;
    let (mtime, len) = file_key(&meta)?;
    let key = path.to_string_lossy().to_string();
    if let Some(hit) = state.file_cache.lock().unwrap().get(&key) {
        if hit.mtime == mtime && hit.len == len {
            return Some((hit.rows, hit.agg.clone()));
        }
    }
    let (rows, agg) = scan_csv(path)?;
    state.file_cache.lock().unwrap().insert(
        key,
        FileCache {
            mtime,
            len,
            rows,
            agg: agg.clone(),
        },
    );
    Some((rows, agg))
}

fn iso_of(m: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(m).to_rfc3339()
}

fn list_runs(state: &Arc<State>) -> Vec<serde_json::Value> {
    let cfg = &state.cfg;
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cfg.out_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some((date, reg)) = parse_csv_name(&name) {
                let (bytes, mtime) = e
                    .metadata()
                    .map(|m| {
                        (
                            m.len(),
                            m.modified().map(iso_of).unwrap_or_default(),
                        )
                    })
                    .unwrap_or((0, String::new()));
                let rows = cached_scan(state, &e.path()).map(|(n, _)| n);
                out.push(serde_json::json!({
                    "date": date, "reg": reg,
                    "rows": rows,
                    "bytes": bytes, "mtime": mtime,
                }));
            }
        }
    }
    out.sort_by(|a, b| {
        let (da, ra) = (a["date"].as_str(), a["reg"].as_str());
        let (db, rb) = (b["date"].as_str(), b["reg"].as_str());
        db.cmp(&da).then(ra.cmp(&rb))
    });
    out
}

fn read_summary(cfg: &Cfg) -> serde_json::Value {
    std::fs::read_to_string(cfg.out_dir.join("summary.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

// ---------- trends / compare ----------

fn day_list(days: usize) -> Vec<String> {
    let n = days.clamp(1, 62);
    let today = Local::now().date_naive();
    (0..n)
        .map(|i| {
            let d = today - chrono::Days::new(i as u64);
            format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
        })
        .collect()
}

#[derive(Default, Clone)]
struct Agg {
    n: u64,
    cb2: f64,
    ca2: f64,
    c2: u64,
    cb5: f64,
    ca5: f64,
    c5: u64,
    ch2: u64,
    ch5: u64,
}

fn num(s: &str) -> Option<f64> {
    if s.is_empty() {
        None
    } else {
        s.parse::<f64>().ok()
    }
}

fn agg_json(date: &str, a: &Agg) -> serde_json::Value {
    let c2 = a.c2.max(1) as f64;
    let c5 = a.c5.max(1) as f64;
    serde_json::json!({
        "date": date, "n": a.n,
        "cb2": a.cb2 / c2, "ca2": a.ca2 / c2,
        "cb5": a.cb5 / c5, "ca5": a.ca5 / c5,
        "ch2": a.ch2, "ch5": a.ch5,
    })
}

fn trends_ours(state: &Arc<State>, dates: &[String]) -> Vec<serde_json::Value> {
    let cfg = &state.cfg;
    let mut by_date: HashMap<String, Agg> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&cfg.out_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some((date, _)) = parse_csv_name(&name) {
                if !dates.contains(&date) {
                    continue;
                }
                let Some((_, a)) = cached_scan(state, &e.path()) else {
                    continue;
                };
                let t = by_date.entry(date).or_default();
                t.n += a.n;
                t.cb2 += a.cb2;
                t.ca2 += a.ca2;
                t.c2 += a.c2;
                t.cb5 += a.cb5;
                t.ca5 += a.ca5;
                t.c5 += a.c5;
                t.ch2 += a.ch2;
                t.ch5 += a.ch5;
            }
        }
    }
    let mut v: Vec<_> = by_date.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.into_iter().map(|(d, a)| agg_json(&d, &a)).collect()
}

fn mysql_tables_like(conn: &mut mysql::PooledConn, pat: &str) -> Vec<String> {
    conn.query_map(
        "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME LIKE '".to_string()
            + pat
            + "'",
        |t: String| t,
    )
    .unwrap_or_default()
}

fn trends_mysql(dates: &[String]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let Ok(mut conn) = mysql_conn() else {
        return out;
    };
    let have: std::collections::HashSet<String> =
        mysql_tables_like(&mut conn, "wifi_optimize\\_%").into_iter().collect();
    for d in dates {
        if !have.contains(&format!("wifi_optimize_{d}")) {
            continue;
        }
        let sql = format!(
            "SELECT COUNT(*), AVG(cost_before_2g), AVG(cost_after_2g), AVG(cost_before_5g), AVG(cost_after_5g), SUM(status_2g='CHANGE'), SUM(status_5g='CHANGE') FROM wifi_optimize_{d}"
        );
        let row: Option<(
            u64,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
        )> = conn.query_first(&sql).unwrap_or(None);
        if let Some((n, cb2, ca2, cb5, ca5, ch2, ch5)) = row {
            out.push(serde_json::json!({
                "date": d, "n": n,
                "cb2": cb2.unwrap_or(0.0), "ca2": ca2.unwrap_or(0.0),
                "cb5": cb5.unwrap_or(0.0), "ca5": ca5.unwrap_or(0.0),
                "ch2": ch2.unwrap_or(0.0), "ch5": ch5.unwrap_or(0.0),
            }));
        }
    }
    out.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));
    out
}

fn compare_mysql(date: &str, regs: &[&str]) -> (serde_json::Value, Option<String>) {
    let Ok(mut conn) = mysql_conn() else {
        return (serde_json::Value::Null, Some("mysql not configured".into()));
    };
    let mut out = serde_json::Map::new();
    for reg in regs {
        let sql = format!(
            "SELECT COUNT(*), AVG(cost_before_2g), AVG(cost_after_2g), AVG(cost_before_5g), AVG(cost_after_5g), SUM(status_2g='CHANGE'), SUM(status_5g='CHANGE') FROM wifi_optimize_{date} WHERE reg='{reg}'"
        );
        let row: Option<(
            u64,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
        )> = conn.query_first(&sql).unwrap_or(None);
        out.insert(
            reg.to_string(),
            match row {
                Some((n, cb2, ca2, cb5, ca5, ch2, ch5)) => serde_json::json!({
                    "n": n, "cb2": cb2, "ca2": ca2, "cb5": cb5, "ca5": ca5,
                    "ch2": ch2, "ch5": ch5,
                }),
                None => serde_json::Value::Null,
            },
        );
    }
    (serde_json::Value::Object(out), None)
}

// ---------- schedule (internal daily run, no system cron) ----------

fn persist_schedule(cfg: &Cfg, sched: &Schedule) {
    let _ = std::fs::create_dir_all(&cfg.out_dir);
    if let Ok(s) = serde_json::to_string(sched) {
        let _ = std::fs::write(schedule_path(cfg), s);
    }
}

// ---------- settings (.env) ----------

fn parse_env_file(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.split('\n') {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim();
                // accept dotenv-style quoting so bash can source the same file
                let v = if v.len() >= 2
                    && ((v.starts_with('\'') && v.ends_with('\''))
                        || (v.starts_with('"') && v.ends_with('"')))
                {
                    &v[1..v.len() - 1]
                } else {
                    v
                };
                out.insert(k.trim().to_string(), v.to_string());
            }
        }
    }
    out
}

fn env_get(map: &HashMap<String, String>, k: &str) -> String {
    map.get(k)
        .cloned()
        .or_else(|| std::env::var(k).ok())
        .unwrap_or_default()
}

// ---------- health ----------

fn latest_cron_errors(cfg: &Cfg) -> Vec<String> {
    let mut logs: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cfg.out_dir) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("cron-") && n.ends_with(".log") {
                logs.push(e.path());
            }
        }
    }
    logs.sort();
    let Some(latest) = logs.last() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(latest) else {
        return Vec::new();
    };
    text.split('\n')
        .rev()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && (t.contains("fail")
                    || t.contains("error")
                    || t.contains("panic")
                    || t.contains("exit=1"))
        })
        .take(10)
        .map(|l| l.to_string())
        .collect()
}

fn health(state: &Arc<State>) -> serde_json::Value {
    let cfg = &state.cfg;
    let bin_meta = std::fs::metadata(&cfg.bin).ok();
    let (bin_bytes, bin_mtime) = bin_meta
        .as_ref()
        .map(|m| {
            (
                m.len(),
                m.modified().ok().map(iso_of).unwrap_or_default(),
            )
        })
        .unwrap_or((0, String::new()));
    let runs = list_runs(state);
    let csv_count = runs.len();
    let latest_date = runs.first().and_then(|r| r["date"].as_str()).unwrap_or("");
    let total_bytes: u64 = runs.iter().filter_map(|r| r["bytes"].as_u64()).sum();
    let jobs = list_jobs(state);
    let failed_jobs: Vec<_> = jobs
        .iter()
        .filter(|j| j.status == "failed")
        .take(5)
        .map(|j| serde_json::json!({"id": j.id, "date": j.date, "startedAt": j.started_at, "finishedAt": j.finished_at}))
        .collect();
    let mut tally = HashMap::new();
    for j in &jobs {
        *tally.entry(j.status.clone()).or_insert(0u32) += 1;
    }
    let (db_status, db_ms) = match mysql_opts() {
        None => ("not configured".to_string(), 0),
        Some(_) => {
            let t = Instant::now();
            match mysql_conn().and_then(|mut c| {
                c.query_drop("SELECT 1")?;
                Ok(())
            }) {
                Ok(()) => ("ok".to_string(), t.elapsed().as_millis() as u64),
                Err(e) => (format!("error: {e:#}"), t.elapsed().as_millis() as u64),
            }
        }
    };
    let cron_errors = latest_cron_errors(cfg);
    let ok = bin_meta.is_some()
        && db_status == "ok"
        && failed_jobs.is_empty()
        && cron_errors.is_empty();
    serde_json::json!({
        "ok": ok,
        "time": chrono::Utc::now().to_rfc3339(),
        "bin": {
            "exists": bin_meta.is_some(),
            "bytes": bin_bytes,
            "mtime": bin_mtime,
            "path": cfg.bin.to_string_lossy(),
        },
        "out": {
            "exists": cfg.out_dir.exists(),
            "dir": cfg.out_dir.to_string_lossy(),
            "csvCount": csv_count,
            "latestDate": latest_date,
            "totalBytes": total_bytes,
            "summary": read_summary(cfg),
        },
        "cron": {
            "source": "internal",
            "schedule": state.schedule.lock().unwrap().clone(),
            "nextRun": next_run_iso(&state.schedule.lock().unwrap().clone()),
            "lastScheduled": state.last_scheduled.lock().unwrap().clone(),
        },
        "jobs": { "tally": tally, "total": jobs.len(), "failed": failed_jobs },
        "errors": cron_errors,
        "db": { "status": db_status, "latencyMs": db_ms },
    })
}

// ---------- http ----------

pub struct Req {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: String,
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for kv in q.split('&') {
        if kv.is_empty() {
            continue;
        }
        match kv.split_once('=') {
            Some((k, v)) => {
                m.insert(k.to_string(), v.to_string());
            }
            None => {
                m.insert(kv.to_string(), String::new());
            }
        }
    }
    m
}

pub fn read_req(stream: &TcpStream) -> Result<Req> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let parts: Vec<&str> = line.trim_end().split(' ').collect();
    if parts.len() < 2 {
        anyhow::bail!("bad request line");
    }
    let method = parts[0].to_string();
    let target = parts[1].to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        r.read_line(&mut h)?;
        let t = h.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len.min(4_000_000)];
    if !body.is_empty() {
        r.read_exact(&mut body)?;
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target, HashMap::new()),
    };
    let path = if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    };
    Ok(Req {
        method,
        path,
        query,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

pub fn respond(stream: &mut TcpStream, status: u16, text: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} {text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn ok(v: &serde_json::Value) -> (u16, String) {
    (200, serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
}

fn err(status: u16, msg: String) -> (u16, String) {
    (
        status,
        serde_json::json!({"error": msg}).to_string(),
    )
}

const INDEX: &str = r#"{"service":"server","ui":false,"endpoints":[
{"method":"GET","path":"/","desc":"this index"},
{"method":"GET","path":"/api/health","desc":"binary, out dir, cron, jobs, recent errors, db ping"},
{"method":"GET","path":"/api/runs","desc":"csv outputs, summary.json, jobs"},
{"method":"POST","path":"/api/runs","desc":"start job {date:YYYYMMDD, regs:[regN], stale, upload:bool, bruteforce:bool}"},
{"method":"GET","path":"/api/jobs/:id","desc":"job status + last 60 log lines"},
{"method":"GET","path":"/api/options","desc":"available dates+regs (mysql, disk fallback)"},
{"method":"GET","path":"/api/trends?days=N","desc":"daily aggregates, ours vs mysql"},
{"method":"GET","path":"/api/compare?date=YYYYMMDD","desc":"per-reg ours-vs-mysql aggregates"},
{"method":"GET","path":"/api/cron","desc":"internal daily schedule + nextRun + lastScheduled"},
{"method":"PUT","path":"/api/cron","desc":"set internal schedule {minute, hour}"},
{"method":"GET","path":"/api/settings","desc":"mysql pointing (password redacted)"},
{"method":"PUT","path":"/api/settings","desc":"update mysql pointing"}
]}"#;

pub fn route(state: &Arc<State>, req: &Req) -> (u16, String) {
    let cfg = &state.cfg;
    let seg: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    match (req.method.as_str(), seg.as_slice()) {
        ("GET", []) => (200, INDEX.to_string()),
        ("GET", ["api", "health"]) => ok(&health(state)),
        ("GET", ["api", "runs"]) => ok(&serde_json::json!({
            "runs": list_runs(state),
            "summary": read_summary(cfg),
            "jobs": list_jobs(state),
        })),
        ("POST", ["api", "runs"]) => {
            let v: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            let date = v["date"].as_str().map(|s| s.to_string()).unwrap_or_else(|| {
                Local::now().format("%Y%m%d").to_string()
            });
            let regs: Vec<String> = v["regs"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| (1..=7).map(|i| format!("reg{i}")).collect());
            let stale = v["stale"].as_u64().unwrap_or(50) as usize;
            let upload = v["upload"].as_bool().unwrap_or(false);
            let bruteforce = v["bruteforce"].as_bool().unwrap_or(false);
            match start_job(state, &date, &regs, stale, upload, bruteforce) {
                Ok(j) => (202, serde_json::json!({"job": j}).to_string()),
                Err(e) => err(400, format!("{e:#}")),
            }
        }
        ("GET", ["api", "jobs", id]) => match get_job(state, id) {
            None => err(404, "no job".into()),
            Some(job) => {
                let log_path = jobs_dir(cfg).join(format!("{id}.log"));
                let log = std::fs::read_to_string(&log_path)
                    .map(|s| {
                        let lines: Vec<&str> = s.split('\n').collect();
                        let n = lines.len().min(60);
                        lines[lines.len() - n..].join("\n")
                    })
                    .unwrap_or_default();
                ok(&serde_json::json!({"job": job, "log": log}))
            }
        },
        ("GET", ["api", "options"]) => {
            let mut by_date: HashMap<String, Vec<String>> = HashMap::new();
            let mut from_mysql = false;
            if let Ok(mut conn) = mysql_conn() {
                let tables = mysql_tables_like(&mut conn, "discover_interference_reborn\\_%");
                for t in tables {
                    if let Some(rest) = t.strip_prefix("discover_interference_reborn_") {
                        if let Some((date, reg)) = rest.rsplit_once('_') {
                            if valid_date(date) && valid_reg(reg) && reg != "template" {
                                by_date.entry(date.to_string()).or_default().push(reg.to_string());
                                from_mysql = true;
                            }
                        }
                    }
                }
            }
            if !from_mysql {
                for r in list_runs(state) {
                    if let (Some(d), Some(g)) = (r["date"].as_str(), r["reg"].as_str()) {
                        by_date.entry(d.to_string()).or_default().push(g.to_string());
                    }
                }
            }
            let mut v: Vec<serde_json::Value> = by_date
                .into_iter()
                .map(|(date, mut regs)| {
                    regs.sort();
                    regs.dedup();
                    serde_json::json!({"date": date, "regs": regs})
                })
                .collect();
            v.sort_by(|a, b| b["date"].as_str().cmp(&a["date"].as_str()));
            ok(&serde_json::json!(v))
        }
        ("GET", ["api", "trends"]) => {
            let days = req
                .query
                .get("days")
                .and_then(|s| s.parse().ok())
                .unwrap_or(14usize);
            let dates = day_list(days);
            ok(&serde_json::json!({
                "ours": trends_ours(state, &dates),
                "mysql": trends_mysql(&dates),
            }))
        }
        ("GET", ["api", "compare"]) => {
            let date: String = req
                .query
                .get("date")
                .map(|s| s.chars().filter(|c| c.is_ascii_digit()).collect())
                .unwrap_or_default();
            if !valid_date(&date) {
                return err(400, "bad date, use YYYYMMDD".into());
            }
            let regs = ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];
            let mut ours = serde_json::Map::new();
            for reg in regs {
                let p = cfg.out_dir.join(format!("wifi_optimize_{date}_{reg}.csv"));
                ours.insert(
                    reg.to_string(),
                    if p.exists() {
                        match cached_scan(state, &p) {
                            Some((_, a)) => agg_json(&date, &a),
                            None => serde_json::Value::Null,
                        }
                    } else {
                        serde_json::Value::Null
                    },
                );
            }
            let (mysql, mysql_error) = compare_mysql(&date, &regs);
            ok(&serde_json::json!({
                "date": date,
                "ours": ours,
                "mysql": mysql,
                "mysqlError": mysql_error,
            }))
        }
        ("GET", ["api", "cron"]) => {
            let sched = state.schedule.lock().unwrap().clone();
            ok(&serde_json::json!({
                "source": "internal",
                "hour": sched.hour, "minute": sched.minute,
                "nextRun": next_run_iso(&sched),
                "lastScheduled": state.last_scheduled.lock().unwrap().clone(),
            }))
        }
        ("PUT", ["api", "cron"]) => {
            let v: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            let minute = v["minute"].as_u64().unwrap_or(30);
            let hour = v["hour"].as_u64().unwrap_or(5);
            if minute > 59 || hour > 23 {
                return err(400, "bad time".into());
            }
            let sched = Schedule { hour: hour as u32, minute: minute as u32 };
            persist_schedule(cfg, &sched);
            *state.schedule.lock().unwrap() = sched.clone();
            ok(&serde_json::json!({
                "source": "internal",
                "hour": sched.hour, "minute": sched.minute,
                "nextRun": next_run_iso(&sched),
            }))
        }
        ("GET", ["api", "settings"]) => {
            let f = parse_env_file(Path::new(".env"));
            ok(&serde_json::json!({
                "host": if env_get(&f, "MYSQL_HOST").is_empty() { "192.168.41.115".into() } else { env_get(&f, "MYSQL_HOST") },
                "port": if env_get(&f, "MYSQL_PORT").is_empty() { "3306".into() } else { env_get(&f, "MYSQL_PORT") },
                "user": env_get(&f, "MYSQL_USER"),
                "database": if env_get(&f, "MYSQL_DATABASE").is_empty() { "wifi_optimization".into() } else { env_get(&f, "MYSQL_DATABASE") },
                "hasPassword": !env_get(&f, "MYSQL_PASSWORD").is_empty(),
            }))
        }
        ("PUT", ["api", "settings"]) => {
            let v: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            let host = v["host"].as_str().unwrap_or("").trim().to_string();
            let port = v["port"].as_str().unwrap_or("").trim().to_string();
            let user = v["user"].as_str().unwrap_or("").trim().to_string();
            let database = v["database"].as_str().unwrap_or("").trim().to_string();
            if host.is_empty() || user.is_empty() || database.is_empty() {
                return err(400, "host, user, database required".into());
            }
            if port.parse::<u16>().is_err() {
                return err(400, "bad port".into());
            }
            let mut f = parse_env_file(Path::new(".env"));
            f.insert("MYSQL_HOST".into(), host);
            f.insert("MYSQL_PORT".into(), port);
            f.insert("MYSQL_USER".into(), user);
            f.insert("MYSQL_DATABASE".into(), database);
            if let Some(p) = v["password"].as_str() {
                if !p.is_empty() {
                    f.insert("MYSQL_PASSWORD".into(), p.to_string());
                }
            }
            let mut keys: Vec<_> = f.keys().cloned().collect();
            keys.sort();
            let body = keys
                .iter()
                .map(|k| format!("{k}={}", f[k]))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            match std::fs::write(Path::new(".env"), body) {
                Ok(()) => {
                    // best-effort restart like before; caller retries on new port if changed
                    std::thread::spawn(|| {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let _ = Command::new("sh")
                            .args(["-c", "systemctl restart server"])
                            .output();
                    });
                    ok(&serde_json::json!({"ok": true, "restartRequired": true}))
                }
                Err(e) => err(500, format!("{e:#}")),
            }
        }
        _ => err(404, "not found".into()),
    }
}
