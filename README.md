# wifiopt

WiFi channel optimization engine for large-scale ONT/CPE fleets. Reads network topology and interference matrices from discovery data, constructs interference graphs, clusters interacting devices, and assigns optimal channels to minimize co-channel and adjacent-channel interference.

## Spectral Interference Model

Interference $I$ between a candidate channel $C$ and neighbor $N$ is calculated as the spectral overlap weighted by received signal strength indicator (RSSI):

$$I = \sum_{N} \text{Overlap}(C, N) \cdot \text{Weight}(RSSI_N)$$

Where $\text{Overlap}$ is the bandwidth-normalized frequency distance:

$$\text{Overlap}(C, N) = \min\left(1.0, \frac{\frac{BW_C}{2} + \frac{BW_N}{2} - |Freq(C) - Freq(N)|}{BW_C}\right)$$

- **2.4 GHz**: 20 MHz channels, centered at $2407 + 5C$ MHz.
- **5 GHz**: 80 MHz candidate / 20 MHz neighbor, centered at $5000 + 5C$ MHz.

## Features

- **Dual-Engine Optimizer**: Exact combinatorial search for clusters within size bounds; coordinate descent fallback for large connected components.
- **Strict Channel Planning**: Enforces 2.4 GHz non-overlapping channels (`1, 6, 11`) and 5 GHz non-DFS channels (`36, 40, 44, 48, 149, 153, 157, 161, 165`). Off-plan and DFS assignments are rejected.
- **Safety Gates**:
  - Preflight checks verify discovery table row counts across all regions prior to truncating target tables.
  - Output plan validation aborts uploads if invalid or out-of-plan channels are detected.
- **Pipelined Processing**: Pipelined region computation and batched database writes (`INSERT ... ON DUPLICATE KEY UPDATE`).
- **Control Plane & Scheduler**: Built-in HTTP server with zero external web framework dependencies, providing REST endpoints, scheduling, status reporting, and trend metrics.

## Architecture

| Component | Responsibility |
|---|---|
| `src/config.rs` | CLI arguments, region parsing, and runtime options |
| `src/db.rs` | MySQL ingestion, read preflight validation, transient retry logic |
| `src/discover.rs` | Telemetry parser, radio/neighbor normalization, multi-radio handling |
| `src/graph.rs` | Interference graph construction and DSU component clustering |
| `src/cost.rs` | Spectral overlap cost matrix, exhaustive search & coordinate descent |
| `src/output.rs` | Atomic 23-column CSV generation and JSON summary generation |
| `src/pipeline.rs` | Region orchestration, 2.4G/5G joining, safety validation gates |
| `src/upload.rs` | Schema migration, batched upload execution, transactional safety |
| `src/api.rs` | HTTP REST endpoints, background worker, and daily scheduler |
| `src/bin/server.rs` | Control plane server binary entry point |

## Quick Start

### Build & Test

```bash
# Build binaries
cargo build --release

# Run unit and regression tests
cargo test
cargo test --test golden_test
```

### CLI Execution

```bash
# Optimize all regions for today and write CSV output
./target/release/wifiopt --date 20261029 --reg all --out ./out/

# Optimize specific regions
./target/release/wifiopt --date 20261029 --reg reg1,reg2 --out ./out/

# Optimize and upload directly to MySQL
./target/release/wifiopt --date 20261029 --reg all --out ./out/ --upload
```

### Server Mode

Run the control plane daemon:

```bash
cargo run --release --bin server
```

Configure through `.env` or environment variables:
- `PORT`: HTTP port (default: `3000`)
- `WIFIPT_BIN`: Path to `wifiopt` binary
- `WIFIPT_OUT`: Base output directory for generated CSVs
- `DATABASE_URL`: MySQL connection string (`mysql://user:pass@host:3306/db`)
- `SCHEDULE_HOUR`, `SCHEDULE_MINUTE`: Daily job execution time
- `SCHEDULE_UPLOAD`: `true` to persist optimized plans to database tables

## Output Contract

Emits CSV files with the standard 23-column format:

```text
reg,sn,clusterid_2g,ch_before_2g,cost_before_2g,ch_after_2g,cost_after_2g,status_2g,
clusterid_5g,ch_before_5g,cost_before_5g,ch_after_5g,cost_after_5g,status_5g,
updated_at,node_id,node_ip,slot,port,onu_id,regional,witel,cpe_product_class
```

Status indicates transition status (`STAY` vs `CHANGE`). Cost baselines are normalized against joint cluster totals.
