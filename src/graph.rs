use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::discover::{ApInfo, INVALID_BSSID};

#[derive(Debug, Clone)]
pub struct MemRow {
    pub sn: String,
    pub own: Vec<String>,
    pub neighbors: Vec<(String, i32, i32)>,
    pub own_channel: Option<i32>,
    pub possible_channels: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct Clusters {
    pub valid: Vec<Vec<String>>,
}

struct Dsu {
    p: Vec<u32>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            p: (0..n as u32).collect(),
        }
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.p[x as usize] != x {
            let p = self.p[x as usize];
            self.p[x as usize] = self.p[p as usize];
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.p[ra as usize] = rb;
        }
    }
}

pub fn band_ok(ch: i32, band: &str) -> bool {
    match band {
        "2g" => (1..=13).contains(&ch),
        "5g" => (36..=165).contains(&ch),
        _ => false,
    }
}

fn mem_rows(aps: &[ApInfo], band: &str) -> Vec<MemRow> {
    aps.iter()
        .map(|ap| {
            let (radios, neighbors) = if band == "2g" {
                (&ap.radios_2g, &ap.neighbors_2g)
            } else {
                (&ap.radios_5g, &ap.neighbors_5g)
            };
            let first = radios.first();
            MemRow {
                sn: ap.serial.clone(),
                own: radios.iter().map(|r| r.bssid.clone()).collect(),
                neighbors: neighbors
                    .iter()
                    .map(|n| (n.bssid.clone(), n.channel as i32, n.signal))
                    .collect(),
                own_channel: first.and_then(|r| r.channel.map(|c| c as i32)),
                possible_channels: first
                    .map(|r| r.possible.iter().map(|&c| c as i32).collect())
                    .unwrap_or_default(),
            }
        })
        .collect()
}

pub fn cluster(aps: &[ApInfo], band: &str, min_size: usize) -> Result<Clusters> {
    let rows = mem_rows(aps, band);
    let min_size = min_size.max(1);

    let mut own_set: HashSet<&str> = HashSet::new();
    for r in &rows {
        for b in &r.own {
            let s = b.trim();
            if !s.is_empty() && s != INVALID_BSSID {
                own_set.insert(s);
            }
        }
    }

    let n = rows.len();
    let mut id_of: HashMap<&str, u32> = HashMap::new();
    let mut next = n as u32;
    for &b in &own_set {
        id_of.insert(b, next);
        next += 1;
    }

    let mut dsu = Dsu::new(next as usize);
    for (i, r) in rows.iter().enumerate() {
        let u = i as u32;
        for b in &r.own {
            if let Some(&id) = id_of.get(b.trim()) {
                dsu.union(u, id);
            }
        }
        for (b, ch, _) in &r.neighbors {
            if !band_ok(*ch, band) {
                continue;
            }
            if let Some(&id) = id_of.get(b.trim()) {
                dsu.union(u, id);
            }
        }
    }

    let mut comps: HashMap<u32, Vec<String>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        comps.entry(dsu.find(i as u32)).or_default().push(r.sn.clone());
    }
    let mut valid: Vec<Vec<String>> = comps
        .into_values()
        .filter(|v| v.len() >= min_size)
        .collect();
    for v in valid.iter_mut() {
        v.sort();
    }
    valid.sort_by(|a, b| (a.len(), &a[0]).cmp(&(b.len(), &b[0])));
    Ok(Clusters { valid })
}

pub fn mem_rows_for(aps: &[ApInfo], band: &str) -> Vec<MemRow> {
    mem_rows(aps, band)
}
