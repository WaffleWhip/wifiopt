use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::config::Options;
use crate::cost::{self, OptimizeRow};
use crate::db;
use crate::discover::{self, ApInfo};
use crate::graph;
use crate::output::{self, OutputRow, Summary};
use crate::upload;

pub fn ts() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

const PLAN_2G: [u8; 3] = [1, 6, 11];
const PLAN_5G: [u8; 9] = [36, 40, 44, 48, 149, 153, 157, 161, 165];

/// Reject a plan that would leave a device on an unusable channel (off-plan
/// 2.4 GHz, DFS 5 GHz, or channel 0). The upload path truncates the day table
/// before writing, so a bad plan must never reach MySQL.
pub fn validate_plan(reg: &str, rows: &[OutputRow]) -> Result<()> {
    if rows.is_empty() {
        anyhow::bail!("{reg}: empty plan");
    }
    let mut bad2 = 0usize;
    let mut bad5 = 0usize;
    let mut first = String::new();
    for r in rows {
        let ok2 = r.ch_after_2g.is_some_and(|c| PLAN_2G.contains(&c));
        let ok5 = r.ch_after_5g.is_none_or(|c| PLAN_5G.contains(&c));
        if !ok2 || !ok5 {
            if !ok2 {
                bad2 += 1;
            }
            if !ok5 {
                bad5 += 1;
            }
            if first.is_empty() {
                first = format!(
                    "sn={} ch_after_2g={:?} ch_after_5g={:?}",
                    r.sn, r.ch_after_2g, r.ch_after_5g
                );
            }
        }
    }
    if bad2 > 0 || bad5 > 0 {
        anyhow::bail!(
            "{reg}: off_plan_2g={bad2} bad_5g={bad5} of {} rows, first {first}",
            rows.len()
        );
    }
    Ok(())
}

pub fn optimize(reg: &str, aps: Vec<ApInfo>, stale: usize, max_k: usize) -> Vec<OutputRow> {
    let updated_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let mem2 = graph::mem_rows_for(&aps, "2g");
    let mem5 = graph::mem_rows_for(&aps, "5g");
    let div2 = aps
        .iter()
        .filter(|a| discover::divergent_radios(a, "2g"))
        .count();
    let div5 = aps
        .iter()
        .filter(|a| discover::divergent_radios(a, "5g"))
        .count();
    println!(
        "[{}] {reg}: multiradio_divergent_2g={div2} 5g={div5} of {}",
        ts(),
        aps.len()
    );
    let t_clu = std::time::Instant::now();
    let c2 = graph::cluster(&aps, "2g", 2).map(|c| c.valid).unwrap_or_default();
    let c5 = graph::cluster(&aps, "5g", 2).map(|c| c.valid).unwrap_or_default();
    drop(aps);
    println!(
        "[{}] {reg}: cluster took {:.2}s",
        ts(),
        t_clu.elapsed().as_secs_f64()
    );

    let t_opt = std::time::Instant::now();
    let (r2, s2) = cost::optimize_band_bruteforce(&mem2, &c2, stale, max_k, "2g");
    // Only APs that get a 2G row are actually executed downstream. Pinning
    // the rest keeps the 5G plan consistent with what will really happen.
    let executable: std::collections::HashSet<String> =
        r2.iter().map(|r| r.sn.clone()).collect();
    let (r5, s5) =
        cost::optimize_band_bruteforce_opt(&mem5, &c5, stale, max_k, "5g", Some(&executable));
    println!(
        "[{}] {reg}: bruteforce 2g groups={} exhaustive={} fallback={} | 5g groups={} exhaustive={} fallback={}",
        ts(),
        s2.groups,
        s2.exhaustive,
        s2.fallback,
        s5.groups,
        s5.exhaustive,
        s5.fallback
    );
    println!(
        "[{}] {reg}: optimize took {:.2}s",
        ts(),
        t_opt.elapsed().as_secs_f64()
    );

    let mut map5: HashMap<String, OptimizeRow> = HashMap::new();
    for r in r5 {
        map5.insert(r.sn.clone(), r);
    }

    let mut rows: Vec<OutputRow> = r2
        .into_iter()
        .map(|a| {
            let b = map5.remove(&a.sn);
            OutputRow {
                reg: reg.to_string(),
                sn: a.sn.clone(),
                clusterid_2g: a.clusterid,
                ch_before_2g: a.ch_before.map(|c| c as u8),
                cost_before_2g: Some(a.cost_before),
                ch_after_2g: Some(a.ch_after as u8),
                cost_after_2g: Some(a.cost_after),
                status_2g: Some(a.status),
                clusterid_5g: b.as_ref().and_then(|x| x.clusterid),
                ch_before_5g: b.as_ref().and_then(|x| x.ch_before.map(|c| c as u8)),
                cost_before_5g: b.as_ref().map(|x| x.cost_before),
                ch_after_5g: b.as_ref().map(|x| x.ch_after as u8),
                cost_after_5g: b.as_ref().map(|x| x.cost_after),
                status_5g: b.as_ref().map(|x| x.status.clone()),
                updated_at: updated_at.clone(),
                node_id: None,
                node_ip: None,
                slot: None,
                port: None,
                onu_id: None,
                regional: None,
                witel: None,
                cpe_product_class: None,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.sn.cmp(&b.sn));
    rows
}

pub fn run(opts: &Options) -> Result<Summary> {
    std::fs::create_dir_all(&opts.out).context("out dir")?;
    // Preflight: a full-day upload truncates wifi_optimize_<date> before any
    // plan is written. Refuse to truncate when an input table is missing or
    // still empty (late ingest), so the previous table is not destroyed.
    if opts.upload && opts.regs.len() >= 7 {
        for reg in &opts.regs {
            let n = db::count_rows(opts, reg)
                .with_context(|| format!("preflight discover count {reg}"))?;
            if n == 0 {
                anyhow::bail!(
                    "preflight: discover_interference_reborn_{}_{reg} has 0 rows; refusing to truncate",
                    opts.date
                );
            }
            println!("[{}] preflight {reg}: {n} discover rows", ts());
        }
    }
    // Upload path mirrors production (ensure + truncate + per-reg batched
    // INSERT pipelined with the next region's compute). Dormant by default.
    let table = if opts.upload {
        let t = upload::ensure_table(&opts.db_url, &opts.date, &opts.table)?;
        println!("[{}] target table: {t}", ts());
        if opts.regs.len() >= 7 {
            upload::truncate_table(&opts.db_url, &t)?;
            println!("[{}] truncated {t}", ts());
        } else {
            println!("[{}] partial regs, truncate skipped", ts());
        }
        Some(t)
    } else {
        None
    };
    let mut total = 0;
    let mut failed = Vec::new();
    let mut prev_upload: Option<std::thread::JoinHandle<Result<usize>>> = None;
    for reg in &opts.regs {
        println!("[{}] start {reg} ...", ts());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut aps: Vec<ApInfo> = Vec::new();
        let t_fe = std::time::Instant::now();
        match db::stream_aps(opts, reg, |ap| aps.push(ap)) {
            Ok(n) => {
                println!(
                    "[{}] {reg}: fetch+parse {n} rows took {:.2}s",
                    ts(),
                    t_fe.elapsed().as_secs_f64()
                );
                let out = optimize(reg, aps, opts.stale, opts.bruteforce_max_k);
                total += out.len();
                let plan_ok = match validate_plan(reg, &out) {
                    Ok(()) => {
                        println!(
                            "[{}] {reg}: plan gate ok rows={} 5g_planned={}",
                            ts(),
                            out.len(),
                            out.iter().filter(|r| r.ch_after_5g.is_some()).count()
                        );
                        true
                    }
                    Err(e) => {
                        eprintln!("[{}] {reg}: plan gate rejected output: {e:#}", ts());
                        failed.push(reg.clone());
                        false
                    }
                };
                let path = format!("{}/wifi_optimize_{}_{}.csv", opts.out, opts.date, reg);
                let t_wr = std::time::Instant::now();
                if let Err(e) = output::write_csv(&path, &out) {
                    eprintln!("[{}] write {reg} failed: {e:#}", ts());
                    failed.push(reg.clone());
                } else {
                    println!(
                        "[{}] {reg}: {} rows -> {path} (write {:.2}s)",
                        ts(),
                        out.len(),
                        t_wr.elapsed().as_secs_f64()
                    );
                    if let Some(t) = table.clone().filter(|_| plan_ok) {
                        if let Some(prev) = prev_upload.take() {
                            match prev.join() {
                                Ok(Ok(n)) => println!("[{}] upload committed {n} rows", ts()),
                                Ok(Err(e)) => {
                                    eprintln!("[{}] previous upload failed: {e:#}", ts());
                                    failed.push(reg.clone());
                                }
                                Err(_) => {
                                    eprintln!("[{}] previous upload thread panicked", ts());
                                    failed.push(reg.clone());
                                }
                            }
                        }
                        let db_url = opts.db_url.clone();
                        let reg_name = reg.clone();
                        prev_upload = Some(std::thread::spawn(move || {
                            let t_up = std::time::Instant::now();
                            let n = upload::upload_rows(&db_url, &t, &out)?;
                            eprintln!(
                                "[upload {reg_name}] committed {n} rows in {:.2}s",
                                t_up.elapsed().as_secs_f64()
                            );
                            Ok(n)
                        }));
                    }
                }
            }
            Err(e) => {
                eprintln!("[{}] load {reg} failed: {e:#}", ts());
                failed.push(reg.clone());
            }
        }
    }
    if let Some(prev) = prev_upload.take() {
        match prev.join() {
            Ok(Ok(n)) => println!("[{}] upload committed {n} rows", ts()),
            Ok(Err(e)) => {
                eprintln!("[{}] final upload failed: {e:#}", ts());
                failed.push("upload".to_string());
            }
            Err(_) => {
                eprintln!("[{}] final upload thread panicked", ts());
                failed.push("upload".to_string());
            }
        }
    }
    let summary = Summary {
        date: opts.date.clone(),
        regs: opts.regs.clone(),
        rows: total,
    };
    let sp = format!("{}/summary.json", opts.out);
    output::write_summary(&sp, &summary)?;
    if !failed.is_empty() {
        anyhow::bail!("failed regions: {}", failed.join(","));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(sn: &str, ch2: u8, ch5: Option<u8>) -> OutputRow {
        OutputRow {
            reg: "reg1".into(),
            sn: sn.into(),
            clusterid_2g: None,
            ch_before_2g: Some(ch2),
            cost_before_2g: Some(0.0),
            ch_after_2g: Some(ch2),
            cost_after_2g: Some(0.0),
            status_2g: Some("STAY".into()),
            clusterid_5g: None,
            ch_before_5g: ch5,
            cost_before_5g: Some(0.0),
            ch_after_5g: ch5,
            cost_after_5g: Some(0.0),
            status_5g: Some("STAY".into()),
            updated_at: "2026-10-30 00:00:00".into(),
            node_id: None,
            node_ip: None,
            slot: None,
            port: None,
            onu_id: None,
            regional: None,
            witel: None,
            cpe_product_class: None,
        }
    }

    #[test]
    fn plan_gate_accepts_plan_channels() {
        let rows = vec![row("a", 1, Some(36)), row("b", 6, None), row("c", 11, Some(165))];
        assert!(validate_plan("reg1", &rows).is_ok());
    }

    #[test]
    fn plan_gate_rejects_offplan_2g() {
        assert!(validate_plan("reg1", &[row("a", 5, None)]).is_err());
    }

    #[test]
    fn plan_gate_rejects_dfs_and_zero_5g() {
        assert!(validate_plan("reg1", &[row("a", 1, Some(112))]).is_err());
        assert!(validate_plan("reg1", &[row("a", 1, Some(0))]).is_err());
    }

    #[test]
    fn plan_gate_rejects_empty_and_missing_2g() {
        assert!(validate_plan("reg1", &[]).is_err());
        let mut r = row("a", 1, None);
        r.ch_after_2g = None;
        assert!(validate_plan("reg1", &[r]).is_err());
    }
}
