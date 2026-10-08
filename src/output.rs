use anyhow::Result;
use serde::Serialize;

pub const HEADER: &str = "reg,sn,clusterid_2g,ch_before_2g,cost_before_2g,ch_after_2g,cost_after_2g,status_2g,clusterid_5g,ch_before_5g,cost_before_5g,ch_after_5g,cost_after_5g,status_5g,updated_at,node_id,node_ip,slot,port,onu_id,regional,witel,cpe_product_class";

pub fn header() -> &'static str {
    HEADER
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputRow {
    pub reg: String,
    pub sn: String,
    pub clusterid_2g: Option<i64>,
    pub ch_before_2g: Option<u8>,
    pub cost_before_2g: Option<f64>,
    pub ch_after_2g: Option<u8>,
    pub cost_after_2g: Option<f64>,
    pub status_2g: Option<String>,
    pub clusterid_5g: Option<i64>,
    pub ch_before_5g: Option<u8>,
    pub cost_before_5g: Option<f64>,
    pub ch_after_5g: Option<u8>,
    pub cost_after_5g: Option<f64>,
    pub status_5g: Option<String>,
    pub updated_at: String,
    pub node_id: Option<String>,
    pub node_ip: Option<String>,
    pub slot: Option<u8>,
    pub port: Option<u8>,
    pub onu_id: Option<u32>,
    pub regional: Option<u8>,
    pub witel: Option<String>,
    pub cpe_product_class: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub date: String,
    pub regs: Vec<String>,
    pub rows: usize,
}

pub fn write_csv(path: &str, rows: &[OutputRow]) -> Result<()> {
    let tmp = format!("{path}.tmp");
    let mut w = csv::Writer::from_path(&tmp)?;
    for r in rows {
        w.serialize(r)?;
    }
    w.flush()?;
    drop(w);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_summary(path: &str, stats: &Summary) -> Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(stats)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
