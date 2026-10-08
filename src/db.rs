use anyhow::{Context, Result};
use mysql::prelude::Queryable;
use std::collections::HashMap;

use crate::config::Options;

#[derive(Debug, Clone)]
pub struct DiscoverRow {
    pub serial: String,
    pub channel_ont: Option<u8>,
    pub channel_ont5g: Option<u8>,
    pub json_config: String,
    pub json_neighbor: String,
}

const DISCOVER_SQL: &str = "SELECT `CPE_Serial_Number`, `channel_ont`, `channel_ont5Ghz`, `json_config`, `json_neighbor` FROM `{table}`";
const BSSID_SQL: &str = "SELECT `CPE_Serial_Number`, `bssid` FROM `list_sn_bssid`";
const BSSID_NEW_SQL: &str = "SELECT `CPE_Serial_Number`, `bssid` FROM `list_sn_bssid_new`";

pub fn all_statements() -> Vec<String> {
    vec![
        DISCOVER_SQL.to_string(),
        BSSID_SQL.to_string(),
        BSSID_NEW_SQL.to_string(),
    ]
}

pub fn with_retry<F, T>(mut f: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                attempt += 1;
                if attempt > 2 {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(200 * attempt as u64));
            }
        }
    }
}

fn check_name(date: &str, reg: &str) -> Result<String> {
    if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
        anyhow::bail!("bad date: {date}");
    }
    if reg.is_empty() || !reg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        anyhow::bail!("bad reg: {reg}");
    }
    Ok(format!("discover_interference_reborn_{date}_{reg}"))
}

pub fn load_discover(opts: &Options, reg: &str) -> Result<Vec<DiscoverRow>> {
    let table = check_name(&opts.date, reg)?;
    let sql = DISCOVER_SQL.replace("{table}", &table);
    with_retry(|| {
        let pool = mysql::Pool::new(opts.db_url.as_str()).context("db pool")?;
        let mut conn = pool.get_conn().context("db conn")?;
        let rows: Vec<(String, Option<u8>, Option<u8>, Option<String>, Option<String>)> =
            conn.query_map(&sql, |(a, b, c, d, e)| (a, b, c, d, e))?;
        Ok(rows
            .into_iter()
            .map(
                |(serial, channel_ont, channel_ont5g, json_config, json_neighbor)| DiscoverRow {
                    serial,
                    channel_ont,
                    channel_ont5g,
                    json_config: json_config.unwrap_or_default(),
                    json_neighbor: json_neighbor.unwrap_or_default(),
                },
            )
            .collect())
    })
}

pub fn load_bssid_index(db_url: &str) -> Result<HashMap<String, String>> {
    with_retry(|| {
        let pool = mysql::Pool::new(db_url).context("db pool")?;
        let mut conn = pool.get_conn().context("db conn")?;
        let mut map = HashMap::new();
        for sql in [BSSID_SQL, BSSID_NEW_SQL] {
            let rows: Vec<(String, String)> = conn.query_map(sql, |(a, b)| (a, b))?;
            for (serial, bssid) in rows {
                map.insert(bssid.to_uppercase(), serial);
            }
        }
        Ok(map)
    })
}
