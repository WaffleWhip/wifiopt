use anyhow::{Context, Result};
use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Config;

#[derive(Debug, Default)]
struct Dsu {
    p: HashMap<String, String>,
}

impl Dsu {
    fn find(&mut self, x: &str) -> String {
        if !self.p.contains_key(x) {
            self.p.insert(x.to_string(), x.to_string());
            return x.to_string();
        }
        let mut cur = x.to_string();
        while self.p[&cur] != cur {
            let gp = self.p[&self.p[&cur].clone()].clone();
            self.p.insert(cur.clone(), gp.clone());
            cur = gp;
        }
        cur
    }
    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.p.insert(ra, rb);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClusterResult {
    pub valid: Vec<(Vec<String>, Vec<String>)>,
    pub n_edges: usize,
    pub total_rows: usize,
    pub cluster_min: usize,
    pub neighbor_freq: HashMap<String, usize>,
}

fn collect_own_set(parse_path: &Path, band: &str, cfg: &Config) -> Result<(HashSet<String>, HashMap<String, usize>, usize)> {
    let file = std::fs::File::open(parse_path)
        .with_context(|| format!("open {}", parse_path.display()))?;
    let iter = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)?
        .build()?;
    let invalid = cfg.invalid_bssid_set();
    let own_col = format!("own_{band}");

    let mut set: HashSet<String> = HashSet::new();
    let mut freq: HashMap<String, usize> = HashMap::new();
    let mut total_rows: usize = 0;
    for batch_result in iter {
        let batch = batch_result?;
        total_rows += batch.num_rows();
        let own_arr = batch
            .column(batch.schema().index_of(&own_col)?)
            .as_any()
            .downcast_ref::<ListArray>()
            .context("own not list")?;
        let own_values = own_arr
            .values()
            .as_any()
            .downcast_ref::<StructArray>()
            .context("own values not struct")?;
        let own_bssid = own_values
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("own bssid not utf8")?;
        let offsets = own_arr.value_offsets();
        for i in 0..batch.num_rows() {
            let start = offsets[i] as usize;
            let end = offsets[i + 1] as usize;
            let mut row_seen: HashSet<String> = HashSet::new();
            for j in start..end {
                if !own_bssid.is_valid(j) {
                    continue;
                }
                let b = own_bssid.value(j).trim().to_lowercase();
                if b.is_empty() || invalid.contains(&b) {
                    continue;
                }
                if row_seen.insert(b.clone()) {
                    *freq.entry(b.clone()).or_insert(0) += 1;
                }
                set.insert(b);
            }
        }
    }
    Ok((set, freq, total_rows))
}

fn run_dsu(parse_path: &Path, band: &str, cfg: &Config) -> Result<ClusterResult> {
    let (own_set, own_freq, total_rows) = collect_own_set(parse_path, band, cfg)?;

    let mut freq_sorted: Vec<(String, usize)> = own_freq.iter().map(|(k, v)| (k.clone(), *v)).collect();
    freq_sorted.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("[anomaly] top own bssids by row-count:");
    for (b, c) in freq_sorted.iter().take(10) {
        eprintln!("  {} rows={} pct={:.2}%", b, c, 100.0 * *c as f64 / total_rows as f64);
    }

    let total_own_b = own_freq.len();
    let mut buckets = [0usize; 6];
    for (_, c) in &freq_sorted {
        let idx = match *c {
            1 => 0,
            2..=10 => 1,
            11..=100 => 2,
            101..=1000 => 3,
            1001..=10000 => 4,
            _ => 5,
        };
        buckets[idx] += 1;
    }
    eprintln!("[anomaly] own bssid freq distribution (total={}):", total_own_b);
    eprintln!("  count=1: {}", buckets[0]);
    eprintln!("  count=2..=10: {}", buckets[1]);
    eprintln!("  count=11..=100: {}", buckets[2]);
    eprintln!("  count=101..=1000: {}", buckets[3]);
    eprintln!("  count=1001..=10000: {}", buckets[4]);
    eprintln!("  count>10000: {}", buckets[5]);

    let file = std::fs::File::open(parse_path)
        .with_context(|| format!("open {}", parse_path.display()))?;
    let iter = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)?
        .build()?;

    let invalid = cfg.invalid_bssid_set();
    let own_col = format!("own_{band}");
    let nb_col = format!("neighbors_{band}");

    let mut dsu = Dsu::default();
    let mut n_edges: usize = 0;
    let mut neighbor_freq: HashMap<String, usize> = HashMap::new();

    for batch_result in iter {
        let batch = batch_result?;
        let sn_arr = batch
            .column(batch.schema().index_of("sn")?)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("sn not utf8")?;
        let own_arr = batch
            .column(batch.schema().index_of(&own_col)?)
            .as_any()
            .downcast_ref::<ListArray>()
            .context("own not list")?;
        let nb_arr = batch
            .column(batch.schema().index_of(&nb_col)?)
            .as_any()
            .downcast_ref::<ListArray>()
            .context("nb not list")?;

        let own_values = own_arr.values().as_any().downcast_ref::<StructArray>().context("own values not struct")?;
        let own_bssid = own_values
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("own bssid not utf8")?;
        let own_offsets = own_arr.value_offsets();

        let nb_values = nb_arr.values().as_any().downcast_ref::<StructArray>().context("nb values not struct")?;
        let nb_bssid = nb_values
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("nb bssid not utf8")?;
        let nb_ch = nb_values
            .column(1)
            .as_any()
            .downcast_ref::<Int32Array>()
            .context("nb channel not i32")?;
        let nb_offsets = nb_arr.value_offsets();

        for i in 0..batch.num_rows() {
            let Some(sn) = (if sn_arr.is_valid(i) { Some(sn_arr.value(i).to_string()) } else { None }) else {
                continue;
            };
            let sk = format!("s:{sn}");
            let mut seen: HashSet<String> = HashSet::new();

            let o_start = own_offsets[i] as usize;
            let o_end = own_offsets[i + 1] as usize;
            for j in o_start..o_end {
                if !own_bssid.is_valid(j) {
                    continue;
                }
                let b = own_bssid.value(j).trim().to_lowercase();
                if b.is_empty() || invalid.contains(&b) {
                    continue;
                }
                if !seen.insert(b.clone()) {
                    continue;
                }
                dsu.union(&sk, &format!("b:{b}"));
                n_edges += 1;
            }

            let n_start = nb_offsets[i] as usize;
            let n_end = nb_offsets[i + 1] as usize;
            for j in n_start..n_end {
                if !nb_bssid.is_valid(j) {
                    continue;
                }
                let b = nb_bssid.value(j).trim().to_lowercase();
                if b.is_empty() || invalid.contains(&b) {
                    continue;
                }
                if !own_set.contains(&b) {
                    continue;
                }
                *neighbor_freq.entry(b.clone()).or_insert(0) += 1;
                if !nb_ch.is_valid(j) {
                    continue;
                }
                let ch = nb_ch.value(j);
                if !cfg.band_ok(ch, band) {
                    continue;
                }
                if !seen.insert(b.clone()) {
                    continue;
                }
                dsu.union(&sk, &format!("b:{b}"));
                n_edges += 1;
            }
        }
    }

    let mut comp_sns: HashMap<String, Vec<String>> = HashMap::new();
    let mut comp_bssids: HashMap<String, Vec<String>> = HashMap::new();
    let keys: Vec<String> = dsu.p.keys().cloned().collect();
    for node in keys {
        let root = dsu.find(&node);
        if node.starts_with('s') {
            comp_sns.entry(root).or_default().push(node[2..].to_string());
        } else if node.starts_with('b') {
            comp_bssids
                .entry(root)
                .or_default()
                .push(node[2..].to_string());
        }
    }

    let cluster_min = cfg.cluster.min_size.max(1);
    let mut valid: Vec<(Vec<String>, Vec<String>)> = comp_sns
        .into_iter()
        .filter(|(_, sns)| sns.len() >= cluster_min)
        .map(|(r, sns)| (sns, comp_bssids.remove(&r).unwrap_or_default()))
        .collect();
    valid.sort_by(|a, b| a.0.len().cmp(&b.0.len()));

    let nb_total: usize = neighbor_freq.values().sum();
    let nb_unique = neighbor_freq.len();
    eprintln!("[anomaly] neighbor_freq total_count={} unique_bssids={}", nb_total, nb_unique);
    let mut nb_sorted: Vec<(String, usize)> = neighbor_freq.iter().map(|(k, v)| (k.clone(), *v)).collect();
    nb_sorted.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("[anomaly] top neighbor bssids by row-count (in own_set) nb_sorted_len={}:", nb_sorted.len());
    for (i, item) in nb_sorted.iter().take(10).enumerate() {
        eprintln!("  #{} {} rows={} pct={:.4}%", i, item.0, item.1, 100.0 * item.1 as f64 / total_rows as f64);
    }

    Ok(ClusterResult {
        valid,
        n_edges,
        total_rows,
        cluster_min,
        neighbor_freq,
    })
}

pub struct MemRow {
    pub sn: String,
    pub own: Vec<String>,
    pub neighbors: Vec<(String, i32, i32)>,
    pub own_channel: Option<i32>,
    pub possible_channels: Vec<i32>,
}

pub fn cluster_dist_in_mem(rows: &[MemRow], band: &str, cfg: &Config) -> Result<ClusterResult> {
    let invalid = cfg.invalid_bssid_set();
    let mut own_set: HashSet<String> = HashSet::new();
    let mut own_freq: HashMap<String, usize> = HashMap::new();
    for r in rows {
        let mut seen: HashSet<String> = HashSet::new();
        for b in &r.own {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) {
                continue;
            }
            if seen.insert(b.clone()) {
                own_set.insert(b.clone());
                *own_freq.entry(b).or_insert(0) += 1;
            }
        }
    }
    let total_rows = rows.len();
    let cluster_min = cfg.cluster.min_size.max(1);

    let mut dsu = Dsu::default();
    let mut n_edges: usize = 0;
    let mut neighbor_freq: HashMap<String, usize> = HashMap::new();

    for r in rows {
        let sk = format!("s:{}", r.sn);
        let mut seen: HashSet<String> = HashSet::new();

        for b in &r.own {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) {
                continue;
            }
            if !seen.insert(b.clone()) {
                continue;
            }
            dsu.union(&sk, &format!("b:{b}"));
            n_edges += 1;
        }

        for (b, ch, _rssi) in &r.neighbors {
            let b = b.trim().to_lowercase();
            if b.is_empty() || invalid.contains(&b) {
                continue;
            }
            if !own_set.contains(&b) {
                continue;
            }
            *neighbor_freq.entry(b.clone()).or_insert(0) += 1;
            if !cfg.band_ok(*ch, band) {
                continue;
            }
            if !seen.insert(b.clone()) {
                continue;
            }
            dsu.union(&sk, &format!("b:{b}"));
            n_edges += 1;
        }
    }

    let mut comp_sns: HashMap<String, Vec<String>> = HashMap::new();
    let mut comp_bssids: HashMap<String, Vec<String>> = HashMap::new();
    let keys: Vec<String> = dsu.p.keys().cloned().collect();
    for node in keys {
        let root = dsu.find(&node);
        if node.starts_with('s') {
            comp_sns.entry(root).or_default().push(node[2..].to_string());
        } else if node.starts_with('b') {
            comp_bssids
                .entry(root)
                .or_default()
                .push(node[2..].to_string());
        }
    }

    let mut valid: Vec<(Vec<String>, Vec<String>)> = comp_sns
        .into_iter()
        .filter(|(_, sns)| sns.len() >= cluster_min)
        .map(|(r, sns)| (sns, comp_bssids.remove(&r).unwrap_or_default()))
        .collect();
    valid.sort_by(|a, b| a.0.len().cmp(&b.0.len()));

    Ok(ClusterResult {
        valid,
        n_edges,
        total_rows,
        cluster_min,
        neighbor_freq,
    })
}

fn write_parquet(out_path: &Path, res: &ClusterResult) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("cluster_id", DataType::UInt32, false),
        Field::new("member_count", DataType::UInt32, false),
        Field::new("member_sns", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), true),
        Field::new("bssids", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), true),
    ]));

    let n = res.valid.len();
    let cluster_ids: UInt32Array = (1..=n as u32).collect();
    let member_counts: UInt32Array = res.valid.iter().map(|(sns, _)| sns.len() as u32).collect();

    let sns_flat: StringArray = res
        .valid
        .iter()
        .flat_map(|(s, _)| s.iter().map(|x| Some(x.as_str())))
        .collect();
    let sns_offsets: Vec<i32> = {
        let mut o = Vec::with_capacity(n + 1);
        o.push(0);
        let mut cur = 0i32;
        for (s, _) in &res.valid {
            cur += s.len() as i32;
            o.push(cur);
        }
        o
    };
    let sns_list = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Utf8, true)),
        arrow::buffer::OffsetBuffer::new(arrow::buffer::ScalarBuffer::from(sns_offsets)),
        Arc::new(sns_flat),
        None,
    )?;

    let bs_flat: StringArray = res
        .valid
        .iter()
        .flat_map(|(_, b)| b.iter().map(|x| Some(x.as_str())))
        .collect();
    let bs_offsets: Vec<i32> = {
        let mut o = Vec::with_capacity(n + 1);
        o.push(0);
        let mut cur = 0i32;
        for (_, b) in &res.valid {
            cur += b.len() as i32;
            o.push(cur);
        }
        o
    };
    let bs_list = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Utf8, true)),
        arrow::buffer::OffsetBuffer::new(arrow::buffer::ScalarBuffer::from(bs_offsets)),
        Arc::new(bs_flat),
        None,
    )?;

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(cluster_ids),
            Arc::new(member_counts),
            Arc::new(sns_list),
            Arc::new(bs_list),
        ],
    )?;

    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();
    let out_file = std::fs::File::create(out_path)
        .with_context(|| format!("create {}", out_path.display()))?;
    let mut writer = ArrowWriter::try_new(out_file, batch.schema(), Some(props))?;
    writer.write(&batch)?;
    writer.finish()?;
    Ok(())
}

pub fn cluster_band(parse_path: &Path, band: &str, cfg: &Config) -> Result<Vec<PathBuf>> {
    let res = run_dsu(parse_path, band, cfg)?;

    let parent = parse_path
        .parent()
        .and_then(|p| p.parent())
        .context("parse_path missing date parent")?
        .to_path_buf();
    let date = parent.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
    let reg = parse_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("reg");
    let out_dir: PathBuf = ["out", date, "cluster"].iter().collect();
    std::fs::create_dir_all(&out_dir).with_context(|| format!("mkdir {}", out_dir.display()))?;
    let out_path: PathBuf = out_dir.join(format!("{reg}_{band}.parquet"));

    write_parquet(&out_path, &res)?;

    eprintln!(
        "[cluster {band}] rows={} edges={} components>=min({})={} -> {}",
        res.total_rows,
        res.n_edges,
        res.cluster_min,
        res.valid.len(),
        out_path.display()
    );

    Ok(vec![out_path])
}

pub fn cluster_dist(parse_path: &Path, band: &str, cfg: &Config) -> Result<()> {
    let res = run_dsu(parse_path, band, cfg)?;

    let mut dist: HashMap<usize, usize> = HashMap::new();
    for entry in &res.valid {
        *dist.entry(entry.0.len()).or_insert(0) += 1;
    }
    let mut keys: Vec<usize> = dist.keys().copied().collect();
    keys.sort();

    eprintln!(
        "[cluster {band}] rows={} edges={} components>=min({})={}",
        res.total_rows,
        res.n_edges,
        res.cluster_min,
        res.valid.len()
    );
    println!("size\tcount");
    for k in keys {
        println!("{}\t{}", k, dist[&k]);
    }

    let threshold = 1000;
    for (idx, (sns, bs)) in res.valid.iter().enumerate().take(5) {
        if sns.len() >= threshold {
            eprintln!(
                "[anomaly] rank={} size={} bssids_count={} bssids_sample={:?} sns_sample={:?}",
                idx + 1,
                sns.len(),
                bs.len(),
                &bs[..bs.len().min(5)],
                &sns[..sns.len().min(3)]
            );
        }
    }
    Ok(())
}