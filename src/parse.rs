use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct RadioConfig {
    #[serde(rename = "ssid_enable")]
    pub ssid_enable: bool,
    #[serde(default)]
    pub bssid: String,
    #[serde(default)]
    pub channel: Value,
    #[serde(default)]
    pub possible_channel: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Neighbor {
    #[serde(deserialize_with = "de_bssid_lenient")]
    pub bssid: String,
    #[serde(deserialize_with = "de_u8_lenient")]
    pub channel: u8,
    #[serde(rename = "signal_strength", deserialize_with = "de_i32_lenient")]
    pub signal_strength: i32,
}

fn de_bssid_lenient<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::String(s) => Ok(s),
        Value::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

fn de_u8_lenient<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Number(n) => n.as_u64().and_then(|x| u8::try_from(x).ok()).ok_or_else(|| serde::de::Error::custom("out of range")),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { Ok(0) } else { t.parse::<u8>().map_err(serde::de::Error::custom) }
        }
        Value::Null => Ok(0),
        _ => Err(serde::de::Error::custom("expected number or string")),
    }
}

fn de_i32_lenient<'de, D>(d: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Number(n) => n.as_i64().and_then(|x| i32::try_from(x).ok()).ok_or_else(|| serde::de::Error::custom("number out of range")),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { Ok(0) } else { t.parse::<i32>().map_err(serde::de::Error::custom) }
        }
        Value::Null => Ok(0),
        _ => Err(serde::de::Error::custom("expected number or string")),
    }
}

pub type ConfigMap = HashMap<String, RadioConfig>;
pub type NeighborMap = HashMap<String, Neighbor>;

pub fn parse_config(s: &str) -> Result<ConfigMap> {
    let v: Value = serde_json::from_str(s).map_err(|e| anyhow!("json_config: {e}"))?;
    match v {
        Value::Object(_) => serde_json::from_value(v).map_err(|e| anyhow!("json_config: {e}")),
        _ => Ok(ConfigMap::new()),
    }
}

pub fn parse_neighbor(s: &str) -> Result<NeighborMap> {
    let v: Value = serde_json::from_str(s).map_err(|e| anyhow!("json_neighbor: {e}"))?;
    match v {
        Value::Object(_) => serde_json::from_value(v).map_err(|e| anyhow!("json_neighbor: {e}")),
        _ => Ok(NeighborMap::new()),
    }
}

pub fn channel_to_u8(v: &Value) -> Option<u8> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|x| u8::try_from(x).ok()),
        Value::String(s) => s.parse::<u8>().ok(),
        _ => None,
    }
}

pub fn parse_possible_ch(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for tok in s.split(',') {
        let t = tok.trim();
        if let Some((lo, hi)) = t.split_once('-') {
            if let (Ok(a), Ok(b)) = (lo.parse::<u8>(), hi.parse::<u8>()) {
                for v in a..=b {
                    out.push(v);
                }
            }
        } else if let Ok(v) = t.parse::<u8>() {
            out.push(v);
        }
    }
    out
}

pub fn pick_own_radios(cfg: &ConfigMap) -> (Option<&RadioConfig>, Option<&RadioConfig>) {
    let mut g2: Option<&RadioConfig> = None;
    let mut g5: Option<&RadioConfig> = None;
    for r in cfg.values() {
        if !r.ssid_enable {
            continue;
        }
        let Some(ch) = channel_to_u8(&r.channel) else { continue };
        if ch == 0 {
            continue;
        }
        if ch <= 14 && g2.is_none() {
            g2 = Some(r);
        } else if ch > 14 && g5.is_none() {
            g5 = Some(r);
        }
    }
    (g2, g5)
}

#[derive(Debug, Serialize)]
pub struct OwnedRadio {
    pub channel: Option<u8>,
    pub bssid: String,
    pub possible_channel: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct NeighRow {
    pub channel: u8,
    pub bssid: String,
    pub rssi: i32,
}

#[derive(Debug, Serialize)]
pub struct ParsedRow {
    pub sn: String,
    pub ch_ont_2g: Option<u8>,
    pub ch_ont_5g: Option<u8>,
    pub own_2g: Option<OwnedRadio>,
    pub own_5g: Option<OwnedRadio>,
    pub neighbors: Vec<NeighRow>,
}

pub fn build_parsed(
    sn: String,
    ch_ont_2g: Option<u8>,
    ch_ont_5g: Option<u8>,
    cfg: &ConfigMap,
    nei: &NeighborMap,
) -> ParsedRow {
    let (g2, g5) = pick_own_radios(cfg);
    let to_owned = |r: Option<&RadioConfig>| {
        r.map(|r| OwnedRadio {
            channel: channel_to_u8(&r.channel),
            bssid: r.bssid.clone(),
            possible_channel: r
                .possible_channel
                .as_deref()
                .map(parse_possible_ch)
                .unwrap_or_default(),
        })
    };
    let neighbors: Vec<NeighRow> = nei
        .values()
        .map(|v| NeighRow {
            channel: v.channel,
            bssid: v.bssid.clone(),
            rssi: v.signal_strength,
        })
        .collect();
    ParsedRow {
        sn,
        ch_ont_2g,
        ch_ont_5g,
        own_2g: to_owned(g2),
        own_5g: to_owned(g5),
        neighbors,
    }
}

fn owned_from_cfg(r: &RadioConfig) -> OwnedRadio {
    OwnedRadio {
        channel: channel_to_u8(&r.channel),
        bssid: r.bssid.trim().to_lowercase(),
        possible_channel: r
            .possible_channel
            .as_deref()
            .map(parse_possible_ch)
            .unwrap_or_default(),
    }
}

pub fn split_own_by_slot(cfg: &ConfigMap) -> (Vec<OwnedRadio>, Vec<OwnedRadio>) {
    let (mut g2, mut g5) = (Vec::new(), Vec::new());
    for (k, r) in cfg {
        if !r.ssid_enable {
            continue;
        }
        let bssid_lc = r.bssid.trim().to_lowercase();
        if bssid_lc.is_empty() || bssid_lc == "00:00:00:00:00:00" {
            continue;
        }
        let slot: u8 = k.parse().unwrap_or(0);
        if slot >= 1 && slot <= 4 {
            g2.push(owned_from_cfg(r));
        } else if slot >= 5 {
            g5.push(owned_from_cfg(r));
        }
    }
    (g2, g5)
}

pub fn split_neighbors_by_band(
    nei: &NeighborMap,
    invalid_bssid: &std::collections::HashSet<String>,
) -> (Vec<NeighRow>, Vec<NeighRow>) {
    let (mut g2, mut g5) = (Vec::new(), Vec::new());
    for v in nei.values() {
        let b = v.bssid.trim().to_lowercase();
        if b.is_empty() || invalid_bssid.contains(&b) {
            continue;
        }
        let r = NeighRow { channel: v.channel, bssid: b.clone(), rssi: v.signal_strength };
        if v.channel == 0 {
            continue;
        }
        if v.channel <= 13 {
            g2.push(r);
        } else {
            g5.push(r);
        }
    }
    (g2, g5)
}
