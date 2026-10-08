import { NextResponse } from "next/server";
import { listJobs, listRuns, readSummary, startJob } from "@/lib/wifiopt";

export async function GET() {
  return NextResponse.json({
    runs: listRuns(),
    summary: readSummary(),
    jobs: listJobs(),
  });
}

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const date: string =
      body.date && String(body.date).trim() !== ""
        ? String(body.date)
        : new Date().toISOString().slice(0, 10).replaceAll("-", "");
    const regs: string[] = Array.isArray(body.regs) && body.regs.length ? body.regs : ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];
    const stale = Number(body.stale ?? 50);
    const job = startJob(date, regs, stale);
    return NextResponse.json({ job }, { status: 202 });
  } catch (e) {
    return NextResponse.json({ error: (e as Error).message }, { status: 400 });
  }
}
