"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

const REGS = ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];

type Job = {
  id: string;
  date: string;
  regs: string[];
  status: string;
};

export default function RunsPage() {
  const [dates, setDates] = useState<{ date: string; regs: string[] }[]>([]);
  const [date, setDate] = useState("");
  const [regs, setRegs] = useState<string[]>(REGS);
  const [stale, setStale] = useState("50");
  const [jobs, setJobs] = useState<Job[]>([]);
  const [activeLog, setActiveLog] = useState("");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cron, setCron] = useState("");
  const [hour, setHour] = useState("5");
  const [minute, setMinute] = useState("30");

  async function refreshJobs() {
    const r = await fetch("/api/runs");
    const j = await r.json();
    setJobs(j.jobs ?? []);
  }

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const r = await fetch("/api/runs");
      const j = await r.json();
      if (cancelled) return;
      setJobs(j.jobs ?? []);
      try {
        const o = await fetch("/api/options");
        const oj = await o.json();
        if (cancelled) return;
        const list = oj.dates ?? [];
        setDates(list);
        if (list.length) {
          setDate(list[0].date);
          setRegs(list[0].regs);
        }
      } catch {
        // keep manual entry
      }
      const c = await fetch("/api/cron").then((r) => r.json());
      if (cancelled) return;
      setCron(c.line ?? "none");
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function saveCron() {
    try {
      const r = await fetch("/api/cron", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hour: Number(hour), minute: Number(minute) }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error ?? "save failed");
      toast.success("cron updated");
      setCron(j.line ?? "");
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  const availRegs = dates.find((d) => d.date === date)?.regs ?? REGS;

  async function trigger() {
    setBusy(true);
    try {
      const r = await fetch("/api/runs", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ date, regs, stale: Number(stale) }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error ?? "trigger failed");
      toast.success(`job started: ${j.job.id}`);
      refreshJobs();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function viewLog(id: string) {
    const r = await fetch(`/api/jobs/${id}`);
    const j = await r.json();
    if (!r.ok) {
      toast.error(j.error ?? "job gone (server restarted?)");
      return;
    }
    setActiveLog(j.log === "" ? "(empty so far, job still loading first region)" : j.log);
    setActiveId(id);
  }

  useEffect(() => {
    if (!activeId) return;
    const t = setInterval(async () => {
      const r = await fetch(`/api/jobs/${activeId}`);
      const j = await r.json();
      if (!r.ok) {
        clearInterval(t);
        return;
      }
      setActiveLog(j.log === "" ? "(empty so far, job still loading first region)" : j.log);
      if (j.job.status !== "running") {
        clearInterval(t);
        toast(j.job.status === "done" ? "job done" : "job failed");
        refreshJobs();
      }
    }, 5000);
    return () => clearInterval(t);
  }, [activeId]);

  return (
    <div className="h-full min-h-0 flex flex-col gap-4 overflow-y-auto">
      <Card tone="white">
        <CardHeader>
          <CardTitle>Trigger run</CardTitle>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="date">Date</FieldLabel>
              {dates.length ? (
                <Select
                  value={date}
                  onValueChange={(v) => {
                    if (!v) return;
                    setDate(v);
                    setRegs(dates.find((d) => d.date === v)?.regs ?? REGS);
                  }}
                >
                  <SelectTrigger id="date">
                    <SelectValue placeholder="date" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {dates.map((d) => (
                        <SelectItem key={d.date} value={d.date}>
                          {d.date} ({d.regs.length} regs)
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              ) : (
                <Input
                  id="date"
                  placeholder="20261007"
                  value={date}
                  onChange={(e) => setDate(e.target.value)}
                />
              )}
            </Field>
            <Field>
              <FieldLabel>Regions</FieldLabel>
              <div className="flex flex-wrap gap-2">
                <ToggleGroup
                  value={regs}
                  onValueChange={(v) => v.length && setRegs(v as string[])}
                >
                  {availRegs.map((r) => (
                    <ToggleGroupItem key={r} value={r} aria-label={r}>
                      {r}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>
            </Field>
            <Field>
              <FieldLabel htmlFor="stale">Stale threshold</FieldLabel>
              <Input id="stale" value={stale} onChange={(e) => setStale(e.target.value)} />
            </Field>
            <Button onClick={trigger} disabled={busy}>
              {busy ? "Starting..." : "Start run"}
            </Button>
          </FieldGroup>
        </CardContent>
      </Card>
      <div className="grid gap-4 lg:grid-cols-2 [&>*]:min-w-0">
        <Card tone="white">
          <CardHeader>
            <CardTitle>Jobs</CardTitle>
          </CardHeader>
          <CardContent>
            {jobs.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>No jobs yet</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
            <div className="overflow-x-auto"><Table>
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Log</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {jobs.map((j) => (
                  <TableRow key={j.id}>
                    <TableCell>
                      <span className="truncate">{j.id}</span>
                    </TableCell>
                    <TableCell>
                      <Badge variant={j.status === "done" ? "default" : "secondary"}>
                        {j.status}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <Button size="sm" variant="outline" onClick={() => viewLog(j.id)}>
                        View
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table></div>
            )}
          </CardContent>
        </Card>
        <Card tone="black">
          <CardHeader>
            <CardTitle>Log tail</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="h-64 rounded-md border p-2">
              <ScrollArea className="h-full">
                <pre className="text-xs">{activeLog || "no log selected"}</pre>
              </ScrollArea>
            </div>
          </CardContent>
        </Card>
      </div>
      <Card tone="gray">
        <CardHeader>
          <CardTitle>Daily cron</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-4">
            <code className="rounded-md bg-muted p-2 text-xs">{cron}</code>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="hour">Hour</FieldLabel>
                <Input id="hour" value={hour} onChange={(e) => setHour(e.target.value)} />
              </Field>
              <Field>
                <FieldLabel htmlFor="minute">Minute</FieldLabel>
                <Input id="minute" value={minute} onChange={(e) => setMinute(e.target.value)} />
              </Field>
              <Button onClick={saveCron}>Save schedule</Button>
            </FieldGroup>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
