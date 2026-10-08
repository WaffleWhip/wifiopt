import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { NextResponse, connection } from "next/server";
import { OUT_DIR } from "@/lib/wifiopt";

type Agg = {
  n: number;
  cb2: number;
  ca2: number;
  cb5: number;
  ca5: number;
  ch2: number;
  ch5: number;
};

async function aggCsv(file: string): Promise<Agg> {
  const a: Agg = { n: 0, cb2: 0, ca2: 0, cb5: 0, ca5: 0, ch2: 0, ch5: 0 };
  let c2 = 0;
  let c5 = 0;
  const rl = readline.createInterface({
    input: fs.createReadStream(/*turbopackIgnore: true*/ file),
    crlfDelay: Infinity,
  });
  let header: string[] | null = null;
  for await (const line of rl) {
    if (!header) {
      header = line.split(",");
      continue;
    }
    if (!line.trim()) continue;
    // CSV has no quoted commas in our output; positional parse is enough.
    const v = line.split(",");
    const num = (i: number) => {
      const s = v[i] ?? "";
      return s === "" ? null : Number(s);
    };
    a.n += 1;
    const b2 = num(4);
    const t2 = num(6);
    const b5 = num(10);
    const t5 = num(12);
    if (b2 !== null) {
      a.cb2 += b2;
      c2 += 1;
    }
    if (t2 !== null) a.ca2 += t2;
    if (b5 !== null) {
      a.cb5 += b5;
      c5 += 1;
    }
    if (t5 !== null) a.ca5 += t5;
    if (v[7] === "CHANGE") a.ch2 += 1;
    if (v[14] === "CHANGE") a.ch5 += 1;
  }
  a.cb2 /= Math.max(c2, 1);
  a.ca2 /= Math.max(c2, 1);
  a.cb5 /= Math.max(c5, 1);
  a.ca5 /= Math.max(c5, 1);
  return a;
}

async function aggMysql(date: string, reg: string) {
  const { default: mysql } = await import("mysql2/promise");
  const conn = await mysql.createConnection({
    host: process.env.MYSQL_HOST ?? "192.168.41.115",
    port: Number(process.env.MYSQL_PORT ?? 3306),
    user: process.env.MYSQL_USER ?? "",
    password: process.env.MYSQL_PASSWORD ?? "",
    database: process.env.MYSQL_DATABASE ?? "wifi_optimization",
  });
  try {
    const [rows] = (await conn.query(
      "SELECT COUNT(*) n, AVG(cost_before_2g) cb2, AVG(cost_after_2g) ca2, AVG(cost_before_5g) cb5, AVG(cost_after_5g) ca5, SUM(status_2g='CHANGE') ch2, SUM(status_5g='CHANGE') ch5 FROM wifi_optimize_" +
        date.replaceAll(/[^0-9]/g, "") +
        " WHERE reg=?",
      [reg],
    )) as unknown as [Record<string, number | null>[]];
    return rows[0] ?? null;
  } finally {
    await conn.end();
  }
}

export async function GET(req: Request) {
  await connection();
  const { searchParams } = new URL(req.url);
  const date = (searchParams.get("date") ?? "").replaceAll(/[^0-9]/g, "");
  if (!/^\d{8}$/.test(date)) {
    return NextResponse.json({ error: "bad date, use YYYYMMDD" }, { status: 400 });
  }
  const regs = ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];
  const ours: Record<string, Agg | null> = {};
  for (const reg of regs) {
    const f = path.join(OUT_DIR, `wifi_optimize_${date}_${reg}.csv`);
    ours[reg] = fs.existsSync(/*turbopackIgnore: true*/ f) ? await aggCsv(f) : null;
  }
  let mysql: Record<string, unknown> | null = null;
  let mysqlError: string | null = null;
  if (process.env.MYSQL_USER) {
    try {
      const out: Record<string, unknown> = {};
      for (const reg of regs) {
        out[reg] = await aggMysql(date, reg);
      }
      mysql = out;
    } catch (e) {
      mysqlError = (e as Error).message;
    }
  } else {
    mysqlError = "MYSQL_USER not set";
  }
  return NextResponse.json({ date, ours, mysql, mysqlError });
}
