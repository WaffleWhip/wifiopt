"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

type Row = {
  reg: string;
  ours: { n: number; cb2: number; ca2: number; ch2: number } | null;
  mysql: { n: number; cb2: number; ca2: number; ch2: number } | null;
};

export default function ComparePage() {
  const [date, setDate] = useState("20261007");
  const [rows, setRows] = useState<Row[]>([]);
  const [err, setErr] = useState("");

  async function load() {
    setErr("");
    const r = await fetch(`/api/compare?date=${date}`);
    const j = await r.json();
    if (!r.ok) {
      setErr(j.error ?? "failed");
      return;
    }
    const regs = ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];
    setRows(
      regs.map((reg) => ({
        reg,
        ours: j.ours?.[reg] ?? null,
        mysql: j.mysql?.[reg] ?? null,
      })),
    );
    if (j.mysqlError) setErr(`mysql: ${j.mysqlError}`);
  }

  const chart = rows.map((r) => ({
    reg: r.reg,
    ours: r.ours ? (1 - r.ours.ca2 / Math.max(r.ours.cb2, 1e-9)) * 100 : 0,
    mysql: r.mysql && r.mysql.cb2 ? (1 - r.mysql.ca2 / r.mysql.cb2) * 100 : 0,
  }));

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Compare</CardTitle>
          <CardDescription>ours (CSV) vs wifi_optimize table, cost reduction % per region</CardDescription>
        </CardHeader>
        <CardContent className="flex gap-2">
          <Input value={date} onChange={(e) => setDate(e.target.value)} placeholder="20261007" />
          <Button onClick={load}>Load</Button>
        </CardContent>
      </Card>
      {err && <p className="text-sm text-muted-foreground">{err}</p>}
      <Card>
        <CardHeader>
          <CardTitle>2G cost reduction %</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer config={{ ours: { label: "ours", color: "var(--chart-1)" }, mysql: { label: "mysql", color: "var(--chart-2)" } }}>
            <BarChart data={chart}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="reg" tickLine={false} axisLine={false} />
              <YAxis />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Bar dataKey="ours" fill="var(--color-ours)" radius={4} />
              <Bar dataKey="mysql" fill="var(--color-mysql)" radius={4} />
            </BarChart>
          </ChartContainer>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Details</CardTitle>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Region</TableHead>
                <TableHead>Ours n</TableHead>
                <TableHead>Ours 2G before→after</TableHead>
                <TableHead>MySQL n</TableHead>
                <TableHead>MySQL 2G before→after</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow key={r.reg}>
                  <TableCell>{r.reg}</TableCell>
                  <TableCell>{r.ours?.n.toLocaleString() ?? "-"}</TableCell>
                  <TableCell>
                    {r.ours ? `${r.ours.cb2.toFixed(3)} → ${r.ours.ca2.toFixed(3)}` : "-"}
                  </TableCell>
                  <TableCell>{r.mysql?.n?.toLocaleString() ?? "-"}</TableCell>
                  <TableCell>
                    {r.mysql && r.mysql.cb2 != null
                      ? `${Number(r.mysql.cb2).toFixed(3)} → ${Number(r.mysql.ca2).toFixed(3)}`
                      : "-"}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
