use crate::cluster::MemRow;
use crate::optimize::OptimizeRow;
use crate::parse;
use anyhow::{Context, Result};
use sqlx::Row;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use std::collections::HashMap;
use std::env;
use std::fmt::Write;

pub async fn connect() -> Result<MySqlPool> {
    let host = env::var("MYSQL_HOST").context("MYSQL_HOST missing")?;
    let port: u16 = env::var("MYSQL_PORT")
        .unwrap_or_else(|_| "3306".to_string())
        .parse()
        .context("MYSQL_PORT invalid")?;
    let user = env::var("MYSQL_USER").context("MYSQL_USER missing")?;
    let pass = env::var("MYSQL_PASSWORD").context("MYSQL_PASSWORD missing")?;
    let db = env::var("MYSQL_DATABASE").unwrap_or_else(|_| "wifi_optimization".to_string());
    let url = format!("mysql://{user}:{pass}@{host}:{port}/{db}");
    MySqlPoolOptions::new()
        .max_connections(32)
        .min_connections(4)
        .connect(&url)
        .await
        .context("connect MySQL")
}

pub async fn list_regions(date: &str, pool: &MySqlPool) -> Result<Vec<u8>> {
    let q = "SHOW TABLES";
    let rows = sqlx::query(q).fetch_all(pool).await?;
    let prefix = format!("discover_interference_reborn_{date}_reg");
    let mut regs: Vec<u8> = Vec::new();
    for r in rows {
        let bytes: Vec<u8> = r.try_get(0)?;
        let s = String::from_utf8_lossy(&bytes);
        if let Some(rest) = s.strip_prefix(&prefix)
            && let Ok(n) = rest.parse::<u8>()
            && !regs.contains(&n)
        {
            regs.push(n);
        }
    }
    regs.sort_unstable();
    Ok(regs)
}

pub async fn fetch_table_raw(
    table: &str,
    pool: &MySqlPool,
) -> Result<Vec<(String, parse::ConfigMap, parse::NeighborMap)>> {
    use futures_util::TryStreamExt;

    let q = format!("SELECT CPE_Serial_Number, json_config, json_neighbor FROM {table}");
    let mut rows = sqlx::query(&q).fetch(pool);

    let mut out = Vec::new();
    loop {
        let r = match rows.try_next().await {
            Ok(Some(r)) => r,
            Ok(None) => break,
            Err(e) => {
                eprintln!("[fetch {table}] row err: {e}");
                continue;
            }
        };
        let sn: String = r.try_get("CPE_Serial_Number").unwrap_or_default();
        if sn.is_empty() {
            continue;
        }
        let cfg_str: Option<String> = r.try_get("json_config").ok().flatten();
        let nei_str: Option<String> = r.try_get("json_neighbor").ok().flatten();

        let cfg_map = match parse::parse_config(cfg_str.as_deref().unwrap_or("{}")) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let nei_map = match parse::parse_neighbor(nei_str.as_deref().unwrap_or("{}")) {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push((sn, cfg_map, nei_map));
    }
    Ok(out)
}

pub fn split_into_band(
    raw: Vec<(String, parse::ConfigMap, parse::NeighborMap)>,
    invalid_bssid: &std::collections::HashSet<String>,
) -> (Vec<MemRow>, Vec<MemRow>) {
    let mut data_2g: Vec<MemRow> = Vec::with_capacity(raw.len());
    let mut data_5g: Vec<MemRow> = Vec::with_capacity(raw.len());

    for (sn, cfg_map, nei_map) in raw {
        let (own_2g, own_5g) = parse::split_own_by_slot(&cfg_map);
        let (nb_2g, nb_5g) = parse::split_neighbors_by_band(&nei_map, invalid_bssid);

        let (own_2g_ch, own_2g_pc) = if let Some(o) = own_2g.first() {
            (
                o.channel.map(|c| c as i32),
                o.possible_channel
                    .iter()
                    .map(|&c| c as i32)
                    .collect::<Vec<i32>>(),
            )
        } else {
            (None, vec![])
        };
        let (own_5g_ch, own_5g_pc) = if let Some(o) = own_5g.first() {
            (
                o.channel.map(|c| c as i32),
                o.possible_channel
                    .iter()
                    .map(|&c| c as i32)
                    .collect::<Vec<i32>>(),
            )
        } else {
            (None, vec![])
        };
        let own_2g_str: Vec<String> = own_2g.into_iter().map(|x| x.bssid).collect();
        let own_5g_str: Vec<String> = own_5g.into_iter().map(|x| x.bssid).collect();
        let nb_2g_str: Vec<(String, i32, i32)> = nb_2g
            .into_iter()
            .map(|x| (x.bssid, x.channel as i32, x.rssi))
            .collect();
        let nb_5g_str: Vec<(String, i32, i32)> = nb_5g
            .into_iter()
            .map(|x| (x.bssid, x.channel as i32, x.rssi))
            .collect();

        data_2g.push(MemRow {
            sn: sn.clone(),
            own: own_2g_str,
            neighbors: nb_2g_str,
            own_channel: own_2g_ch,
            possible_channels: own_2g_pc,
        });
        data_5g.push(MemRow {
            sn,
            own: own_5g_str,
            neighbors: nb_5g_str,
            own_channel: own_5g_ch,
            possible_channels: own_5g_pc,
        });
    }
    (data_2g, data_5g)
}

pub async fn fetch_and_parse(
    table: &str,
    pool: &MySqlPool,
    invalid_bssid: &std::collections::HashSet<String>,
) -> Result<(Vec<MemRow>, Vec<MemRow>)> {
    use std::time::Instant;
    let t = Instant::now();
    let raw = fetch_table_raw(table, pool).await?;
    let n = raw.len();
    let (d2, d5) = split_into_band(raw, invalid_bssid);
    eprintln!(
        "[parse {table}] {n} rows, {:.2}s",
        t.elapsed().as_secs_f64()
    );
    Ok((d2, d5))
}

pub async fn ensure_table(date: &str, pool: &MySqlPool) -> Result<String> {
    let table = format!("wifi_optimize_{date}");
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS `{table}` (
            reg varchar(32) NOT NULL,
            sn varchar(128) NOT NULL,
            clusterid_2g bigint,
            ch_before_2g bigint,
            cost_before_2g double,
            ch_after_2g bigint,
            cost_after_2g double,
            status_2g varchar(8),
            clusterid_5g bigint,
            ch_before_5g bigint,
            cost_before_5g double,
            ch_after_5g bigint,
            cost_after_5g double,
            status_5g varchar(8),
            updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (reg, sn),
            KEY idx_clusterid_2g (clusterid_2g),
            KEY idx_clusterid_5g (clusterid_5g),
            KEY idx_status_2g (status_2g),
            KEY idx_status_5g (status_5g)
        ) ENGINE=MyISAM DEFAULT CHARSET=utf8mb4"
    );
    sqlx::query(&sql).execute(pool).await?;
    Ok(table)
}

pub struct JoinedOptimizeRow {
    pub reg: String,
    pub sn: String,
    pub clusterid_2g: Option<i64>,
    pub ch_before_2g: Option<i32>,
    pub cost_before_2g: f64,
    pub ch_after_2g: i32,
    pub cost_after_2g: f64,
    pub status_2g: String,
    pub clusterid_5g: Option<i64>,
    pub ch_before_5g: Option<i32>,
    pub cost_before_5g: f64,
    pub ch_after_5g: i32,
    pub cost_after_5g: f64,
    pub status_5g: String,
}

pub fn join_bands(rows_2g: Vec<OptimizeRow>, rows_5g: Vec<OptimizeRow>) -> Vec<JoinedOptimizeRow> {
    let mut map_5g: HashMap<String, OptimizeRow> = HashMap::with_capacity(rows_5g.len());
    for r in rows_5g {
        map_5g.insert(r.sn.clone(), r);
    }

    rows_2g
        .into_iter()
        .map(|r2g| {
            let r5g = map_5g.remove(&r2g.sn);
            JoinedOptimizeRow {
                reg: r2g.reg,
                sn: r2g.sn,
                clusterid_2g: r2g.clusterid,
                ch_before_2g: r2g.ch_before,
                cost_before_2g: r2g.cost_before,
                ch_after_2g: r2g.ch_after,
                cost_after_2g: r2g.cost_after,
                status_2g: r2g.status,
                clusterid_5g: r5g.as_ref().and_then(|r| r.clusterid),
                ch_before_5g: r5g.as_ref().and_then(|r| r.ch_before),
                cost_before_5g: r5g.as_ref().map(|r| r.cost_before).unwrap_or(0.0),
                ch_after_5g: r5g.as_ref().map(|r| r.ch_after).unwrap_or(0),
                cost_after_5g: r5g.as_ref().map(|r| r.cost_after).unwrap_or(0.0),
                status_5g: r5g
                    .as_ref()
                    .map(|r| r.status.clone())
                    .unwrap_or_else(|| "STAY".to_string()),
            }
        })
        .collect()
}

pub async fn truncate_table(table: &str, pool: &MySqlPool) -> Result<()> {
    let q = format!("TRUNCATE TABLE `{table}`");
    sqlx::query(&q).execute(pool).await?;
    eprintln!("[upload] truncated {table}");
    Ok(())
}

fn escape_sql(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

pub async fn upload_optimize(
    rows: &[JoinedOptimizeRow],
    table: &str,
    pool: &MySqlPool,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let batch_size = 3000_usize;
    let mut count = 0usize;

    for chunk in rows.chunks(batch_size) {
        let mut sql = String::with_capacity(chunk.len() * 120 + 512);
        sql.push_str("INSERT INTO `");
        sql.push_str(table);
        sql.push_str(
            "` (reg, sn, clusterid_2g, ch_before_2g, cost_before_2g, \
                     ch_after_2g, cost_after_2g, status_2g, clusterid_5g, ch_before_5g, \
                     cost_before_5g, ch_after_5g, cost_after_5g, status_5g) VALUES ",
        );

        for (i, r) in chunk.iter().enumerate() {
            if i > 0 {
                sql.push(',');
            }
            write!(
                sql,
                "('{}','{}',{},{},{:.4},{},{:.4},'{}',{},{},{:.4},{},{:.4},'{}')",
                escape_sql(&r.reg),
                escape_sql(&r.sn),
                r.clusterid_2g
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "NULL".to_string()),
                r.ch_before_2g
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "NULL".to_string()),
                r.cost_before_2g,
                r.ch_after_2g,
                r.cost_after_2g,
                escape_sql(&r.status_2g),
                r.clusterid_5g
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "NULL".to_string()),
                r.ch_before_5g
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "NULL".to_string()),
                r.cost_before_5g,
                r.ch_after_5g,
                r.cost_after_5g,
                escape_sql(&r.status_5g),
            )
            .unwrap();
        }

        sql.push_str(
            " ON DUPLICATE KEY UPDATE \
            clusterid_2g=VALUES(clusterid_2g), \
            ch_before_2g=VALUES(ch_before_2g), \
            cost_before_2g=VALUES(cost_before_2g), \
            ch_after_2g=VALUES(ch_after_2g), \
            cost_after_2g=VALUES(cost_after_2g), \
            status_2g=VALUES(status_2g), \
            clusterid_5g=VALUES(clusterid_5g), \
            ch_before_5g=VALUES(ch_before_5g), \
            cost_before_5g=VALUES(cost_before_5g), \
            ch_after_5g=VALUES(ch_after_5g), \
            cost_after_5g=VALUES(cost_after_5g), \
            status_5g=VALUES(status_5g), \
            updated_at=CURRENT_TIMESTAMP",
        );

        sqlx::query(&sql)
            .execute(pool)
            .await
            .context("batch insert execute")?;
        count += chunk.len();
    }

    eprintln!("[upload] committed {count} rows to {table}");
    Ok(())
}
