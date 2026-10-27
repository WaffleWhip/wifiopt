use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::db::DiscoverRow;

pub const INVALID_BSSID: &str = "00:00:00:00:00:00";

#[derive(Debug, Clone)]
pub struct OwnedRadio {
    pub slot: u8,
    pub channel: Option<u8>,
    pub bssid: String,
    pub possible: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Neighbor {
    pub bssid: String,
    pub channel: u8,
    pub signal: i32,
}

#[derive(Debug, Clone)]
pub struct ApInfo {
    pub serial: String,
    pub radios_2g: Vec<OwnedRadio>,
    pub radios_5g: Vec<OwnedRadio>,
    pub neighbors_2g: Vec<Neighbor>,
    pub neighbors_5g: Vec<Neighbor>,
}

#[derive(Debug, Deserialize)]
struct RadioConfig {
    ssid_enable: bool,
    #[serde(default)]
    bssid: String,
    #[serde(default)]
    channel: Value,
    #[serde(default)]
    possible_channel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NbRaw {
    #[serde(deserialize_with = "de_bssid")]
    bssid: String,
    #[serde(deserialize_with = "de_u8")]
    channel: u8,
    #[serde(rename = "signal_strength", deserialize_with = "de_i32")]
    signal: i32,
}

fn de_bssid<'de, D>(d: D) -> Result<String, D::Error>
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

fn de_u8<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Number(n) => n
            .as_u64()
            .and_then(|x| u8::try_from(x).ok())
            .ok_or_else(|| serde::de::Error::custom("out of range")),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(0)
            } else {
                t.parse::<u8>().map_err(serde::de::Error::custom)
            }
        }
        Value::Null => Ok(0),
        _ => Err(serde::de::Error::custom("expected number or string")),
    }
}

fn de_i32<'de, D>(d: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Number(n) => n
            .as_i64()
            .and_then(|x| i32::try_from(x).ok())
            .ok_or_else(|| serde::de::Error::custom("out of range")),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(0)
            } else {
                t.parse::<i32>().map_err(serde::de::Error::custom)
            }
        }
        Value::Null => Ok(0),
        _ => Err(serde::de::Error::custom("expected number or string")),
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
            if let (Ok(a), Ok(b)) = (lo.trim().parse::<u8>(), hi.trim().parse::<u8>()) {
                out.extend(a..=b);
            }
        } else if let Ok(v) = t.parse::<u8>() {
            out.push(v);
        }
    }
    out
}

fn parse_map<T>(s: &str) -> BTreeMap<String, T>
where
    T: for<'de> Deserialize<'de>,
{
    let t = s.trim();
    if t.is_empty() || t == "{}" || t == "null" {
        return BTreeMap::new();
    }
    serde_json::from_str::<BTreeMap<String, T>>(t).unwrap_or_default()
}

pub fn parse(row: &DiscoverRow) -> Result<ApInfo> {
    let cfg: BTreeMap<String, RadioConfig> = parse_map(&row.json_config);
    let nei: BTreeMap<String, NbRaw> = parse_map(&row.json_neighbor);

    let mut radios_2g: Vec<(u8, OwnedRadio)> = Vec::new();
    let mut radios_5g: Vec<(u8, OwnedRadio)> = Vec::new();
    for (k, r) in &cfg {
        if !r.ssid_enable {
            continue;
        }
        let b = r.bssid.trim().to_lowercase();
        if b.is_empty() || b == INVALID_BSSID {
            continue;
        }
        let radio = OwnedRadio {
            slot: 0,
            channel: channel_to_u8(&r.channel),
            bssid: b,
            possible: r
                .possible_channel
                .as_deref()
                .map(parse_possible_ch)
                .unwrap_or_default(),
        };
        // Numeric slot order: BTreeMap<String> sorts "10" before "5",
        // and the old HashMap order was random per run (misread radio bug).
        let slot: u8 = k.parse().unwrap_or(0);
        if slot == 0 {
            continue;
        }
        if (1..=4).contains(&slot) {
            radios_2g.push((slot, radio));
        } else if slot >= 5 {
            radios_5g.push((slot, radio));
        }
    }
    radios_2g.sort_by_key(|(s, _)| *s);
    radios_5g.sort_by_key(|(s, _)| *s);
    let mut radios_2g_out = Vec::with_capacity(radios_2g.len());
    for (slot, mut radio) in radios_2g {
        radio.slot = slot;
        radios_2g_out.push(radio);
    }
    let mut radios_5g_out = Vec::with_capacity(radios_5g.len());
    for (slot, mut radio) in radios_5g {
        radio.slot = slot;
        radios_5g_out.push(radio);
    }

    let mut neighbors_2g = Vec::new();
    let mut neighbors_5g = Vec::new();
    for v in nei.values() {
        let b = v.bssid.trim().to_lowercase();
        if b.is_empty() || b == INVALID_BSSID || v.channel == 0 {
            continue;
        }
        let nb = Neighbor {
            bssid: b,
            channel: v.channel,
            signal: v.signal,
        };
        if v.channel <= 13 {
            neighbors_2g.push(nb);
        } else {
            neighbors_5g.push(nb);
        }
    }

    Ok(ApInfo {
        serial: row.serial.clone(),
        radios_2g: radios_2g_out,
        radios_5g: radios_5g_out,
        neighbors_2g,
        neighbors_5g,
    })
}

/// True when >=2 enabled same-band radios report different known channels.
/// Such ONTs confused the old reader (random radio picked per run);
/// the optimizer now always reads the primary radio (see graph::mem_rows).
pub fn divergent_radios(ap: &ApInfo, band: &str) -> bool {
    let radios = if band == "2g" {
        &ap.radios_2g
    } else {
        &ap.radios_5g
    };
    let mut seen: Vec<u8> = Vec::new();
    for r in radios {
        if let Some(c) = r.channel {
            if !seen.contains(&c) {
                seen.push(c);
            }
            if seen.len() > 1 {
                return true;
            }
        }
    }
    false
}
