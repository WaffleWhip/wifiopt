use std::collections::{HashMap, HashSet};

use crate::discover::INVALID_BSSID;
use crate::graph::MemRow;

pub const CANDIDATES_2G: &[i32] = &[1, 6, 11];
pub const CANDIDATES_5G: &[i32] = &[
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 149,
    153, 157, 161, 165,
];
pub const DFS_5G: &[i32] = &[
    52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140,
];

pub const OWN_GATEWAY: f64 = 0.50;
pub const EXTERNAL: f64 = 1.00;
pub const CO_FACTOR: f64 = 1.00;
pub const ADJ_FACTOR: f64 = 1.20;

pub fn band_cost_ok(ch: i32, band: &str) -> bool {
    if band == "2g" {
        (1..=14).contains(&ch)
    } else {
        ch >= 32 && !DFS_5G.contains(&ch)
    }
}

fn overlap(cand_ch: i32, cand_bw: i32, nb_ch: i32, nb_bw: i32, band: &str) -> f64 {
    let cf = if band == "2g" {
        |c: i32| 2407 + 5 * c
    } else {
        |c: i32| 5000 + 5 * c
    };
    let dist = (cf(cand_ch) - cf(nb_ch)).abs();
    (std::cmp::max(0, cand_bw / 2 + nb_bw / 2 - dist) as f64 / cand_bw as f64).min(1.0)
}

pub fn overlap_2g(a: i32, b: i32) -> f64 {
    overlap(a, 20, b, 20, "2g")
}

pub fn overlap_5g(a: i32, b: i32) -> f64 {
    overlap(a, 80, b, 20, "5g")
}

pub fn rssi_factor(rssi: i32) -> f64 {
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

pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn possible_channels(ap: &MemRow, band: &str) -> Vec<i32> {
    let base = if band == "2g" {
        CANDIDATES_2G
    } else {
        CANDIDATES_5G
    };
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
    if band == "5g" {
        set.retain(|c| !DFS_5G.contains(c));
    }
    if set.is_empty() {
        for &c in base {
            if band != "5g" || !DFS_5G.contains(&c) {
                set.insert(c);
            }
        }
    }
    let mut v: Vec<i32> = set.into_iter().collect();
    v.sort_unstable();
    v
}

#[derive(Debug, Clone)]
pub struct OptimizeRow {
    pub sn: String,
    pub clusterid: Option<i64>,
    pub ch_before: Option<i32>,
    pub cost_before: f64,
    pub ch_after: i32,
    pub cost_after: f64,
    pub status: String,
}

struct ApOpt {
    sn: String,
    cluster_id: Option<i64>,
    neighbors: Vec<(i32, f64)>,
    possible: Vec<i32>,
    cur_ch: i32,
}

fn cost_for(cand: i32, neighbors: &[(i32, f64)], band: &str) -> f64 {
    let mut total = 0.0;
    for &(nb_ch, w) in neighbors {
        let ov = if band == "2g" {
            overlap_2g(cand, nb_ch)
        } else {
            overlap_5g(cand, nb_ch)
        };
        if ov <= 0.0 {
            continue;
        }
        total += ov * if nb_ch == cand { CO_FACTOR } else { ADJ_FACTOR } * w;
    }
    total
}

pub fn optimize_band(
    rows: &[MemRow],
    cluster_sns: &[Vec<String>],
    stale_threshold: usize,
    band: &str,
) -> Vec<OptimizeRow> {
    let mut own_set: HashSet<&str> = HashSet::new();
    for r in rows {
        for b in &r.own {
            let s = b.trim();
            if !s.is_empty() && s != INVALID_BSSID {
                own_set.insert(s);
            }
        }
    }

    let mut cid_of: HashMap<&str, i64> = HashMap::new();
    for (i, sns) in cluster_sns.iter().enumerate() {
        for sn in sns {
            cid_of.insert(sn.as_str(), (i + 1) as i64);
        }
    }

    let mut aps: Vec<ApOpt> = Vec::new();
    for r in rows {
        if r.own.is_empty() {
            continue;
        }
        let possible = possible_channels(r, band);
        if possible.is_empty() {
            continue;
        }
        let cur = r.own_channel.unwrap_or(possible[0]);
        let mut neighbors = Vec::new();
        for (bssid, ch, rssi) in &r.neighbors {
            let s = bssid.trim();
            if s.is_empty() || s == INVALID_BSSID || !band_cost_ok(*ch, band) {
                continue;
            }
            neighbors.push((*ch, rssi_factor(*rssi) * if own_set.contains(s) {
                OWN_GATEWAY
            } else {
                EXTERNAL
            }));
        }
        aps.push(ApOpt {
            cluster_id: cid_of.get(r.sn.as_str()).copied(),
            sn: r.sn.clone(),
            neighbors,
            possible,
            cur_ch: cur,
        });
    }

    aps.sort_by(|a, b| {
        (a.cluster_id, a.neighbors.len(), &a.sn).cmp(&(b.cluster_id, b.neighbors.len(), &b.sn))
    });

    let mut assigned: Vec<i32> = aps.iter().map(|a| a.cur_ch).collect();
    let mut counter = stale_threshold.max(1);
    loop {
        let mut changed = false;
        for (i, ap) in aps.iter().enumerate() {
            let cur = assigned[i];
            let cost_cur = cost_for(cur, &ap.neighbors, band);
            let mut best_ch = ap.possible[0];
            let mut best_c = cost_for(best_ch, &ap.neighbors, band);
            for &cand in ap.possible.iter().skip(1) {
                let c = cost_for(cand, &ap.neighbors, band);
                if c < best_c {
                    best_c = c;
                    best_ch = cand;
                }
            }
            if !ap.possible.contains(&cur) || cost_cur - best_c > 0.0005 {
                if best_ch != cur {
                    changed = true;
                }
                assigned[i] = best_ch;
            }
        }
        if changed {
            counter = stale_threshold.max(1);
        } else {
            counter -= 1;
        }
        if counter == 0 || !changed {
            break;
        }
    }

    aps.into_iter()
        .enumerate()
        .map(|(i, ap)| {
            let ca = assigned[i];
            let cb = ap.cur_ch;
            OptimizeRow {
                sn: ap.sn,
                clusterid: ap.cluster_id,
                ch_before: Some(cb),
                cost_before: round3(cost_for(cb, &ap.neighbors, band)),
                ch_after: ca,
                cost_after: round3(cost_for(ca, &ap.neighbors, band)),
                status: if ca == cb { "STAY".into() } else { "CHANGE".into() },
            }
        })
        .collect()
}
