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
    let (r5, s5) = cost::optimize_band_bruteforce(&mem5, &c5, stale, max_k, "5g");
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
                    if let Some(t) = table.clone() {
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
