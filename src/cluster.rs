use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::Config;

#[derive(Debug, Default)]
struct Dsu {
    p: HashMap<String, String>,
}

impl Dsu {
    fn find(&mut self, x: &str) -> String {
        if !self.p.contains_key(x) {
            self.p.insert(x.to_string(), x.to_string());
            return x.to_string();
        }
        let mut cur = x.to_string();
        while self.p[&cur] != cur {
            let gp = self.p[&self.p[&cur].clone()].clone();
            self.p.insert(cur.clone(), gp.clone());
            cur = gp;
        }
        cur
    }
    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.p.insert(ra, rb);
        }
    }
}

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

pub fn cluster_dist_in_mem(rows: &[MemRow], band: &str, cfg: &Config) -> Result<ClusterResult> {
    let invalid = cfg.invalid_bssid_set();
    let mut own_set: HashSet<String> = HashSet::new();
    for r in rows {
        let mut seen: HashSet<String> = HashSet::new();
        for b in &r.own {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) { continue; }
            if seen.insert(b.clone()) { own_set.insert(b); }
        }
    }
    let cluster_min = cfg.cluster.min_size.max(1);

    let mut dsu = Dsu::default();
    for r in rows {
        let sk = format!("s:{}", r.sn);
        let mut seen: HashSet<String> = HashSet::new();
        for b in &r.own {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) { continue; }
            if !seen.insert(b.clone()) { continue; }
            dsu.union(&sk, &format!("b:{b}"));
        }
        for (b, ch, _rssi) in &r.neighbors {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) { continue; }
            if !own_set.contains(&b) { continue; }
            if !cfg.band_ok(*ch, band) { continue; }
            if !seen.insert(b.clone()) { continue; }
            dsu.union(&sk, &format!("b:{b}"));
        }
    }

    let mut comp_sns: HashMap<String, Vec<String>> = HashMap::new();
    let mut comp_bssids: HashMap<String, Vec<String>> = HashMap::new();
    let keys: Vec<String> = dsu.p.keys().cloned().collect();
    for node in keys {
        let root = dsu.find(&node);
        if node.starts_with('s') {
            comp_sns.entry(root).or_default().push(node[2..].to_string());
        } else if node.starts_with('b') {
            comp_bssids.entry(root).or_default().push(node[2..].to_string());
        }
    }
    let mut valid: Vec<(Vec<String>, Vec<String>)> = comp_sns
        .into_iter()
        .filter(|(_, sns)| sns.len() >= cluster_min)
        .map(|(r, sns)| (sns, comp_bssids.remove(&r).unwrap_or_default()))
        .collect();
    valid.sort_by(|a, b| a.0.len().cmp(&b.0.len()));
    Ok(ClusterResult { valid, cluster_min })
}