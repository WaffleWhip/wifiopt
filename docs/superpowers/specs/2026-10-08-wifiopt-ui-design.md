# WiFiOpt UI Design

Date: 2026-10-08
Status: approved for build
Scope: control and visualization web UI, co-hosted on `192.168.1.69`.

## Stack

Next.js App Router (TypeScript), shadcn latest, Tailwind CSS v4, ESLint, Recharts via shadcn Chart. Node 22 on server, npm. Deploy as systemd service on port 3000. Dev locally with `next dev`, sync to server for runs.

## Pages

- Dashboard: latest-run cards per region (rows, CHANGE counts, avg cost delta), cost-reduction bar chart, run history mini table.
- Runs: trigger form (date defaults today, region ToggleGroup multi-select default all, stale threshold), live job log tail, history Table with status and row counts.
- Compare: ours vs `wifi_optimize_*` aggregates side-by-side table plus chart (avg cost before/after per band, CHANGE share).
- Settings: cron hour/minute flex form, environment status (binary present, DB reachable SELECT-only check, out dir size).

## API (Next.js Route Handlers, Node runtime)

- `GET /api/runs` — scan `/root/wifiopt/out` for CSVs plus `summary.json` entries.
- `POST /api/runs {date, regs, stale}` — validate, spawn binary detached with log file, return job id.
- `GET /api/jobs/[id]` — status plus log tail.
- `GET /api/compare?date=` — aggregate our CSVs and `wifi_optimize_*` via mysql2 read-only pool.
- `GET/PUT /api/cron` — read and rewrite the single wifiopt crontab line plus hour config.
- `GET /api/health` — binary version, out dir, DB ping.

## Constraints

- Credentials only via environment file on server, never in repo.
- MySQL access is SELECT-only (enforced by using a read-only user and query audit).
- Trigger validates date format `YYYYMMDD` and region names.
- ESLint clean, shadcn critical rules followed (semantic colors, FieldGroup forms, no space-x, size-* avatars).
