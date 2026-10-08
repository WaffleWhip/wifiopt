# WiFi Optimizer Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a fetch-only Rust optimizer that reads `discover_*` and writes file output with the exact `wifi_optimize` 23-column shape.

**Architecture:** Sync Rust binary with rayon per-cluster parallelism. Read-only MySQL `SELECT`, parse JSON configs and neighbors, build interference graph, greedy minimal-cost channel assignment, atomic CSV output.

**Tech Stack:** Rust 1.99 (server authoritative), clap 4, serde and serde_json, csv, rayon, anyhow, chrono, mysql crate. Latest stable dependencies at `cargo init` time.

**Spec:** `docs/superpowers/specs/2026-10-08-wifiopt-optimizer-design.md`

## Global Constraints

- MySQL is fetch-only. The binary issues `SELECT` only, never `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `DROP`, or `ALTER`.
- Output shape matches `wifi_optimize_*` exactly: 23 columns in spec order.
- CLI supports `--date` default today, `--reg all` or subset such as `reg1,reg2`, `--out` directory.
- Cost defaults: 2.4 GHz co-channel 1.0 and adjacent 0.5 or 0.25, 5 GHz co-channel only, RSSI threshold `-85` dBm, `STAY` delta `0.05`. All tunable via CLI.
- Empty `possible_channel` falls back to 2.4 GHz `1-13` and 5 GHz `36,40,44,48,52,56,60,64,149,153,157,161`.
- Cluster IDs are deterministic, sorted by minimum serial in each component.
- Development is local, builds and full runs happen on `192.168.1.69` as root over ssh. No optimizer runs write to the database from anywhere.
- File output is one CSV per region plus `summary.json`, written via temp file plus rename.

## Review Focus

- Neighbor rows with malformed `signal_strength` or non-numeric `channel` are ignored rather than crashing the run.
- `json_config` missing `possible_channel` falls back to the default channel sets above.
- `discover` table missing for one region and date fails only that region.
- Mixed 2.4 GHz only, 5 GHz only, and dual-band rows produce nulls on the absent band, never zeros.
- Reruns over the same date and regions produce byte-identical CSV row sets apart from `updated_at` and `summary.json` timestamps.

---

### Task 1: Scaffold binary plus CLI config

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/config.rs`
- Test: `tests/cli_test.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `config::Options { date: String, regs: Vec<String>, out: String, db_url: String, rssi_threshold: f32, stay_delta: f64 }`, `config::parse_args() -> Options`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_reg_subset_and_defaults_date() {
    let opts = config::parse_from(["wifiopt", "--reg", "reg1,reg2", "--out", "./out"]);
    assert_eq!(opts.regs, vec!["reg1".to_string(), "reg2".to_string()]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test cli_test parses_reg_subset_and_defaults_date`
Expected: FAIL with `config` module not found.

- [ ] **Step 3: Implement `config::parse_from` and `parse_args` in `src/config.rs` with clap 4**

Flags: `--date` default today `YYYYMMDD`, `--reg` default `all` expanding to `reg1..reg7`, `--out` required, `--db-url` env default, `--rssi-threshold` default `-85.0`, `--stay-delta` default `0.05`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test cli_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/main.rs src/config.rs tests/cli_test.rs
git commit -m "feat: add optimizer CLI scaffold"
```

### Task 2: Read-only database layer

**Files:**
- Create: `src/db.rs`
- Test: `tests/db_audit_test.rs`
- Test: `tests/db_fetch_test.rs`

**Interfaces:**
- Consumes: `config::Options`.
- Produces: `db::load_discover(date: &str, reg: &str) -> Vec<DiscoverRow>`, `db::load_bssid_index() -> HashMap<String, String>`, `db::DiscoverRow` struct.

- [ ] **Step 1: Write the failing audit test**

```rust
#[test]
fn only_select_statements_allowed() {
    for sql in db::all_statements() {
        assert!(sql.trim_start().to_uppercase().starts_with("SELECT"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test db_audit_test`
Expected: FAIL with `db` module not found.

- [ ] **Step 3: Implement `src/db.rs` with the `mysql` crate and pooled `SELECT` queries only**

Queries: discover rows by date and region table name, BSSID index from `list_sn_bssid` plus `list_sn_bssid_new`. Retry 3 times with backoff on disconnect. Expose `all_statements()` returning every SQL string the module can issue.

- [ ] **Step 4: Run audit test to verify it passes**

Run: `cargo test --test db_audit_test`
Expected: PASS.

- [ ] **Step 5: Run live fetch test on the server**

Run: `ssh root@192.168.1.69 'cd /root/wifiopt && cargo test --test db_fetch_test -- --nocapture'`
Expected: PASS, fetch test reads a small region sample with no writes.

- [ ] **Step 6: Commit**

```bash
git add src/db.rs tests/db_audit_test.rs tests/db_fetch_test.rs
git commit -m "feat: add read-only discover fetch"
```

### Task 3: Discover parsing

**Files:**
- Create: `src/discover.rs`
- Test: `tests/discover_test.rs`
- Create: `tests/fixtures/neighbor_sample.json`

**Interfaces:**
- Consumes: `db::DiscoverRow`.
- Produces: `discover::ApInfo { serial: String, band2g: Option<RadioInfo>, band5g: Option<RadioInfo>, neighbors: Vec<Neighbor> }`, `discover::parse(row: &DiscoverRow) -> ApInfo`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_config_and_neighbors() {
    let row = fixture_row();
    let ap = discover::parse(&row).unwrap();
    assert_eq!(ap.band2g.unwrap().current, 2);
    assert_eq!(ap.neighbors.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test discover_test`
Expected: FAIL with `discover` module not found.

- [ ] **Step 3: Implement `discover::parse` in `src/discover.rs`**

Parse `json_config` for current channel and `possible_channel` per band, parse `json_neighbor` for channel and `signal_strength`, skip malformed neighbor entries, apply default channel sets when `possible_channel` is empty.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test discover_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/discover.rs tests/discover_test.rs tests/fixtures/neighbor_sample.json
git commit -m "feat: parse discover config and neighbors"
```

### Task 4: Interference graph plus clustering

**Files:**
- Create: `src/graph.rs`
- Test: `tests/graph_test.rs`

**Interfaces:**
- Consumes: `Vec<discover::ApInfo>`, BSSID index, RSSI threshold.
- Produces: `graph::clusters(aps: &[ApInfo], bssid: &HashMap<String, String>, threshold: f32) -> Vec<Vec<usize>>`, separate 2.4 GHz and 5 GHz cluster lists.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn connected_aps_share_cluster() {
    let ids = graph::clusters(&three_ap_chain(), &bssid_index(), -85.0);
    assert_eq!(ids.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test graph_test`
Expected: FAIL with `graph` module not found.

- [ ] **Step 3: Implement connected components in `src/graph.rs`**

Edge when a neighbor BSSID maps to a known serial and signal is stronger than threshold. IDs are deterministic, sorted by minimum serial. 2.4 GHz and 5 GHz graphs are built independently.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test graph_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/graph.rs tests/graph_test.rs
git commit -m "feat: cluster interference graph"
```

### Task 5: Cost plus greedy assignment

**Files:**
- Create: `src/cost.rs`
- Test: `tests/cost_test.rs`

**Interfaces:**
- Consumes: `discover::ApInfo`, cluster members, channel candidates.
- Produces: `cost::score(ap: &ApInfo, channel: u8, band: Band) -> f64`, `cost::assign(cluster: &[ApInfo], band: Band, stay_delta: f64) -> Vec<Assignment>`, `cost::Assignment { serial: String, before: Option<u8>, after: Option<u8>, cost_before: Option<f64>, cost_after: Option<f64>, status: Option<String> }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn picks_minimum_cost_channel() {
    let a = cost::assign(&two_ap_cluster(), Band::G2, 0.05);
    assert!(a[0].cost_after.unwrap() <= a[0].cost_before.unwrap());
}

#[test]
fn absent_band_yields_nulls() {
    let a = cost::assign(&single_band_only_fixture(), Band::G2, 0.05);
    assert!(a[0].before_5g.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test cost_test`
Expected: FAIL with `cost` module not found.

- [ ] **Step 3: Implement RSSI-weighted scoring and greedy choice in `src/cost.rs`**

2.4 GHz weights are 1.0 co-channel, 0.5 at distance 1, 0.25 at distance 2. 5 GHz weights are 1.0 co-channel only. Empty neighbors score 0 and `STAY`. `STAY` when improvement is at most `stay_delta`, else `CHANGE`. Absent band yields `None` fields.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test cost_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cost.rs tests/cost_test.rs
git commit -m "feat: greedy channel assignment"
```

### Task 6: File output with exact shape

**Files:**
- Create: `src/output.rs`
- Test: `tests/output_test.rs`

**Interfaces:**
- Consumes: `Vec<cost::Assignment>` plus metadata map.
- Produces: `output::write_csv(path: &str, rows: &[OutputRow])`, `output::write_summary(path: &str, stats: &Summary)`, `output::OutputRow` with the 23 columns in spec order.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn csv_header_matches_wifi_optimize() {
    output::write_csv(tmp_path(), &[sample_row()]);
    assert_eq!(header(), "reg,sn,clusterid_2g,ch_before_2g,cost_before_2g,ch_after_2g,cost_after_2g,status_2g,clusterid_5g,ch_before_5g,cost_before_5g,ch_after_5g,cost_after_5g,status_5g,updated_at,node_id,node_ip,slot,port,onu_id,regional,witel,cpe_product_class");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test output_test`
Expected: FAIL with `output` module not found.

- [ ] **Step 3: Implement CSV plus summary writer in `src/output.rs`**

One CSV per region with the exact header above, temp file plus rename for atomicity, plus `summary.json` with row counts and per-region status.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test output_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/output.rs tests/output_test.rs
git commit -m "feat: write exact-shape CSV output"
```

### Task 7: Orchestration, logging, and server run

**Files:**
- Modify: `src/main.rs`
- Test: `tests/golden_test.rs`
- Create: `README.md`

**Interfaces:**
- Consumes: all task interfaces above.
- Produces: working `wifiopt` binary, per-region logs, exit codes, run documentation.

- [ ] **Step 1: Write the failing golden test**

```rust
#[test]
fn golden_fixture_matches_shape() {
    let rows = run_fixture();
    assert_eq!(rows[0].reg, "reg1");
    assert_eq!(output::header().split(',').count(), 23);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test golden_test`
Expected: FAIL with fixture runner missing.

- [ ] **Step 3: Implement `main` orchestration with rayon per-cluster work**

Resolve date and regions, load, parse, cluster, assign, and write outputs. Log per-region counts. Exit nonzero when any selected region fails while still completing the others.

- [ ] **Step 4: Run full suite locally**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Run small server run**

Run: `ssh root@192.168.1.69 'cd /root/wifiopt && cargo test && ./target/release/wifiopt --date CURRENT --reg reg1 --out ./out/'`
Expected: PASS with CSV and `summary.json` in `./out/`, no database writes.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/golden_test.rs README.md
git commit -m "feat: orchestrate optimizer run"
```
