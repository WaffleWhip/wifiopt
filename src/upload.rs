//! MySQL upload path, byte-compatible with production (`~/wifiopt/src/mysql.rs`).
//!
//! - `ensure_table`: same 15-column CREATE TABLE (no node_* columns;
//!   those are added + filled by the team's downstream process, same as prod).
//! - batches of 3000 multi-row INSERTs with ON DUPLICATE KEY UPDATE.
//! - costs formatted `{:.4}`, NULLs for missing radios, `updated_at=CURRENT_TIMESTAMP`.
//!
//! Dormant unless `--upload` is passed. The optimizer stays fetch-only by default.

use anyhow::{Context, Result};
use mysql::prelude::Queryable;
use std::fmt::Write as _;

use crate::output::OutputRow;

pub const BATCH: usize = 3000;

pub fn table_for(date: &str) -> Result<String> {
    if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
        anyhow::bail!("bad date: {date}");
    }
    Ok(format!("wifi_optimize_{date}"))
}

/// Explicit table override (scratch/backfill). Strict charset, no dates needed.
pub fn table_name(name: &str) -> Result<String> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        anyhow::bail!("bad table: {name}");
    }
    Ok(name.to_string())
}

pub fn ensure_sql(table: &str) -> String {
    format!(
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
    )
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn opt_int(v: Option<impl std::fmt::Display>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "NULL".into())
}

fn opt_cost(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.4}")).unwrap_or_else(|| "NULL".into())
}

fn opt_str(v: &Option<String>) -> String {
    v.as_deref()
        .map(|s| format!("'{}'", esc(s)))
        .unwrap_or_else(|| "NULL".into())
}

/// Pure builder: one INSERT statement for a batch of rows. Tested, no DB needed.
pub fn insert_sql(table: &str, rows: &[OutputRow]) -> String {
    let mut sql = String::new();
    sql.push_str("INSERT INTO `");
    sql.push_str(table);
    sql.push_str(
        "` (reg, sn, clusterid_2g, ch_before_2g, cost_before_2g, \
         ch_after_2g, cost_after_2g, status_2g, clusterid_5g, ch_before_5g, \
         cost_before_5g, ch_after_5g, cost_after_5g, status_5g) VALUES ",
    );
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            sql.push(',');
        }
        write!(
            sql,
            "('{}','{}',{},{},{},{},{},{},{},{},{},{},{},{})",
            esc(&r.reg),
            esc(&r.sn),
            opt_int(r.clusterid_2g),
            opt_int(r.ch_before_2g),
            opt_cost(r.cost_before_2g),
            opt_int(r.ch_after_2g),
            opt_cost(r.cost_after_2g),
            opt_str(&r.status_2g),
            opt_int(r.clusterid_5g),
            opt_int(r.ch_before_5g),
            opt_cost(r.cost_before_5g),
            opt_int(r.ch_after_5g),
            opt_cost(r.cost_after_5g),
            opt_str(&r.status_5g),
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
    sql
}

pub fn pool(db_url: &str) -> Result<mysql::Pool> {
    mysql::Pool::new(db_url).context("db pool")
}

pub fn ensure_table(db_url: &str, date: &str, table_override: &str) -> Result<String> {
    let table = if table_override.is_empty() {
        table_for(date)?
    } else {
        table_name(table_override)?
    };
    let p = pool(db_url)?;
    let mut conn = p.get_conn().context("db conn")?;
    conn.query_drop(ensure_sql(&table))?;
    Ok(table)
}

pub fn truncate_table(db_url: &str, table: &str) -> Result<()> {
    let p = pool(db_url)?;
    let mut conn = p.get_conn().context("db conn")?;
    conn.query_drop(format!("TRUNCATE TABLE `{table}`"))?;
    Ok(())
}

/// Upload one region's rows (own connection; safe to call from a thread).
/// Returns rows uploaded.
pub fn upload_rows(db_url: &str, table: &str, rows: &[OutputRow]) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let p = pool(db_url)?;
    let mut conn = p.get_conn().context("db conn")?;
    let mut n = 0;
    for chunk in rows.chunks(BATCH) {
        conn.query_drop(insert_sql(table, chunk))
            .context("batch insert")?;
        n += chunk.len();
    }
    Ok(n)
}
