"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
  const [history, setHistory] = useState<
    { date: string; reg: string; rows: number | null; bytes: number; mtime: string }[]
  >([]);
  const [activeLog, setActiveLog] = useState("");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refreshJobs() {
    const r = await fetch("/api/runs");
    const j = await r.json();
    setJobs(j.jobs ?? []);
    setHistory(j.runs ?? []);
  }

  async function refreshOptions() {
    try {
      const r = await fetch("/api/options");
      const j = await r.json();
      const list = j.dates ?? [];
      setDates(list);
      if (list.length && !date) {
        setDate(list[0].date);
        setRegs(list[0].regs);
      }
    } catch {
      // keep manual entry
    }
  }

  useEffect(() => {
    // fetch-on-mount: load jobs and available dates once when page opens
    // eslint-disable-next-line react-hooks/set-state-in-effect
    refreshJobs();
    refreshOptions();
  }, []);

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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId]);

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Trigger run</CardTitle>
          <CardDescription>date defaults to today, all regions default</CardDescription>
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
                    <SelectValue placeholder="pick date" />
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
              <ToggleGroup
                value={regs}
                onValueChange={(v) => v.length && setRegs(v as string[])}
                className="flex flex-wrap gap-2"
              >
                {availRegs.map((r) => (
                  <ToggleGroupItem key={r} value={r} aria-label={r}>
                    {r}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
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
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Jobs</CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
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
                    <TableCell className="truncate">{j.id}</TableCell>
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
            </Table>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Log tail</CardTitle>
          </CardHeader>
          <CardContent>
            <ScrollArea className="h-64 rounded-md border p-2">
              <pre className="text-xs">{activeLog || "no log selected"}</pre>
            </ScrollArea>
          </CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>Output history</CardTitle>
          <CardDescription>every CSV on disk, newest first</CardDescription>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Date</TableHead>
                <TableHead>Region</TableHead>
                <TableHead>Rows</TableHead>
                <TableHead>Size</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {history.slice(0, 60).map((h) => (
                <TableRow key={`${h.date}-${h.reg}`}>
                  <TableCell>{h.date}</TableCell>
                  <TableCell>
                    <Badge variant="secondary">{h.reg}</Badge>
                  </TableCell>
                  <TableCell>{h.rows?.toLocaleString() ?? "-"}</TableCell>
                  <TableCell>{(h.bytes / 1024 / 1024).toFixed(1)} MB</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
