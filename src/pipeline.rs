use anyhow::{Context, Result};
use chrono::Local;
use rayon::prelude::*;
use std::collections::HashMap;

use crate::config::Options;
use crate::cost::{assign, Band};
use crate::db;
use crate::discover::{self, ApInfo};
use crate::graph;
use crate::output::{self, OutputRow, Summary};

pub fn optimize(
    reg: &str,
    _date: &str,
    aps: &[ApInfo],
    bssid_index: &HashMap<String, String>,
    threshold: f32,
    stay_delta: f64,
) -> Vec<OutputRow> {
    let updated_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let clusters = graph::clusters(aps, bssid_index, threshold);

    let mut idx2g: HashMap<String, (i64, &ApInfo)> = HashMap::new();
    for (ci, members) in clusters.g2.iter().enumerate() {
        for &i in members {
            idx2g.insert(aps[i].serial.clone(), (ci as i64 + 1, &aps[i]));
        }
    }
    let mut idx5g: HashMap<String, (i64, &ApInfo)> = HashMap::new();
    for (ci, members) in clusters.g5.iter().enumerate() {
        for &i in members {
            idx5g.insert(aps[i].serial.clone(), (ci as i64 + 1, &aps[i]));
        }
    }

    let mut rows: Vec<OutputRow> = aps
        .par_iter()
        .map(|ap| {
            let single = std::slice::from_ref(ap);
            let a2 = assign(single, Band::G2, threshold, stay_delta).pop();
            let a5 = assign(single, Band::G5, threshold, stay_delta).pop();
            let cid2 = idx2g.get(&ap.serial).map(|(c, _)| *c).unwrap_or(0);
            let cid5 = idx5g.get(&ap.serial).map(|(c, _)| *c).unwrap_or(0);
            let has2 = ap.band2g.is_some();
            let has5 = ap.band5g.is_some();
            OutputRow {
                reg: reg.to_string(),
                sn: ap.serial.clone(),
                clusterid_2g: has2.then_some(cid2),
                ch_before_2g: a2.as_ref().and_then(|a| a.before),
                cost_before_2g: a2.as_ref().and_then(|a| a.cost_before),
                ch_after_2g: a2.as_ref().and_then(|a| a.after),
                cost_after_2g: a2.as_ref().and_then(|a| a.cost_after),
                status_2g: a2.map(|a| a.status),
                clusterid_5g: has5.then_some(cid5),
                ch_before_5g: a5.as_ref().and_then(|a| a.before),
                cost_before_5g: a5.as_ref().and_then(|a| a.cost_before),
                ch_after_5g: a5.as_ref().and_then(|a| a.after),
                cost_after_5g: a5.as_ref().and_then(|a| a.cost_after),
                status_5g: a5.map(|a| a.status),
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
    let bssid = db::load_bssid_index(&opts.db_url).unwrap_or_default();
    let mut total = 0;
    let mut failed = Vec::new();
    for reg in &opts.regs {
        match db::load_discover(opts, reg) {
            Ok(rows) => {
                let aps: Vec<ApInfo> = rows
                    .par_iter()
                    .filter_map(|r| discover::parse(r).ok())
                    .collect();
                let out = optimize(
                    reg,
                    &opts.date,
                    &aps,
                    &bssid,
                    opts.rssi_threshold,
                    opts.stay_delta,
                );
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
