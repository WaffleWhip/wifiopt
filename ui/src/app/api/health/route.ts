import fs from "node:fs";
import { NextResponse, connection } from "next/server";
import { BIN, OUT_DIR } from "@/lib/wifiopt";

export async function GET() {
  await connection();
  const bin = fs.existsSync(BIN);
  const out = fs.existsSync(OUT_DIR);
  let db: string = "unknown";
  if (process.env.MYSQL_USER) {
    try {
      const { default: mysql } = await import("mysql2/promise");
      const conn = await mysql.createConnection({
        host: process.env.MYSQL_HOST ?? "192.168.41.115",
        port: Number(process.env.MYSQL_PORT ?? 3306),
        user: process.env.MYSQL_USER,
        password: process.env.MYSQL_PASSWORD ?? "",
        database: process.env.MYSQL_DATABASE ?? "wifi_optimization",
      });
      await conn.query("SELECT 1");
      await conn.end();
      db = "ok";
    } catch (e) {
      db = (e as Error).message;
    }
  } else {
    db = "MYSQL_USER not set";
  }
  return NextResponse.json({ bin, out, outDir: OUT_DIR, binPath: BIN, db });
}
