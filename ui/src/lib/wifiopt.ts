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
  const live = jobs.get(id);
  if (live) return live;
  try {
    return JSON.parse(fs.readFileSync(/*turbopackIgnore: true*/ jobFile(id), "utf8")) as Job;
  } catch {
    return undefined;
  }
}

export function listJobs(): Job[] {
  const out: Job[] = [...jobs.values()];
  try {
    if (fs.existsSync(/*turbopackIgnore: true*/ JOBS_DIR)) {
      for (const f of fs.readdirSync(/*turbopackIgnore: true*/ JOBS_DIR)) {
        if (!f.endsWith(".json")) continue;
        try {
          const j = JSON.parse(
            fs.readFileSync(/*turbopackIgnore: true*/ path.join(JOBS_DIR, f), "utf8"),
          ) as Job;
          if (!out.some((x) => x.id === j.id)) out.push(j);
        } catch {
          // skip corrupt
        }
      }
    }
  } catch {
    // ignore
  }
  return out.sort((a, b) => b.startedAt.localeCompare(a.startedAt));
}

export function jobLogPath(id: string): string {
  return path.join(/*turbopackIgnore: true*/ JOBS_DIR, `${id}.log`);
}

export function jobFile(id: string): string {
  return path.join(/*turbopackIgnore: true*/ JOBS_DIR, `${id}.json`);
}

function persistJob(job: Job) {
  try {
    fs.mkdirSync(/*turbopackIgnore: true*/ JOBS_DIR, { recursive: true });
    fs.writeFileSync(
      /*turbopackIgnore: true*/ jobFile(job.id),
      JSON.stringify(job),
    );
  } catch {
    // history best-effort
  }
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
  persistJob(job);
  const log = fs.openSync(/*turbopackIgnore: true*/ logPath, "w");
  const args = ["--date", date, "--reg", regs.join(","), "--out", OUT_DIR, "--stale", String(stale)];
  const env = { ...process.env };
  if (!env.DATABASE_URL && process.env.MYSQL_USER) {
    const host = process.env.MYSQL_HOST ?? "192.168.41.115";
    const port = process.env.MYSQL_PORT ?? "3306";
    const user = process.env.MYSQL_USER ?? "";
    const pass = process.env.MYSQL_PASSWORD ?? "";
    const db = process.env.MYSQL_DATABASE ?? "wifi_optimization";
    env.DATABASE_URL = `mysql://${user}:${pass}@${host}:${port}/${db}`;
  }
  const child = spawn(BIN, args, {
    detached: true,
    stdio: ["ignore", log, log],
    env,
  });
  child.unref();
  child.on("close", (code) => {
    fs.closeSync(/*turbopackIgnore: true*/ log);
    const j = jobs.get(id);
    if (j) {
      j.status = code === 0 ? "done" : "failed";
      j.finishedAt = new Date().toISOString();
      persistJob(j);
    }
  });
  return job;
}

export async function listOptions(): Promise<{ date: string; regs: string[] }[]> {
  const byDate = new Map<string, Set<string>>();
  try {
    if (process.env.MYSQL_USER) {
      const { default: mysql } = await import("mysql2/promise");
      const conn = await mysql.createConnection({
        host: process.env.MYSQL_HOST ?? "192.168.41.115",
        port: Number(process.env.MYSQL_PORT ?? 3306),
        user: process.env.MYSQL_USER,
        password: process.env.MYSQL_PASSWORD ?? "",
        database: process.env.MYSQL_DATABASE ?? "wifi_optimization",
      });
      try {
        const [rows] = (await conn.query(
          "SELECT TABLE_NAME t FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME LIKE 'discover_interference_reborn\\_%'",
        )) as unknown as [{ t: string }[]];
        for (const r of rows) {
          const m = r.t.match(/^discover_interference_reborn_(\d{8})_(reg\d+)$/);
          if (!m || m[2] === "template") continue;
          if (!byDate.has(m[1])) byDate.set(m[1], new Set());
          byDate.get(m[1])!.add(m[2]);
        }
      } finally {
        await conn.end();
      }
    }
  } catch {
    // fall through to disk scan
  }
  if (byDate.size === 0 && fs.existsSync(/*turbopackIgnore: true*/ OUT_DIR)) {
    for (const f of fs.readdirSync(/*turbopackIgnore: true*/ OUT_DIR)) {
      const m = f.match(/^wifi_optimize_(\d{8})_(reg\d+)\.csv$/);
      if (!m) continue;
      if (!byDate.has(m[1])) byDate.set(m[1], new Set());
      byDate.get(m[1])!.add(m[2]);
    }
  }
  return [...byDate.entries()]
    .map(([date, regs]) => ({ date, regs: [...regs].sort() }))
    .sort((a, b) => b.date.localeCompare(a.date));
}

const ENV_PATH = process.env.WIFIPT_ENV ?? path.resolve(process.cwd(), ".env");

export type MysqlSettings = {
  host: string;
  port: string;
  user: string;
  database: string;
  hasPassword: boolean;
};

function parseEnvFile(): Record<string, string> {
  const out: Record<string, string> = {};
  try {
    const text = fs.readFileSync(/*turbopackIgnore: true*/ ENV_PATH, "utf8");
    for (const line of text.split("\n")) {
      const m = line.match(/^([A-Z_]+)=(.*)$/);
      if (m) out[m[1]] = m[2];
    }
  } catch {
    // fall back to process env below
  }
  return out;
}

export function readMysql(): MysqlSettings {
  const f = parseEnvFile();
  const get = (k: string) => f[k] ?? process.env[k] ?? "";
  return {
    host: get("MYSQL_HOST") || "192.168.41.115",
    port: get("MYSQL_PORT") || "3306",
    user: get("MYSQL_USER"),
    database: get("MYSQL_DATABASE") || "wifi_optimization",
    hasPassword: Boolean(get("MYSQL_PASSWORD")),
  };
}

export function writeMysql(s: {
  host: string;
  port: string;
  user: string;
  password?: string;
  database: string;
}): void {
  if (!s.host || !s.user || !s.database) throw new Error("host, user, database required");
  if (!/^\d+$/.test(s.port)) throw new Error("bad port");
  const cur = parseEnvFile();
  const next = {
    ...cur,
    MYSQL_HOST: s.host.trim(),
    MYSQL_PORT: s.port.trim(),
    MYSQL_USER: s.user.trim(),
    MYSQL_DATABASE: s.database.trim(),
    ...(s.password ? { MYSQL_PASSWORD: s.password } : {}),
  };
  const body =
    Object.entries(next)
      .map(([k, v]) => `${k}=${v}`)
      .join("\n") + "\n";
  fs.writeFileSync(/*turbopackIgnore: true*/ ENV_PATH, body, { mode: 0o600 });
}

export function scheduleRestart(): void {
  try {
    const child = spawn("sh", ["-c", "sleep 2 && systemctl restart wifiopt-ui"], {
      detached: true,
      stdio: "ignore",
    });
    child.unref();
  } catch {
    // manual restart needed
  }
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
