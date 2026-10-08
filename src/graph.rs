use std::collections::HashMap;

use crate::discover::ApInfo;

#[derive(Debug, Clone)]
pub struct Clusters {
    pub g2: Vec<Vec<usize>>,
    pub g5: Vec<Vec<usize>>,
}

struct Union {
    parent: Vec<usize>,
}

impl Union {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            let r = self.find(self.parent[x]);
            self.parent[x] = r;
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

fn band_clusters(aps: &[ApInfo], serial_idx: &HashMap<String, usize>, want_5g: bool, threshold: f32) -> Vec<Vec<usize>> {
    let n = aps.len();
    let mut u = Union::new(n);
    for (i, ap) in aps.iter().enumerate() {
        let has_band = if want_5g {
            ap.band5g.is_some()
        } else {
            ap.band2g.is_some()
        };
        if !has_band {
            continue;
        }
        for nbr in &ap.neighbors {
            if nbr.signal as f32 <= threshold {
                continue;
            }
            let is_5g = nbr.channel > 14;
            if is_5g != want_5g {
                continue;
            }
            if let Some(&j) = serial_idx.get(&nbr.bssid.to_uppercase()) {
                if j != i {
                    u.union(i, j);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, ap) in aps.iter().enumerate() {
        let has_band = if want_5g {
            ap.band5g.is_some()
        } else {
            ap.band2g.is_some()
        };
        if !has_band {
            continue;
        }
        groups.entry(u.find(i)).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    for g in out.iter_mut() {
        g.sort_by_key(|&i| aps[i].serial.clone());
    }
    out.sort_by_key(|g| aps[g[0]].serial.clone());
    out
}

pub fn clusters(
    aps: &[ApInfo],
    bssid_index: &HashMap<String, String>,
    threshold: f32,
) -> Clusters {
    let serial_idx: HashMap<String, usize> = aps
        .iter()
        .enumerate()
        .map(|(i, a)| (a.serial.clone(), i))
        .collect();
    let norm: HashMap<String, usize> = bssid_index
        .iter()
        .filter_map(|(b, s)| serial_idx.get(s).map(|&i| (b.to_uppercase(), i)))
        .collect();
    Clusters {
        g2: band_clusters(aps, &norm, false, threshold),
        g5: band_clusters(aps, &norm, true, threshold),
    }
}
