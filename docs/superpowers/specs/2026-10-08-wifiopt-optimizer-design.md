# WiFi Optimize (Backend First) Design

Date: 2026-10-08
Status: draft, backend scope only
Scope: Rust optimizer core, fetch-only MySQL. UI web and scheduler are separate subsystems, deferred.

## Intent

Build a Rust optimizer that reproduces `wifi_optimize_*` output exactly in shape, without writing to MySQL. Development happens locally, execution happens on `192.168.1.69` (`wifiopt`, 16 core, 16 GB, Rust 1.99.0). Local toolchain is Rust 1.95.0; server toolchain is authoritative. Use latest stable dependencies at init time.

What user said versus assumptions:
- Said: optimize channel like `wifi_optimize` table exactly, full Rust.
- Said: approach A, backend first, fetch-only MySQL, dev local and run on server.
- Said: latest versions, UI with shadcn plus Tailwind latest, everything manageable via UI, default all regions with flexible single or subset selection, daily cron on current date at flexible hour.
- Assumed: backend spec covers optimizer only. UI and scheduler get their own specs. Optimizer exposes CLI flags so UI and scheduler can drive it later.
- Assumed: metadata columns (`node_id`, `node_ip`, `slot`, `port`, `onu_id`, `regional`, `witel`, `cpe_product_class`) are passthrough and nullable when inventory is absent.

## Inputs

Database `wifi_optimization` on `192.168.41.115:3306`, MySQL 8.4.10, read-only access.

Tables read:
- `discover_interference_reborn_YYYYMMDD_reg1..7` (daily per region, 308 tables matching `discover%`, plus `discover_interference_reborn_template` empty).
- `list_sn_bssid`, `list_sn_bssid_new` for BSSID to serial mapping.
- Existing `wifi_optimize_YYYYMMDD` only as golden reference for testing, never written.

`discover` row shape:
- `CPE_Serial_Number` PK, `channel_ont`, `channel_ont5Ghz`, `json_config`, `json_neighbor`, `updated_at`.
- `json_config` per SSID: `channel`, `possible_channel`, `bssid`, `ssid_name`.
- `json_neighbor` list: `channel`, `signal_strength`, `bssid`, `ssid`.

Example scale: `wifi_optimize_20261007` has 2,952,262 rows; `wifi_optimize_20260930` has 3,216,138 rows. Optimizer must handle millions of rows with parallel execution.

No writes to MySQL. The optimizer issues `SELECT` only. Enforcement by code review, query audit in tests, and read-only credentials where possible.

## Output

No MySQL writes. Output goes to files: one CSV per region plus `summary.json`, with the same 23-column shape as `wifi_optimize_*`:

`reg`, `sn`, `clusterid_2g`, `ch_before_2g`, `cost_before_2g`, `ch_after_2g`, `cost_after_2g`, `status_2g`, `clusterid_5g`, `ch_before_5g`, `cost_before_5g`, `ch_after_5g`, `cost_after_5g`, `status_5g`, `updated_at`, `node_id`, `node_ip`, `slot`, `port`, `onu_id`, `regional`, `witel`, `cpe_product_class`.

`updated_at` is generation time. Metadata fields are null unless an inventory source supplies them.

## Architecture

Local repository holds Rust source. Remote server `192.168.1.69` builds and runs the binary. Flow:

1. Resolve `date` (default today) and region list (default all `reg1..reg7`, flexible subset).
2. Load `discover_*` rows for the selected date and regions plus the BSSID index.
3. Build the interference graph, find connected components as clusters.
4. Compute `cost_before` from `channel_ont` and `channel_ont5Ghz`.
5. Greedy channel assignment for minimal cost within `possible_channel`.
6. Compute `cost_after` and `status`.
7. Write rows to file output and print summary counts.

CLI sketch:

```text
wifiopt --date 2026-10-07 --reg all --out ./out/
wifiopt --date 2026-10-07 --reg reg1,reg2 --out ./out/
```

Scheduler and UI are out of scope for this spec but the CLI must support them later: date defaults to current date, region accepts `all` or an explicit subset, and cron runs the same binary at a flexible hour.

## Components

- `config`: CLI parsing for date, region selection, output directory, DB connection.
- `db`: read-only MySQL access, `SELECT` only, connection retry.
- `discover`: parse `json_config` and `json_neighbor`, normalize channels and signal strength.
- `graph`: edges when a neighbor BSSID maps to a known serial and RSSI passes threshold.
- `cluster`: connected components, deterministic IDs sorted by minimum serial in each component.
- `cost`: 2.4 GHz co-channel 1.0 and adjacent 0.5 or 0.25, 5 GHz co-channel only, RSSI-weighted. Only neighbors with signal stronger than `-85` dBm count. `STAY` when improvement is `0.05` or less. Thresholds are CLI-tunable with these defaults.
- `output`: CSV writer per region plus `summary.json`, with atomic temp-file rename.
- `main`: orchestration, logging per region, exit codes.

## Data Flow

Resolve date and regions. Load `discover_*` plus BSSID index. Build graph. Cluster. Compute `cost_before` from current channels. Greedy assign `ch_after` within allowed channels. Compute `cost_after` and `status`. Enrich nullable metadata. Write files and log counts. No MySQL writes at any step.

## Error Handling

- DB disconnect: retry 3 times with backoff, then fail only the affected region and continue others.
- Malformed JSON: skip the row and log it.
- Empty neighbors: cost 0 and `STAY`.
- Empty `possible_channel`: fall back to 2.4 GHz `1-13` and 5 GHz `36,40,44,48,52,56,60,64,149,153,157,161`.
- Missing discover table for a region and date: fail that region only.
- File output: write to a temp file and rename for atomicity so reruns are idempotent.
- Exit codes and per-region logs for cron visibility.

## Testing

- Unit tests for parsing, cost, greedy assignment, and clustering.
- Integration tests on fixtures without a database.
- Golden comparison against existing `wifi_optimize_*` rows for shape and plausible values.
- Query audit test that fails on any non-`SELECT` statement.
- Performance test on a subset with parallel execution enabled.

## Deferred

- UI with shadcn and Tailwind latest, full management via UI.
- Scheduler for daily current-date runs at a flexible hour.
- Region UI with default all and flexible subset selection.
- MySQL output writes, if ever wanted, need a separate approved spec.
