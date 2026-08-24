use crate::cluster::MemRow;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashSet;

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

struct OverlapTable {
    table: Box<[[f64; 170]; 170]>,
}

impl OverlapTable {
    fn for_band(band: &str) -> Self {
        let mut table = Box::new([[0.0_f64; 170]; 170]);
        let (cands, nbs): (Vec<i32>, Vec<i32>) = if band == "2g" {
            (CANDIDATES_2G.to_vec(), (1..=14).collect())
        } else {
            (CANDIDATES_5G.to_vec(), CANDIDATES_5G.to_vec())
        };
        let cand_bw = if band == "2g" { 20 } else { 80 };
        let nb_bw = 20;

        for &cand_ch in &cands {
            for &nb_ch in &nbs {
                if (0..170).contains(&cand_ch) && (0..170).contains(&nb_ch) {
                    table[cand_ch as usize][nb_ch as usize] =
                        compute_overlap(cand_ch, cand_bw, nb_ch, nb_bw, band);
                }
            }
        }
        Self { table }
    }

    #[inline(always)]
    fn get(&self, cand_ch: i32, nb_ch: i32) -> f64 {
        if (0..170).contains(&cand_ch) && (0..170).contains(&nb_ch) {
            unsafe {
                *self
                    .table
                    .get_unchecked(cand_ch as usize)
                    .get_unchecked(nb_ch as usize)
            }
        } else {
            0.0
        }
    }
}

#[inline]
fn compute_overlap(cand_ch: i32, cand_bw: i32, nb_ch: i32, nb_bw: i32, band: &str) -> f64 {
    let cf = if band == "2g" {
        |c: i32| 2407 + 5 * c
    } else {
        |c: i32| 5000 + 5 * c
    };
    let dist = (cf(cand_ch) - cf(nb_ch)).abs();
    (std::cmp::max(0, cand_bw / 2 + nb_bw / 2 - dist) as f64 / cand_bw as f64).min(1.0)
}

#[inline]
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

#[inline]
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn possible_channels_for(ap: &MemRow, band: &str) -> Vec<i32> {
    let base = if band == "2g" {
        CANDIDATES_2G
    } else {
        CANDIDATES_5G
    };
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
    if let Some(ch) = ap.own_channel
        && base_set.contains(&ch)
    {
        set.insert(ch);
    }
    if set.is_empty() {
        for &c in base {
            set.insert(c);
        }
    }
    set.retain(|c| !DFS_CHANNELS_5G.contains(c));
    let mut v: Vec<i32> = set.into_iter().collect();
    v.sort_unstable();
    v
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

struct ApOpt {
    sn: String,
    cluster_id: Option<i64>,
    neighbors: Vec<(i32, f64)>,
    possible: Vec<i32>,
    cur_ch: i32,
}

pub fn optimize_band_cluster_aware(
    rows: &[MemRow],
    cluster_sns_list: &[Vec<String>],
    stale_threshold: usize,
    reg: &str,
    band: &str,
) -> Result<Vec<OptimizeRow>> {
    let invalid_set: HashSet<&str> = HashSet::from(["00:00:00:00:00:00"]);

    let mut own_bssid_set: HashSet<&str> = HashSet::with_capacity(rows.len() * 2);
    for r in rows {
        for b in &r.own {
            let b_str = b.trim();
            if !b_str.is_empty() && !invalid_set.contains(b_str) {
                own_bssid_set.insert(b_str);
            }
        }
    }

    let table = OverlapTable::for_band(band);

    let mut sn_to_clusterid: std::collections::HashMap<&str, i64> =
        std::collections::HashMap::with_capacity(cluster_sns_list.iter().map(|v| v.len()).sum());
    for (i, sns) in cluster_sns_list.iter().enumerate() {
        let cid = (i + 1) as i64;
        for sn in sns {
            sn_to_clusterid.insert(sn.as_str(), cid);
        }
    }

    let mut aps: Vec<ApOpt> = Vec::with_capacity(rows.len());
    for r in rows {
        if r.own.is_empty() {
            continue;
        }
        let possible = possible_channels_for(r, band);
        if possible.is_empty() {
            continue;
        }
        let cur_ch = r.own_channel.unwrap_or(possible[0]);
        let cluster_id = sn_to_clusterid.get(r.sn.as_str()).copied();

        let mut neighbors: Vec<(i32, f64)> = Vec::with_capacity(r.neighbors.len());
        for (bssid, ch, rssi) in &r.neighbors {
            let b_str = bssid.trim();
            if b_str.is_empty() || invalid_set.contains(b_str) {
                continue;
            }
            if !band_ok(*ch, band) {
                continue;
            }
            let owner_f = if own_bssid_set.contains(b_str) {
                OWN_GATEWAY
            } else {
                EXTERNAL
            };
            neighbors.push((*ch, rssi_factor(*rssi) * owner_f));
        }
        aps.push(ApOpt {
            sn: r.sn.clone(),
            cluster_id,
            neighbors,
            possible,
            cur_ch,
        });
    }

    aps.sort_unstable_by(|a, b| match (a.cluster_id, b.cluster_id) {
        (Some(x), Some(y)) if x == y => a.neighbors.len().cmp(&b.neighbors.len()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => x.cmp(&y),
        (None, None) => a.neighbors.len().cmp(&b.neighbors.len()),
    });

    let mut assigned: Vec<i32> = aps.iter().map(|ap| ap.cur_ch).collect();

    let compute_costs = |assigned_snap: &[i32]| -> Vec<f64> {
        assigned_snap
            .par_iter()
            .enumerate()
            .map(|(i, &ch)| cost_for_ap(ch, &aps[i], &table))
            .collect()
    };
    let cost_before_vec = compute_costs(&assigned);

    let mut counter = stale_threshold;
    let mut total_rounds = 0_usize;
    let mut resets = 0_usize;
    loop {
        total_rounds += 1;
        let mut changed = false;
        for (i, ap) in aps.iter().enumerate() {
            let cur_ch = assigned[i];
            let cost_at_cur = cost_for_ap(cur_ch, ap, &table);
            let mut best_ch = ap.possible[0];
            let mut best_c = cost_for_ap(best_ch, ap, &table);
            for &cand in ap.possible.iter().skip(1) {
                let c = cost_for_ap(cand, ap, &table);
                if c < best_c {
                    best_c = c;
                    best_ch = cand;
                }
            }
            let saving = cost_at_cur - best_c;
            if saving > 0.0005 {
                if best_ch != cur_ch {
                    changed = true;
                }
                assigned[i] = best_ch;
            }
        }
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

    let cost_after_vec = compute_costs(&assigned);

    let mut out = Vec::with_capacity(aps.len());
    for (i, ap) in aps.iter().enumerate() {
        let cb = ap.cur_ch;
        let ca = assigned[i];
        out.push(OptimizeRow {
            reg: reg.to_string(),
            sn: ap.sn.clone(),
            clusterid: ap.cluster_id,
            ch_before: Some(cb),
            cost_before: round3(cost_before_vec[i]),
            ch_after: ca,
            cost_after: round3(cost_after_vec[i]),
            status: if ca == cb {
                "STAY".into()
            } else {
                "CHANGE".into()
            },
        });
    }
    eprintln!(
        "[optimize {band}] rounds={} resets={} stale_threshold={} final_counter={}",
        total_rounds, resets, stale_threshold, counter
    );
    Ok(out)
}

#[inline]
fn band_ok(ch: i32, band: &str) -> bool {
    if band == "2g" {
        (1..=14).contains(&ch)
    } else {
        ch >= 32 && !DFS_CHANNELS_5G.contains(&ch)
    }
}

#[inline(always)]
fn cost_for_ap(cand_ch: i32, ap: &ApOpt, table: &OverlapTable) -> f64 {
    let mut total = 0.0_f64;
    for &(nb_ch, pre_weight) in &ap.neighbors {
        let overlap = table.get(cand_ch, nb_ch);
        if overlap <= 0.0 {
            continue;
        }
        let type_f = if nb_ch == cand_ch {
            CO_FACTOR
        } else {
            ADJ_FACTOR
        };
        total += overlap * type_f * pre_weight;
    }
    total
}
