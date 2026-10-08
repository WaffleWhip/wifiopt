# wifiopt

Fetch-only WiFi channel optimizer. Reads `discover_*` from MySQL, writes file output with the exact `wifi_optimize` 23-column shape. Never writes to MySQL.

## Usage

```bash
wifiopt --date 20261007 --reg all --out ./out/
wifiopt --date 20261007 --reg reg1,reg2 --out ./out/
```

`--date` defaults to today (`YYYYMMDD`). `--reg` defaults to `all` (`reg1..reg7`).

Connection via `--db-url` or `DATABASE_URL` env. Credentials never live in the repo.

## Run on server

```bash
scp -r Cargo.toml Cargo.lock src tests root@192.168.1.69:/root/wifiopt/
ssh root@192.168.1.69 "cd /root/wifiopt && cargo build --release && DATABASE_URL='mysql://USER:PASS@192.168.41.115:3306/wifi_optimization' ./target/release/wifiopt --date $(date +%Y%m%d) --reg all --out ./out/"
```

Daily cron at a flexible hour calls the same binary with the current date.

## Layout

`config`, `db` (SELECT only), `discover`, `graph`, `cost`, `output`, `pipeline`.
