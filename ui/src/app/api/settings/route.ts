import { NextResponse, connection } from "next/server";
import { readMysql, scheduleRestart, writeMysql } from "@/lib/wifiopt";

export async function GET() {
  await connection();
  return NextResponse.json(readMysql());
}

export async function PUT(req: Request) {
  await connection();
  try {
    const body = await req.json();
    writeMysql({
      host: String(body.host ?? ""),
      port: String(body.port ?? ""),
      user: String(body.user ?? ""),
      password: body.password ? String(body.password) : undefined,
      database: String(body.database ?? ""),
    });
    scheduleRestart();
    return NextResponse.json({ ok: true, restart: "scheduled" });
  } catch (e) {
    return NextResponse.json({ error: (e as Error).message }, { status: 400 });
  }
}
