use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::db::DiscoverRow;

const DEFAULT_2G: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
const DEFAULT_5G: &[u8] = &[36, 40, 44, 48, 52, 56, 60, 64, 149, 153, 157, 161];

#[derive(Debug, Clone)]
pub struct RadioInfo {
    pub current: u8,
    pub possible: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Neighbor {
    pub bssid: String,
    pub channel: u8,
    pub signal: i16,
}

#[derive(Debug, Clone)]
pub struct ApInfo {
    pub serial: String,
    pub band2g: Option<RadioInfo>,
    pub band5g: Option<RadioInfo>,
    pub neighbors: Vec<Neighbor>,
}

#[derive(Debug, Deserialize)]
struct CfgEntry {
    #[serde(default)]
    channel: Option<serde_json::Value>,
    #[serde(default)]
    possible_channel: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct NbEntry {
    #[serde(default)]
    bssid: Option<String>,
    #[serde(default)]
    channel: Option<serde_json::Value>,
    #[serde(default)]
    signal_strength: Option<serde_json::Value>,
}

fn to_u8(v: &serde_json::Value) -> Option<u8> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().and_then(|x| u8::try_from(x).ok()),
        serde_json::Value::String(s) => s.trim().parse::<u8>().ok(),
        _ => None,
    }
}

fn to_i16(v: &serde_json::Value) -> Option<i16> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().and_then(|x| i16::try_from(x).ok()),
        serde_json::Value::String(s) => s.trim().parse::<i16>().ok(),
        _ => None,
    }
}

fn parse_possible(raw: &Option<serde_json::Value>, band2g: bool) -> Vec<u8> {
    let mut out: Vec<u8> = match raw {
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .filter_map(|p| p.trim().parse::<u8>().ok())
            .collect(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(to_u8).collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|c| if band2g { *c >= 1 && *c <= 14 } else { *c >= 32 })
    .collect();
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        out = if band2g {
            DEFAULT_2G.to_vec()
        } else {
            DEFAULT_5G.to_vec()
        };
    }
    out
}

pub fn parse(row: &DiscoverRow) -> Result<ApInfo> {
    let cfg: BTreeMap<String, CfgEntry> = if row.json_config.trim().is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(&row.json_config)?
    };
    let mut band2g: Option<RadioInfo> = None;
    let mut band5g: Option<RadioInfo> = None;
    for entry in cfg.values() {
        let Some(chv) = &entry.channel else { continue };
        let Some(ch) = to_u8(chv) else { continue };
        if ch <= 14 && band2g.is_none() {
            band2g = Some(RadioInfo {
                current: ch,
                possible: parse_possible(&entry.possible_channel, true),
            });
        } else if ch > 14 && band5g.is_none() {
            band5g = Some(RadioInfo {
                current: ch,
                possible: parse_possible(&entry.possible_channel, false),
            });
        }
    }
    if band2g.is_none() {
        if let Some(ch) = row.channel_ont {
            band2g = Some(RadioInfo {
                current: ch,
                possible: DEFAULT_2G.to_vec(),
            });
        }
    }
    if band5g.is_none() {
        if let Some(ch) = row.channel_ont5g {
            band5g = Some(RadioInfo {
                current: ch,
                possible: DEFAULT_5G.to_vec(),
            });
        }
    }

    let mut neighbors = Vec::new();
    let s = row.json_neighbor.trim();
    if !s.is_empty() && s != "[]" {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, NbEntry>>(s) {
            for e in map.values() {
                let (Some(bssid), Some(chv), Some(sgv)) =
                    (&e.bssid, &e.channel, &e.signal_strength)
                else {
                    continue;
                };
                let (Some(ch), Some(sig)) = (to_u8(chv), to_i16(sgv)) else {
                    continue;
                };
                neighbors.push(Neighbor {
                    bssid: bssid.clone(),
                    channel: ch,
                    signal: sig,
                });
            }
        }
    }

    Ok(ApInfo {
        serial: row.serial.clone(),
        band2g,
        band5g,
        neighbors,
    })
}
