import { NextResponse, connection } from "next/server";
import { findWifioptCron, setWifioptCron } from "@/lib/wifiopt";

export async function GET() {
  await connection();
  return NextResponse.json({ line: await findWifioptCron() });
}

export async function PUT(req: Request) {
  await connection();
  try {
    const body = await req.json();
    const line = await setWifioptCron(Number(body.minute ?? 30), Number(body.hour ?? 5));
    return NextResponse.json({ line });
  } catch (e) {
    return NextResponse.json({ error: (e as Error).message }, { status: 400 });
  }
}
