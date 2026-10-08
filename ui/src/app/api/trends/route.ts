import { NextResponse, connection } from "next/server";
import { trends } from "@/lib/wifiopt";

export async function GET(req: Request) {
  await connection();
  const { searchParams } = new URL(req.url);
  const days = Number(searchParams.get("days") ?? 14);
  return NextResponse.json(await trends(days));
}
