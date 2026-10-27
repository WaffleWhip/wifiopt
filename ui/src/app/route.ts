import { NextResponse, connection } from "next/server";

const ENDPOINTS = [
  { method: "GET", path: "/", desc: "this index" },
  { method: "GET", path: "/api/health", desc: "binary, out dir, cron, jobs, recent errors, db ping" },
  { method: "GET", path: "/api/runs", desc: "csv outputs, summary.json, jobs" },
  {
    method: "POST",
    path: "/api/runs",
    desc: 'start job {date:"YYYYMMDD", regs:["reg1"..], stale:number}',
  },
  { method: "GET", path: "/api/jobs/[id]", desc: "job status + last 60 log lines" },
  { method: "GET", path: "/api/options?days=", desc: "available dates+regs (mysql or disk)" },
  { method: "GET", path: "/api/trends?days=N", desc: "daily cost/change aggregates, ours vs mysql" },
  { method: "GET", path: "/api/compare?date=YYYYMMDD", desc: "per-reg ours-vs-mysql aggregates" },
  { method: "GET", path: "/api/cron", desc: "installed wifiopt cron line" },
  { method: "PUT", path: "/api/cron", desc: 'set cron {minute, hour}' },
  { method: "GET", path: "/api/settings", desc: "mysql pointing (password redacted)" },
  { method: "PUT", path: "/api/settings", desc: "update mysql pointing, restarts service" },
];

export async function GET() {
  await connection();
  return NextResponse.json({
    service: "wifiopt-api",
    ui: false,
    time: new Date().toISOString(),
    endpoints: ENDPOINTS,
  });
}
