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

/// Move-acceptance epsilon (same scale as the descent decision rule).
/// A move is reverted when the FINAL assignment prices it worse than staying.
pub const ACCEPT_EPS: f64 = 0.0005;

/// Upper bound on exhaustive combinations per cluster before falling back
/// to coordinate descent for that cluster.
pub const MAX_COMBOS: u64 = 20_000;

#[derive(Debug, Default)]
pub struct BruteStats {
    pub groups: usize,
    pub exhaustive: usize,
    pub fallback: usize,
}

/// Final guard for one AP: keep the candidate only if the final assignment
/// does not price it worse than the current channel beyond noise.
///
/// `cur_legal` = current channel is one of the AP's planned candidates.
/// When it is NOT (non-plan 2.4G channel such as 5, or a DFS 5G channel),
/// the move is never vetoed: leaving the device off-plan is worse than a
/// small local cost increase, and the plan channel is what executors apply.
/// Returns (channel, cost) actually reported.
pub fn accept_move(
    cur: i32,
    cur_cost: f64,
    cand: i32,
    cand_cost: f64,
    cur_legal: bool,
) -> (i32, f64) {
    if !cur_legal {
        return (cand, cand_cost);
    }
    if cand != cur && cand_cost > cur_cost + ACCEPT_EPS {
        (cur, cur_cost)
    } else {
        (cand, cand_cost)
    }
}

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
            if !ap.possible.contains(&cur) || cost_cur - best_c > ACCEPT_EPS {
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
            let cb = ap.cur_ch;
            let cb_cost = round3(cost_for(cb, &ap.neighbors, band));
            let (ca, ca_cost) = accept_move(
                cb,
                cb_cost,
                assigned[i],
                round3(cost_for(assigned[i], &ap.neighbors, band)),
                ap.possible.contains(&cb),
            );
            OptimizeRow {
                sn: ap.sn,
                clusterid: ap.cluster_id,
                ch_before: Some(cb),
                cost_before: cb_cost,
                ch_after: ca,
                cost_after: ca_cost,
                status: if ca == cb { "STAY".into() } else { "CHANGE".into() },
            }
        })
        .collect()
}

struct BfAp {
    sn: String,
    possible: Vec<i32>,
    cur: i32,
    /// (in-group member position or None, fixed channel, weight)
    nb: Vec<(Option<usize>, i32, f64)>,
}

fn bf_cost(band: &str, own_ch: i32, nb: &[(Option<usize>, i32, f64)], combo: &[i32]) -> f64 {
    let mut total = 0.0;
    for &(pos, ch, w) in nb {
        let nc = match pos {
            Some(i) => combo[i],
            None => ch,
        };
        let ov = if band == "2g" {
            overlap_2g(own_ch, nc)
        } else {
            overlap_5g(own_ch, nc)
        };
        if ov <= 0.0 {
            continue;
        }
        total += ov * if nc == own_ch { CO_FACTOR } else { ADJ_FACTOR } * w;
    }
    total
}

/// Joint per-cluster optimizer ("bruteforce").
///
/// Each interference cluster (a connected component: no managed radio outside
/// it hears or is heard by a member, so the joint optimum is exact) is
/// searched exhaustively over the product of member candidate channels.
/// Lexicographic combo order + strict improvement = fully deterministic.
///
/// Groups larger than `max_k` members or with more than [`MAX_COMBOS`]
/// combinations fall back to [`optimize_band`] on that subset (counted in
/// stats). No per-AP acceptance veto here on purpose: a joint optimum may
/// raise one member's individual cost while lowering the total; adoption
/// itself requires beating the current joint total (joint acceptance).
pub fn optimize_band_bruteforce(
    rows: &[MemRow],
    cluster_sns: &[Vec<String>],
    stale: usize,
    max_k: usize,
    band: &str,
) -> (Vec<OptimizeRow>, BruteStats) {
    let mut stats = BruteStats::default();
    let invalid_set: HashSet<&str> = HashSet::from(["00:00:00:00:00:00"]);

    let mut own_set: HashSet<&str> = HashSet::new();
    for r in rows {
        for b in &r.own {
            let s = b.trim();
            if !s.is_empty() && !invalid_set.contains(s) {
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

    // Group row indices: clustered by cid (first-appearance order), then
    // unclustered rows each as their own singleton group. Skip empty-own
    // rows exactly like the descent path (they produce no output row).
    let mut order: Vec<usize> = Vec::new();
    let mut by_cid: HashMap<i64, Vec<usize>> = HashMap::new();
    for (idx, r) in rows.iter().enumerate() {
        if r.own.is_empty() {
            continue;
        }
        match cid_of.get(r.sn.as_str()).copied() {
            Some(cid) => {
                if !by_cid.contains_key(&cid) {
                    order.push(idx);
                }
                by_cid.entry(cid).or_default().push(idx);
            }
            None => order.push(idx),
        }
    }
    // Deterministic member order inside groups.
    for v in by_cid.values_mut() {
        v.sort_by(|&a, &b| rows[a].sn.cmp(&rows[b].sn));
    }

    let mut out: Vec<OptimizeRow> = Vec::new();
    // Iterate groups in first-appearance order for deterministic logging;
    // results are order-independent (no cross-group managed edges).
    let mut seen_cid: HashSet<i64> = HashSet::new();
    let mut groups: Vec<(Option<i64>, Vec<usize>)> = Vec::new();
    for &idx in &order {
        match cid_of.get(rows[idx].sn.as_str()).copied() {
            Some(cid) => {
                if seen_cid.insert(cid) {
                    groups.push((Some(cid), by_cid[&cid].clone()));
                }
            }
            None => groups.push((None, vec![idx])),
        }
    }
    stats.groups = groups.len();

    for (cid, members) in &groups {
        // Build member working sets.
        let mut maps: Vec<BfAp> = Vec::with_capacity(members.len());
        let mut usable: Vec<usize> = Vec::new();
        for &ri in members {
            let r = &rows[ri];
            let possible = possible_channels(r, band);
            if possible.is_empty() {
                continue;
            }
            usable.push(ri);
            maps.push(BfAp {
                sn: r.sn.clone(),
                cur: r.own_channel.unwrap_or(possible[0]),
                possible,
                nb: Vec::new(),
            });
        }
        if maps.is_empty() {
            continue;
        }
        let mut bpos: HashMap<&str, usize> = HashMap::new();
        for (pos, &ri) in usable.iter().enumerate() {
            for b in &rows[ri].own {
                bpos.insert(b.trim(), pos);
            }
        }
        for (pos, bfap) in maps.iter_mut().enumerate() {
            let r = &rows[usable[pos]];
            let mut nb = Vec::with_capacity(r.neighbors.len());
            for (bssid, ch, rssi) in &r.neighbors {
                let s = bssid.trim();
                if s.is_empty() || invalid_set.contains(s) || !band_cost_ok(*ch, band) {
                    continue;
                }
                let w = rssi_factor(*rssi)
                    * if own_set.contains(s) {
                        OWN_GATEWAY
                    } else {
                        EXTERNAL
                    };
                // Self/sibling radios of the SAME AP stay priced at their
                // scanned channel, exactly like the descent path prices them
                // (stale). Resolving them via combo would force a constant
                // co-channel term that descent never sees, breaking parity.
                let slot = bpos.get(s).copied().filter(|&p| p != pos);
                nb.push((slot, *ch, w));
            }
            bfap.nb = nb;
        }

        let exhaustive = maps.len() <= max_k.max(1)
            && maps
                .iter()
                .map(|m| m.possible.len() as u64)
                .fold(1u64, |a, b| a.saturating_mul(b))
                <= MAX_COMBOS;
        if !exhaustive {
            stats.fallback += 1;
            let sub: Vec<MemRow> = usable.iter().map(|&ri| rows[ri].clone()).collect();
            let sub_sns = vec![sub.iter().map(|r| r.sn.clone()).collect::<Vec<_>>()];
            let mut sub_rows = optimize_band(&sub, &sub_sns, stale, band);
            for r in sub_rows.iter_mut() {
                r.clusterid = *cid;
            }
            out.extend(sub_rows);
            continue;
        }
        stats.exhaustive += 1;

        // Current combo is adoptable only when every cur is a real candidate;
        // otherwise the argmin is forced (same rule as the descent path).
        let cur_combo: Option<Vec<i32>> = maps
            .iter()
            .map(|m| {
                if m.possible.contains(&m.cur) {
                    Some(m.cur)
                } else {
                    None
                }
            })
            .collect();
        // Baseline is always the real current state (even for off-plan
        // channels), so the reported before/after pair is comparable.
        let before_combo: Vec<i32> = maps.iter().map(|m| m.cur).collect();
        // Off-plan currents are canonicalized by force: no baseline to beat.
        let mut best_total = if cur_combo.is_some() {
            let t: f64 = maps
                .iter()
                .enumerate()
                .map(|(i, m)| bf_cost(band, before_combo[i], &m.nb, &before_combo))
                .sum();
            t - ACCEPT_EPS
        } else {
            f64::INFINITY
        };
        let mut best_combo = cur_combo.clone().unwrap_or_else(|| {
            maps.iter().map(|m| m.possible[0]).collect()
        });
        let dbg = std::env::var("WIFIOPT_DEBUG_CID")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| cid == &Some(*v));

        let lens: Vec<usize> = maps.iter().map(|m| m.possible.len()).collect();
        let mut idx = vec![0usize; maps.len()];
        loop {
            let combo: Vec<i32> = idx
                .iter()
                .enumerate()
                .map(|(i, &k)| maps[i].possible[k])
                .collect();
            let total: f64 = maps
                .iter()
                .enumerate()
                .map(|(i, m)| bf_cost(band, combo[i], &m.nb, &combo))
                .sum();
            if total < best_total {
                best_total = total;
                best_combo = combo;
            }
            let mut j = 0;
            while j < idx.len() {
                idx[j] += 1;
                if idx[j] < lens[j] {
                    break;
                }
                idx[j] = 0;
                j += 1;
            }
            if j == idx.len() {
                break;
            }
        }

        if dbg.is_some() {
            let cur_t: f64 = maps
                .iter()
                .enumerate()
                .map(|(i, m)| bf_cost(band, before_combo[i], &m.nb, &before_combo))
                .sum();
            let best_t: f64 = maps
                .iter()
                .enumerate()
                .map(|(i, m)| bf_cost(band, best_combo[i], &m.nb, &best_combo))
                .sum();
            let cur_ok = cur_combo.is_some();
            eprintln!(
                "[dbg cid={cid:?} band={band}] cur_ok={cur_ok} cur_total={cur_t:.6} best_total={best_t:.6}"
            );
            for (i, m) in maps.iter().enumerate() {
                eprintln!(
                    "  {} cur={} poss={:?} -> best={} nb_len={}",
                    m.sn,
                    m.cur,
                    m.possible,
                    best_combo[i],
                    m.nb.len()
                );
            }
        }

        for (i, m) in maps.iter().enumerate() {
            let cb = m.cur;
            let ca = best_combo[i];
            // Fair comparison, both sides "real":
            //   before = current channel priced against neighbors' CURRENT
            //            channels (the scanned baseline, same convention as
            //            the descent path),
            //   after  = planned channel priced against neighbors' PLANNED
            //            channels (the projected outcome).
            let cb_cost = round3(bf_cost(band, cb, &m.nb, &before_combo));
            let ca_cost = round3(bf_cost(band, ca, &m.nb, &best_combo));
            out.push(OptimizeRow {
                sn: m.sn.clone(),
                clusterid: *cid,
                ch_before: Some(cb),
                cost_before: cb_cost,
                ch_after: ca,
                cost_after: ca_cost,
                status: if ca == cb { "STAY".into() } else { "CHANGE".into() },
            });
        }
    }
    (out, stats)
}
