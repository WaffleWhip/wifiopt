use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::Config;

#[derive(Debug, Clone)]
pub struct ClusterResult {
    pub valid: Vec<(Vec<String>, Vec<String>)>,
    pub cluster_min: usize,
}

pub struct MemRow {
    pub sn: String,
    pub own: Vec<String>,
    pub neighbors: Vec<(String, i32, i32)>,
    pub own_channel: Option<i32>,
    pub possible_channels: Vec<i32>,
}

struct IntDsu {
    p: Vec<u32>,
}

impl IntDsu {
    fn new(size: usize) -> Self {
        Self {
            p: (0..size as u32).collect(),
        }
    }

    #[inline]
    fn find(&mut self, mut x: u32) -> u32 {
        while self.p[x as usize] != x {
            let p = self.p[x as usize];
            self.p[x as usize] = self.p[p as usize];
            x = p;
        }
        x
    }

    #[inline]
    fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.p[ra as usize] = rb;
        }
    }
}

pub fn cluster_dist_in_mem(rows: &[MemRow], band: &str, cfg: &Config) -> Result<ClusterResult> {
    let invalid = cfg.invalid_bssid_set();
    let mut own_set: HashSet<&str> = HashSet::with_capacity(rows.len() * 2);
    for r in rows {
        for b in &r.own {
            let b_str = b.trim();
            if b_str.is_empty() || invalid.contains(b_str) {
                continue;
            }
            own_set.insert(b_str);
        }
    }
    let cluster_min = cfg.cluster.min_size.max(1);

    let num_sns = rows.len();
    let mut bssid_to_id: HashMap<&str, u32> = HashMap::with_capacity(own_set.len());
    let mut bssid_list: Vec<&str> = Vec::with_capacity(own_set.len());
    let mut next_bssid_id = num_sns as u32;

    for &b in &own_set {
        bssid_to_id.insert(b, next_bssid_id);
        bssid_list.push(b);
        next_bssid_id += 1;
    }

    let total_nodes = next_bssid_id as usize;
    let mut dsu = IntDsu::new(total_nodes);

    for (sn_id, r) in rows.iter().enumerate() {
        let u_sn = sn_id as u32;

        for b in &r.own {
            let b_str = b.trim();
            if let Some(&b_id) = bssid_to_id.get(b_str) {
                dsu.union(u_sn, b_id);
            }
        }

        for (b, ch, _rssi) in &r.neighbors {
            let b_str = b.trim();
            if !cfg.band_ok(*ch, band) {
                continue;
            }
            if let Some(&b_id) = bssid_to_id.get(b_str) {
                dsu.union(u_sn, b_id);
            }
        }
    }

    let mut comp_sns: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut comp_bssids: HashMap<u32, Vec<u32>> = HashMap::new();

    for sn_id in 0..num_sns as u32 {
        let root = dsu.find(sn_id);
        comp_sns.entry(root).or_default().push(sn_id);
    }

    for (idx, _) in bssid_list.iter().enumerate() {
        let b_id = (num_sns + idx) as u32;
        let root = dsu.find(b_id);
        comp_bssids.entry(root).or_default().push(idx as u32);
    }

    let mut valid: Vec<(Vec<String>, Vec<String>)> = comp_sns
        .into_iter()
        .filter(|(_, sn_ids)| sn_ids.len() >= cluster_min)
        .map(|(root, sn_ids)| {
            let sns: Vec<String> = sn_ids
                .into_iter()
                .map(|id| rows[id as usize].sn.clone())
                .collect();
            let bssids: Vec<String> = comp_bssids
                .remove(&root)
                .unwrap_or_default()
                .into_iter()
                .map(|idx| bssid_list[idx as usize].to_string())
                .collect();
            (sns, bssids)
        })
        .collect();

    valid.sort_by_key(|a| a.0.len());
    Ok(ClusterResult { valid, cluster_min })
}
