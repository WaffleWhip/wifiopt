use crate::cluster::MemRow;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

const CANDIDATES_2G: &[i32] = &[1, 6, 11];
const CANDIDATES_5G: &[i32] = &[
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 149,
    153, 157, 161, 165,
];
const DFS_CHANNELS_5G: &[i32] = &[
    52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140,
];

const OWN_GATEWAY: f64 = 0.50;
const EXTERNAL: f64 = 1.00;

const CO_FACTOR: f64 = 1.00;
const ADJ_FACTOR: f64 = 1.20;

fn rssi_factor(rssi: i32) -> f64 {
    if rssi >= -50 {
        1.00
    } else if rssi >= -60 {
        0.80
    } else if rssi >= -70 {
        0.60
    } else if rssi >= -80 {
        0.30
    } else {
        0.10
    }
}

fn center_freq_24(ch: i32) -> i32 {
    2407 + 5 * ch
}
fn center_freq_5(ch: i32) -> i32 {
    5000 + 5 * ch
}

fn overlap_factor(
    cand_ch: i32,
    cand_bw: i32,
    nb_ch: i32,
    nb_bw: i32,
    band: &str,
) -> f64 {
    if band == "2g" && nb_bw == 40 {
        let offsets: &[i32] = &[-2, 2];
        let mut max_overlap = 0.0_f64;
        for &off in offsets {
            let center = center_freq_24(nb_ch + off);
            let dist = (center_freq_24(cand_ch) - center).abs();
            let overlap_mhz = std::cmp::max(0, cand_bw / 2 + 40 / 2 - dist);
            let ov = (overlap_mhz as f64 / cand_bw as f64).min(1.0);
            if ov > max_overlap {
                max_overlap = ov;
            }
        }
        return max_overlap;
    }
    let (cand_c, nb_c) = if band == "2g" {
        (center_freq_24(cand_ch), center_freq_24(nb_ch))
    } else {
        (center_freq_5(cand_ch), center_freq_5(nb_ch))
    };
    let dist = (cand_c - nb_c).abs();
    let overlap_mhz = std::cmp::max(0, cand_bw / 2 + nb_bw / 2 - dist);
    (overlap_mhz as f64 / cand_bw as f64).min(1.0)
}

pub fn possible_channels_for(ap: &MemRow, band: &str) -> Vec<i32> {
    let base = if band == "2g" { CANDIDATES_2G } else { CANDIDATES_5G };
    if band == "2g" {
        return base.to_vec();
    }
    let base_set: HashSet<i32> = base.iter().copied().collect();
    let mut set: HashSet<i32> = HashSet::new();
    for &c in &ap.possible_channels {
        if base_set.contains(&c) {
            set.insert(c);
        }
    }
    if let Some(ch) = ap.own_channel {
        if base_set.contains(&ch) {
            set.insert(ch);
        }
    }
    if set.is_empty() {
        for &c in base {
            set.insert(c);
        }
    }
    set.retain(|c| !DFS_CHANNELS_5G.contains(c));
    let mut v: Vec<i32> = set.into_iter().collect();
    v.sort();
    v
}

pub fn channel_cost(
    cand_ch: i32,
    cand_bw: i32,
    ap: &MemRow,
    own_set: &HashSet<String>,
    band: &str,
) -> f64 {
    let mut total = 0.0_f64;
    for (nb_bssid, nb_ch, nb_rssi) in &ap.neighbors {
        let nb_bw = 20_i32;
        let overlap = overlap_factor(cand_ch, cand_bw, *nb_ch, nb_bw, band);
        if overlap <= 0.0 {
            continue;
        }
        let type_f = if *nb_ch == cand_ch { CO_FACTOR } else { ADJ_FACTOR };
        let rssi_f = rssi_factor(*nb_rssi);
        let own = own_set.contains(nb_bssid);
        let owner_f = if own { OWN_GATEWAY } else { EXTERNAL };
        total += overlap * type_f * rssi_f * owner_f;
    }
    total
}

pub fn channel_cost_with_assignment(
    cand_ch: i32,
    cand_bw: i32,
    ap: &MemRow,
    assigned: &HashMap<String, i32>,
    own_set: &HashSet<String>,
    band: &str,
) -> f64 {
    let mut total = 0.0_f64;
    for (nb_bssid, nb_ch, nb_rssi) in &ap.neighbors {
        let nb_bw = 20_i32;
        let effective_nb_ch = assigned.get(nb_bssid).copied().unwrap_or(*nb_ch);
        let overlap = overlap_factor(cand_ch, cand_bw, effective_nb_ch, nb_bw, band);
        if overlap <= 0.0 {
            continue;
        }
        let type_f = if effective_nb_ch == cand_ch { CO_FACTOR } else { ADJ_FACTOR };
        let rssi_f = rssi_factor(*nb_rssi);
        let own = own_set.contains(nb_bssid);
        let owner_f = if own { OWN_GATEWAY } else { EXTERNAL };
        total += overlap * type_f * rssi_f * owner_f;
    }
    total
}

#[derive(Debug, Clone)]
pub struct OptimizeRow {
    pub reg: String,
    pub sn: String,
    pub clusterid: Option<i64>,
    pub ch_before: Option<i32>,
    pub cost_before: f64,
    pub ch_after: i32,
    pub cost_after: f64,
    pub status: String,
}

pub fn optimize_band_cluster_aware(
    rows: &[MemRow],
    cluster_sns_list: &[Vec<String>],
    stale_threshold: usize,
    reg: &str,
    band: &str,
) -> Result<Vec<OptimizeRow>> {
    let invalid = ["00:00:00:00:00:00"];
    let invalid_set: HashSet<String> = invalid.iter().map(|s| s.to_string()).collect();

    let mut own_set: HashSet<String> = HashSet::new();
    for r in rows {
        for b in &r.own {
            let b = b.trim().to_lowercase();
            if !b.is_empty() && !invalid_set.contains(&b) {
                own_set.insert(b);
            }
        }
    }

    let mut sn_to_clusterid: HashMap<String, i64> = HashMap::new();
    for (i, sns) in cluster_sns_list.iter().enumerate() {
        let cid = (i + 1) as i64;
        for sn in sns {
            sn_to_clusterid.insert(sn.clone(), cid);
        }
    }

    let mut aps: Vec<&MemRow> = rows.iter().filter(|r| !r.own.is_empty()).collect();
    aps.sort_by(|a, b| {
        let ca = sn_to_clusterid.get(&a.sn);
        let cb = sn_to_clusterid.get(&b.sn);
        match (ca, cb) {
            (Some(x), Some(y)) if x == y => a.neighbors.len().cmp(&b.neighbors.len()),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => x.cmp(y),
            (None, None) => a.neighbors.len().cmp(&b.neighbors.len()),
        }
    });

    let cand_bw = if band == "2g" { 20 } else { 80 };

    let mut possible_map: HashMap<String, Vec<i32>> = HashMap::new();
    let mut cur_ch_map: HashMap<String, i32> = HashMap::new();
    let mut assigned: HashMap<String, i32> = HashMap::new();

    for ap in &aps {
        let possible = possible_channels_for(ap, band);
        if possible.is_empty() {
            continue;
        }
        possible_map.insert(ap.sn.clone(), possible.clone());
        let cur_ch = ap.own_channel.unwrap_or(possible[0]);
        cur_ch_map.insert(ap.sn.clone(), cur_ch);
        assigned.insert(ap.sn.clone(), cur_ch);
    }

    let cost_for_snapshot = |assigned_snap: &HashMap<String, i32>| -> HashMap<String, f64> {
        let mut costs: HashMap<String, f64> = HashMap::new();
        for ap in &aps {
            if let Some(&ch) = assigned_snap.get(&ap.sn) {
                let c = channel_cost_with_assignment(ch, cand_bw, ap, assigned_snap, &own_set, band);
                costs.insert(ap.sn.clone(), c);
            }
        }
        costs
    };

    let cost_before_map = cost_for_snapshot(&assigned);

    let mut counter = stale_threshold;
    let mut best_cost: f64 = f64::INFINITY;
    let mut total_rounds = 0_usize;
    let mut resets = 0_usize;

    loop {
        total_rounds += 1;
        let mut changed = false;
        let mut new_assigned = assigned.clone();

        for ap in &aps {
            let Some(possible) = possible_map.get(&ap.sn) else {
                continue;
            };
            let Some(&cur_ch) = assigned.get(&ap.sn) else {
                continue;
            };
            let cur_in_possible = possible.contains(&cur_ch);
            let mut best_ch = if cur_in_possible { cur_ch } else { possible[0] };
            let mut best_c = channel_cost_with_assignment(best_ch, cand_bw, ap, &new_assigned, &own_set, band);
            for &cand in possible {
                let c = channel_cost_with_assignment(cand, cand_bw, ap, &new_assigned, &own_set, band);
                if c < best_c {
                    best_c = c;
                    best_ch = cand;
                }
            }
            if best_ch != new_assigned.get(&ap.sn).copied().unwrap_or(cur_ch) {
                changed = true;
            }
            new_assigned.insert(ap.sn.clone(), best_ch);
        }

        assigned = new_assigned;

        if changed {
            counter = stale_threshold;
            resets += 1;
        } else {
            counter -= 1;
        }
        if counter == 0 || !changed {
            break;
        }
    }

    let cost_after_map = cost_for_snapshot(&assigned);

    let mut out = Vec::with_capacity(aps.len());
    for ap in &aps {
        let cid = sn_to_clusterid.get(&ap.sn).copied();
        let ch_before = cur_ch_map.get(&ap.sn).copied();
        let ch_after = assigned.get(&ap.sn).copied();
        let (Some(cb), Some(ca)) = (ch_before, ch_after) else {
            continue;
        };
        let cost_before = cost_before_map.get(&ap.sn).copied().unwrap_or(0.0);
        let cost_after = cost_after_map.get(&ap.sn).copied().unwrap_or(0.0);
        let status = if ca == cb { "STAY" } else { "CHANGE" }.to_string();
        out.push(OptimizeRow {
            reg: reg.to_string(),
            sn: ap.sn.clone(),
            clusterid: cid,
            ch_before: Some(cb),
            cost_before,
            ch_after: ca,
            cost_after,
            status,
        });
    }
    eprintln!(
        "[optimize {band}] rounds={} resets={} stale_threshold={} final_counter={}",
        total_rounds,
        resets,
        stale_threshold,
        counter
    );
    Ok(out)
}

pub fn print_tsv_header() {
    println!(
        "#reg\tsn\tclusterid_{}\tch_before_{}\tcost_before_{}\tch_after_{}\tcost_after_{}\tstatus_{}\tclusterid_{}\tch_before_{}\tcost_before_{}\tch_after_{}\tcost_after_{}\tstatus_{}",
        "2g", "2g", "2g", "2g", "2g", "2g",
        "5g", "5g", "5g", "5g", "5g", "5g"
    );
}

pub fn print_tsv_row_2g(r2g: &OptimizeRow) {
    let cid_2g = r2g.clusterid.map(|c| c.to_string()).unwrap_or_else(|| "\\N".to_string());
    let ch_b = r2g.ch_before.map(|c| c.to_string()).unwrap_or_else(|| "\\N".to_string());
    let ch_a = r2g.ch_after.to_string();
    println!(
        "{}\t{}\t{}\t{}\t{:.4}\t{}\t{:.4}\t{}\t\\N\t\\N\t{:.4}\t\\N\t{:.4}\tSTAY",
        r2g.reg, r2g.sn, cid_2g, ch_b, r2g.cost_before, ch_a, r2g.cost_after, r2g.status,
        0.0, 0.0
    );
}

pub fn print_tsv_row_5g(r5g: &OptimizeRow) {
    let cid_5g = r5g.clusterid.map(|c| c.to_string()).unwrap_or_else(|| "\\N".to_string());
    let ch_b = r5g.ch_before.map(|c| c.to_string()).unwrap_or_else(|| "\\N".to_string());
    let ch_a = r5g.ch_after.to_string();
    println!(
        "{}\t{}\t\\N\t\\N\t{:.4}\t\\N\t{:.4}\tSTAY\t{}\t{}\t{:.4}\t{}\t{:.4}\t{}",
        r5g.reg, r5g.sn,
        0.0, 0.0,
        cid_5g, ch_b, r5g.cost_before, ch_a, r5g.cost_after, r5g.status
    );
}

pub fn print_summary_2g(band: &str, rows: &[OptimizeRow]) {
    let total = rows.len();
    let change = rows.iter().filter(|r| r.status == "CHANGE").count();
    let stay = total - change;
    let total_before: f64 = rows.iter().map(|r| r.cost_before).sum();
    let total_after: f64 = rows.iter().map(|r| r.cost_after).sum();
    let reduction = if total_before > 0.0 {
        (1.0 - total_after / total_before) * 100.0
    } else {
        0.0
    };
    eprintln!(
        "[optimize {band}] total={} change={} stay={} reduction={:.2}% before={:.2} after={:.2}",
        total, change, stay, reduction, total_before, total_after
    );
}
