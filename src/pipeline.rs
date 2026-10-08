use anyhow::{Context, Result};
use chrono::Local;
use rayon::prelude::*;
use std::collections::HashMap;

use crate::config::Options;
use crate::cost::{assign, Assignment, Band};
use crate::db;
use crate::discover::{self, ApInfo};
use crate::graph;
use crate::output::{self, OutputRow, Summary};

fn with_assigned(
    ap: &ApInfo,
    bssid_index: &HashMap<String, String>,
    assigned: &HashMap<String, u8>,
) -> ApInfo {
    let mut out = ap.clone();
    for n in out.neighbors.iter_mut() {
        if let Some(serial) = bssid_index.get(&n.bssid.to_uppercase()) {
            if let Some(ch) = assigned.get(serial) {
                n.channel = *ch;
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn assign_band(
    aps: &[ApInfo],
    groups: &[Vec<usize>],
    band: Band,
    bssid_index: &HashMap<String, String>,
    threshold: f32,
    stay_delta: f64,
) -> HashMap<String, Assignment> {
    let mut assigned_ch: HashMap<String, u8> = HashMap::new();
    let mut out: HashMap<String, Assignment> = HashMap::new();
    for g in groups {
        for &i in g {
            let view = with_assigned(&aps[i], bssid_index, &assigned_ch);
            if let Some(a) = assign(std::slice::from_ref(&view), band, threshold, stay_delta).pop() {
                if let Some(ch) = a.after {
                    assigned_ch.insert(a.serial.clone(), ch);
                }
                out.insert(a.serial.clone(), a);
            }
        }
    }
    out
}

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

    let a2 = assign_band(aps, &clusters.g2, Band::G2, bssid_index, threshold, stay_delta);
    let a5 = assign_band(aps, &clusters.g5, Band::G5, bssid_index, threshold, stay_delta);

    let mut cid2: HashMap<String, i64> = HashMap::new();
    for (ci, members) in clusters.g2.iter().enumerate() {
        for &i in members {
            cid2.insert(aps[i].serial.clone(), ci as i64 + 1);
        }
    }
    let mut cid5: HashMap<String, i64> = HashMap::new();
    for (ci, members) in clusters.g5.iter().enumerate() {
        for &i in members {
            cid5.insert(aps[i].serial.clone(), ci as i64 + 1);
        }
    }

    let mut rows: Vec<OutputRow> = aps
        .iter()
        .map(|ap| {
            let b2 = a2.get(&ap.serial);
            let b5 = a5.get(&ap.serial);
            let has2 = ap.band2g.is_some();
            let has5 = ap.band5g.is_some();
            OutputRow {
                reg: reg.to_string(),
                sn: ap.serial.clone(),
                clusterid_2g: has2.then(|| cid2.get(&ap.serial).copied().unwrap_or(0)),
                ch_before_2g: b2.and_then(|a| a.before),
                cost_before_2g: b2.and_then(|a| a.cost_before),
                ch_after_2g: b2.and_then(|a| a.after),
                cost_after_2g: b2.and_then(|a| a.cost_after),
                status_2g: b2.map(|a| a.status.clone()),
                clusterid_5g: has5.then(|| cid5.get(&ap.serial).copied().unwrap_or(0)),
                ch_before_5g: b5.and_then(|a| a.before),
                cost_before_5g: b5.and_then(|a| a.cost_before),
                ch_after_5g: b5.and_then(|a| a.after),
                cost_after_5g: b5.and_then(|a| a.cost_after),
                status_5g: b5.map(|a| a.status.clone()),
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
    let bssid = match db::load_bssid_index(&opts.db_url) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bssid index failed, clusters fall back to singletons: {e:#}");
            HashMap::new()
        }
    };
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
