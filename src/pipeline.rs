use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;

use crate::config::Options;
use crate::cost::{self, OptimizeRow};
use crate::db;
use crate::discover::{self, ApInfo};
use crate::graph;
use crate::output::{self, OutputRow, Summary};

pub fn optimize(
    reg: &str,
    aps: &[ApInfo],
    stale: usize,
    threshold_note: &str,
) -> Vec<OutputRow> {
    let _ = threshold_note;
    let updated_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let mem2 = graph::mem_rows_for(aps, "2g");
    let mem5 = graph::mem_rows_for(aps, "5g");
    let c2 = graph::cluster(aps, "2g", 2).map(|c| c.valid).unwrap_or_default();
    let c5 = graph::cluster(aps, "5g", 2).map(|c| c.valid).unwrap_or_default();

    let r2 = cost::optimize_band(&mem2, &c2, stale, "2g");
    let r5 = cost::optimize_band(&mem5, &c5, stale, "5g");

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
    let mut total = 0;
    let mut failed = Vec::new();
    for reg in &opts.regs {
        println!("start {reg} ...");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        match db::load_discover(opts, reg) {
            Ok(drows) => {
                let aps: Vec<ApInfo> = drows
                    .par_iter()
                    .filter_map(|r| discover::parse(r).ok())
                    .collect();
                let out = optimize(reg, &aps, opts.stale, "");
                total += out.len();
                let path = format!("{}/wifi_optimize_{}_{}.csv", opts.out, opts.date, reg);
                if let Err(e) = output::write_csv(&path, &out) {
                    eprintln!("write {reg} failed: {e:#}");
                    failed.push(reg.clone());
                } else {
                    println!("{reg}: {} rows -> {path}", out.len());
                }
            }
            Err(e) => {
                eprintln!("load {reg} failed: {e:#}");
                failed.push(reg.clone());
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
