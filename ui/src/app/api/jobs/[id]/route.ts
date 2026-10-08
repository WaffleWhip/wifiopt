import fs from "node:fs";
import { NextResponse } from "next/server";
import { getJob, jobLogPath } from "@/lib/wifiopt";

export async function GET(_req: Request, ctx: { params: Promise<{ id: string }> }) {
  const { id } = await ctx.params;
  const job = getJob(id);
  if (!job) return NextResponse.json({ error: "no job" }, { status: 404 });
  let log = "";
  try {
    const data = fs.readFileSync(jobLogPath(id), "utf8");
    log = data.split("\n").slice(-60).join("\n");
  } catch {
    log = "";
  }
  return NextResponse.json({ job, log });
}
