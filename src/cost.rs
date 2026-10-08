use crate::discover::ApInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    G2,
    G5,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub serial: String,
    pub before: Option<u8>,
    pub after: Option<u8>,
    pub cost_before: Option<f64>,
    pub cost_after: Option<f64>,
    pub status: String,
}

fn overlap(band: Band, a: u8, b: u8) -> f64 {
    let d = a.abs_diff(b);
    match band {
        Band::G5 => {
            if d == 0 {
                1.0
            } else {
                0.0
            }
        }
        Band::G2 => match d {
            0 => 1.0,
            1 => 0.5,
            2 => 0.25,
            _ => 0.0,
        },
    }
}

fn rssi_weight(signal: i16, threshold: f32) -> f64 {
    let w = (signal as f32 - threshold + 5.0) / 30.0;
    w.clamp(0.05, 1.0) as f64
}

pub fn score(ap: &ApInfo, channel: u8, band: Band, threshold: f32) -> f64 {
    ap.neighbors
        .iter()
        .filter(|n| {
            let is_5g = n.channel > 14;
            (is_5g == (band == Band::G5)) && n.signal as f32 > threshold
        })
        .map(|n| overlap(band, channel, n.channel) * rssi_weight(n.signal, threshold))
        .sum()
}

fn radio(ap: &ApInfo, band: Band) -> Option<(u8, Vec<u8>)> {
    match band {
        Band::G2 => ap.band2g.as_ref().map(|r| (r.current, r.possible.clone())),
        Band::G5 => ap.band5g.as_ref().map(|r| (r.current, r.possible.clone())),
    }
}

pub fn assign(cluster: &[ApInfo], band: Band, threshold: f32, stay_delta: f64) -> Vec<Assignment> {
    cluster
        .iter()
        .filter_map(|ap| {
            let (current, possible) = radio(ap, band)?;
            if possible.is_empty() {
                return None;
            }
            let cost_before = score(ap, current, band, threshold);
            let mut best = current;
            let mut best_cost = f64::INFINITY;
            for ch in &possible {
                let c = score(ap, *ch, band, threshold);
                if c < best_cost - 1e-9 {
                    best_cost = c;
                    best = *ch;
                }
            }
            let improvement = cost_before - best_cost;
            let (after, cost_after, status) = if improvement <= stay_delta {
                (current, cost_before, "STAY".to_string())
            } else {
                (best, best_cost, "CHANGE".to_string())
            };
            Some(Assignment {
                serial: ap.serial.clone(),
                before: Some(current),
                after: Some(after),
                cost_before: Some(cost_before),
                cost_after: Some(cost_after),
                status,
            })
        })
        .collect()
}
