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

/// Pre-computed overlap table.
/// Key: encoded as ((cand_ch, nb_ch, nb_bw) packed into usize, band).
/// For 2g: cand_ch ∈ [1..=14], nb_ch ∈ [1..=14], nb_bw ∈ {20, 40}.
/// For 5g: cand_ch ∈ [32..=165], nb_ch ∈ [32..=165], nb_bw ∈ {20}.
/// We use a flat HashMap lookup at runtime.
struct OverlapTable {
    map: std::collections::HashMap<(i32, i32, i32, &'static str), f64>,
}

impl OverlapTable {
    fn for_band(band: &'static str) -> Self {
        let mut map = std::collections::HashMap::new();
        let (cands, nbs): (Vec<i32>, Vec<i32>) = if band == "2g" {
            (CANDIDATES_2G.to_vec(), (1..=14).collect())
        } else {
            (CANDIDATES_5G.to_vec(), CANDIDATES_5G.to_vec())
        };
        let cand_bw = if band == "2g" { 20 } else { 80 };
        let nb_bws: Vec<i32> = if band == "2g" { vec![20, 40] } else { vec![20] };
        for &cand_ch in &cands {
            for &nb_ch in &nbs {
                for &nb_bw in &nb_bws {
                    let v = compute_overlap(cand_ch, cand_bw, nb_ch, nb_bw, band);
                    map.insert((cand_ch, nb_ch, nb_bw, band), v);
                }
            }
        }
        Self { map }
    }
    #[inline]
    fn get(&self, cand_ch: i32, nb_ch: i32, nb_bw: i32, band: &'static str) -> f64 {
        self.map.get(&(cand_ch, nb_ch, nb_bw, band)).copied().unwrap_or(0.0)
    }
}

#[inline]
fn compute_overlap(cand_ch: i32, cand_bw: i32, nb_ch: i32, nb_bw: i32, band: &str) -> f64 {
    if band == "2g" && nb_bw == 40 {
        let mut max_overlap = 0.0_f64;
        for &off in &[-2_i32, 2] {
            let dist = (2407 + 5 * cand_ch - (2407 + 5 * (nb_ch + off))).abs();
            let ov = std::cmp::max(0, cand_bw / 2 + 20 - dist) as f64 / cand_bw as f64;
            if ov >= 1.0 { return 1.0; }
            if ov > max_overlap { max_overlap = ov; }
        }
        return max_overlap;
    }
    let cf = if band == "2g" { |c: i32| 2407 + 5 * c } else { |c: i32| 5000 + 5 * c };
    let dist = (cf(cand_ch) - cf(nb_ch)).abs();
    (std::cmp::max(0, cand_bw / 2 + nb_bw / 2 - dist) as f64 / cand_bw as f64).min(1.0)
}

#[inline]
fn rssi_factor(rssi: i32) -> f64 {
    if rssi >= -50 { 1.00 }
    else if rssi >= -60 { 0.80 }
    else if rssi >= -70 { 0.60 }
    else if rssi >= -80 { 0.30 }
    else { 0.10 }
}

/// Round to 3 decimal places for storage display.
#[inline]
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub fn possible_channels_for(ap: &MemRow, band: &str) -> Vec<i32> {
    let base = if band == "2g" { CANDIDATES_2G } else { CANDIDATES_5G };
    if band == "2g" { return base.to_vec(); }
    let base_set: HashSet<i32> = base.iter().copied().collect();
    let mut set: HashSet<i32> = HashSet::new();
    for &c in &ap.possible_channels {
        if base_set.contains(&c) { set.insert(c); }
    }
    if let Some(ch) = ap.own_channel {
        if base_set.contains(&ch) { set.insert(ch); }
    }
    if set.is_empty() { for &c in base { set.insert(c); } }
    set.retain(|c| !DFS_CHANNELS_5G.contains(c));
    let mut v: Vec<i32> = set.into_iter().collect();
    v.sort();
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

/// Compact per-AP representation for fast iteration.
struct ApOpt {
    sn: String,
    /// (ch, nb_bw, rssi_factor, owner_f) — pre-computed.
    neighbors: Vec<(i32, i32, f64, f64)>,
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
    let invalid = ["00:00:00:00:00:00"];
    let invalid_set: HashSet<String> = invalid.iter().map(|s| s.to_string()).collect();

    // own_bssid_set: all bssids owned by APs in this band.
    let mut own_bssid_set: HashSet<String> = HashSet::new();
    for r in rows {
        for b in &r.own {
            let b = b.trim().to_lowercase();
            if !b.is_empty() && !invalid_set.contains(&b) { own_bssid_set.insert(b); }
        }
    }

    // Build overlap lookup table once per band.
    let band_static: &'static str = if band == "2g" { "2g" } else { "5g" };
    let table = OverlapTable::for_band(band_static);

    // SN → clusterid (for sort + output).
    let mut sn_to_clusterid: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for (i, sns) in cluster_sns_list.iter().enumerate() {
        let cid = (i + 1) as i64;
        for sn in sns { sn_to_clusterid.insert(sn.as_str(), cid); }
    }

    // Build ApOpt list with pre-computed neighbors.
    let mut aps: Vec<ApOpt> = Vec::with_capacity(rows.len());
    for r in rows {
        if r.own.is_empty() { continue; }
        let possible = possible_channels_for(r, band);
        if possible.is_empty() { continue; }
        let cur_ch = r.own_channel.unwrap_or(possible[0]);
        let mut neighbors: Vec<(i32, i32, f64, f64)> = Vec::with_capacity(r.neighbors.len());
        for (bssid, ch, rssi) in &r.neighbors {
            let b = bssid.trim().to_lowercase();
            if b.is_empty() || invalid_set.contains(&b) { continue; }
            if !band_ok(*ch, band) { continue; }
            let owner_f = if own_bssid_set.contains(&b) { OWN_GATEWAY } else { EXTERNAL };
            // Old code uses nb_bw=20 hardcoded — match that.
            neighbors.push((*ch, 20, rssi_factor(*rssi), owner_f));
        }
        aps.push(ApOpt { sn: r.sn.clone(), neighbors, possible, cur_ch });
    }

    // Sort: cluster id ASC, then neighbor count ASC.
    let cluster_for = |sn: &str| -> Option<i64> { sn_to_clusterid.get(sn).copied() };
    aps.sort_by(|a, b| {
        let ca = cluster_for(&a.sn);
        let cb = cluster_for(&b.sn);
        match (ca, cb) {
            (Some(x), Some(y)) if x == y => a.neighbors.len().cmp(&b.neighbors.len()),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => x.cmp(&y),
            (None, None) => a.neighbors.len().cmp(&b.neighbors.len()),
        }
    });

    // assigned: bookkeeping only (matches old behavior where cost doesn't use it).
    let mut assigned: Vec<i32> = aps.iter().map(|ap| ap.cur_ch).collect();

    // Parallel cost snapshot computation — uses current assigned[] channel.
    let compute_costs = |assigned_snap: &[i32]| -> Vec<f64> {
        assigned_snap.par_iter().enumerate().map(|(i, &ch)| {
            cost_for_ap(ch, &aps[i], &table, band_static)
        }).collect()
    };
    let cost_before_vec = compute_costs(&assigned);

    // Sequential Gauss-Seidel main loop — matches old algorithm exactly.
    let mut counter = stale_threshold;
    let mut total_rounds = 0_usize;
    let mut resets = 0_usize;
    loop {
        total_rounds += 1;
        let mut changed = false;
        for (i, ap) in aps.iter().enumerate() {
            let cur_ch = assigned[i];
            let cost_at_cur = cost_for_ap(cur_ch, ap, &table, band_static);
            let mut best_ch = ap.possible[0];
            let mut best_c = cost_for_ap(best_ch, ap, &table, band_static);
            for &cand in ap.possible.iter().skip(1) {
                let c = cost_for_ap(cand, ap, &table, band_static);
                if c < best_c { best_c = c; best_ch = cand; }
            }
            // Only switch if cost strictly improves AND saving exceeds display precision
            // (we round to 3 decimals, so anything < 0.0005 would round to 0).
            let saving = cost_at_cur - best_c;
            if saving > 0.0005 {
                if best_ch != cur_ch { changed = true; }
                assigned[i] = best_ch;
            }
            // else: stay at cur_ch (assigned unchanged)
        }
        if changed { counter = stale_threshold; resets += 1; } else { counter -= 1; }
        if counter == 0 || !changed { break; }
    }

    let cost_after_vec = compute_costs(&assigned);

    let mut out = Vec::with_capacity(aps.len());
    for (i, ap) in aps.iter().enumerate() {
        let cid = cluster_for(&ap.sn);
        let cb = ap.cur_ch;
        let ca = assigned[i];
        out.push(OptimizeRow {
            reg: reg.to_string(),
            sn: ap.sn.clone(),
            clusterid: cid,
            ch_before: Some(cb),
            cost_before: round3(cost_before_vec[i]),
            ch_after: ca,
            cost_after: round3(cost_after_vec[i]),
            status: if ca == cb { "STAY".into() } else { "CHANGE".into() },
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
    if band == "2g" { (1..=14).contains(&ch) } else { ch >= 32 && !DFS_CHANNELS_5G.contains(&ch) }
}

#[inline]
fn cost_for_ap(cand_ch: i32, ap: &ApOpt, table: &OverlapTable, band: &'static str) -> f64 {
    let mut total = 0.0_f64;
    for &(nb_ch, nb_bw, rssi_f, owner_f) in &ap.neighbors {
        let overlap = table.get(cand_ch, nb_ch, nb_bw, band);
        if overlap <= 0.0 { continue; }
        let type_f = if nb_ch == cand_ch { CO_FACTOR } else { ADJ_FACTOR };
        total += overlap * type_f * rssi_f * owner_f;
    }
    total
}