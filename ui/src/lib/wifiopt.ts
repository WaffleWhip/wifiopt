import { execFile, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export const OUT_DIR =
  process.env.WIFIPT_OUT ?? "/root/wifiopt/out";
export const BIN =
  process.env.WIFIPT_BIN ?? "/root/wifiopt/target/release/wifiopt";
export const JOBS_DIR = path.join(/*turbopackIgnore: true*/ OUT_DIR, "jobs");

export type RunEntry = {
  date: string;
  reg: string;
  rows: number | null;
  bytes: number;
  mtime: string;
};

export function listRuns(): RunEntry[] {
  if (!fs.existsSync(/*turbopackIgnore: true*/ OUT_DIR)) return [];
  const files = fs
    .readdirSync(OUT_DIR)
    .filter((f) => f.startsWith("wifi_optimize_") && f.endsWith(".csv"));
  return files
    .map((f) => {
      const m = f.match(/^wifi_optimize_(\d{8})_(reg\d+)\.csv$/);
      if (!m) return null;
      const st = fs.statSync(/*turbopackIgnore: true*/ path.join(/*turbopackIgnore: true*/ OUT_DIR, f));
      let rows: number | null = null;
      try {
        const head = fs.readFileSync(/*turbopackIgnore: true*/ path.join(/*turbopackIgnore: true*/ OUT_DIR, f), "utf8").split("\n");
        rows = head.length > 1 ? head.length - 1 - (head[head.length - 1] === "" ? 1 : 0) : 0;
      } catch {
        rows = null;
      }
      return {
        date: m[1],
        reg: m[2],
        rows,
        bytes: st.size,
        mtime: st.mtime.toISOString(),
      };
    })
    .filter((r): r is RunEntry => r !== null)
    .sort((a, b) =>
      a.date === b.date ? a.reg.localeCompare(b.reg) : b.date.localeCompare(a.date),
    );
}

export function readSummary(): { date: string; regs: string[]; rows: number } | null {
  const p = path.join(/*turbopackIgnore: true*/ OUT_DIR, "summary.json");
  if (!fs.existsSync(/*turbopackIgnore: true*/ p)) return null;
  try {
    return JSON.parse(fs.readFileSync(/*turbopackIgnore: true*/ p, "utf8"));
  } catch {
    return null;
  }
}

export type Job = {
  id: string;
  date: string;
  regs: string[];
  stale: number;
  status: "running" | "done" | "failed";
  startedAt: string;
  finishedAt: string | null;
};

const jobs = new Map<string, Job>();

export function getJob(id: string): Job | undefined {
  return jobs.get(id);
}

export function listJobs(): Job[] {
  return [...jobs.values()].reverse();
}

export function jobLogPath(id: string): string {
  return path.join(/*turbopackIgnore: true*/ JOBS_DIR, `${id}.log`);
}

export function startJob(date: string, regs: string[], stale: number): Job {
  if (!/^\d{8}$/.test(date)) throw new Error("bad date");
  if (!regs.length || !regs.every((r) => /^reg\d+$/.test(r))) throw new Error("bad regs");
  const id = `${date}-${Date.now().toString(36)}`;
  fs.mkdirSync(/*turbopackIgnore: true*/ JOBS_DIR, { recursive: true });
  const logPath = jobLogPath(id);
  const job: Job = {
    id,
    date,
    regs,
    stale,
    status: "running",
    startedAt: new Date().toISOString(),
    finishedAt: null,
  };
  jobs.set(id, job);
  const log = fs.openSync(/*turbopackIgnore: true*/ logPath, "w");
  const args = ["--date", date, "--reg", regs.join(","), "--out", OUT_DIR, "--stale", String(stale)];
  const child = spawn(BIN, args, {
    detached: true,
    stdio: ["ignore", log, log],
    env: process.env,
  });
  child.unref();
  child.on("close", (code) => {
    fs.closeSync(log);
    const j = jobs.get(id);
    if (j) {
      j.status = code === 0 ? "done" : "failed";
      j.finishedAt = new Date().toISOString();
    }
  });
  return job;
}

export async function readCrontab(): Promise<string[]> {
  try {
    const { stdout } = await execFileAsync("crontab", ["-l"]);
    return stdout.split("\n");
  } catch {
    return [];
  }
}

export async function findWifioptCron(): Promise<string | null> {
  const lines = await readCrontab();
  return lines.find((l) => l.includes("wifiopt") && !l.trim().startsWith("#")) ?? null;
}

export async function setWifioptCron(minute: number, hour: number): Promise<string> {
  if (minute < 0 || minute > 59 || hour < 0 || hour > 23) throw new Error("bad time");
  const line = `${minute} ${hour} * * * ${BIN} --date CURRENT --reg all --out ${OUT_DIR} >> ${OUT_DIR}/cron.log 2>&1`;
  const lines = (await readCrontab()).filter(
    (l) => !(l.includes("wifiopt") && !l.trim().startsWith("#")),
  );
  lines.push(line);
  const { spawnSync } = await import("node:child_process");
  const r = spawnSync("crontab", ["-"], { input: lines.filter(Boolean).join("\n") + "\n" });
  if (r.status !== 0) throw new Error("crontab install failed");
  return line;
}
