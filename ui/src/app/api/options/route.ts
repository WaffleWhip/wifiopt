import { NextResponse, connection } from "next/server";
import { listOptions } from "@/lib/wifiopt";

export async function GET() {
  await connection();
  return NextResponse.json({ dates: await listOptions() });
}
