"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";

type Agg = { n: number; cb2: number; ca2: number; ch2: number };

export default function CostChart({ date }: { date: string }) {
  const [rows, setRows] = useState<{ reg: string; before: number; after: number }[]>([]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const r = await fetch(`/api/compare?date=${date}`);
      const j = await r.json();
      if (cancelled) return;
      const regs = ["reg1", "reg2", "reg3", "reg4", "reg5", "reg6", "reg7"];
      setRows(
        regs
          .map((reg) => {
            const o = j.ours?.[reg] as Agg | null;
            if (!o) return null;
            return { reg, before: Number(o.cb2.toFixed(3)), after: Number(o.ca2.toFixed(3)) };
          })
          .filter((x): x is { reg: string; before: number; after: number } => x !== null),
      );
    })();
    return () => {
      cancelled = true;
    };
  }, [date]);

  if (!rows.length) return null;

  return (
    <Card tone="grey">
      <CardHeader>
        <CardTitle>2G cost before vs after</CardTitle>
      </CardHeader>
      <CardContent>
        <ChartContainer
          config={{
            before: { label: "before", color: "var(--chart-1)" },
            after: { label: "after", color: "var(--chart-2)" },
          }}
        >
          <BarChart data={rows}>
            <CartesianGrid vertical={false} />
            <XAxis dataKey="reg" tickLine={false} axisLine={false} />
            <YAxis />
            <ChartTooltip content={<ChartTooltipContent />} />
            <Bar dataKey="before" fill="var(--color-before)" radius={4} />
            <Bar dataKey="after" fill="var(--color-after)" radius={4} />
          </BarChart>
        </ChartContainer>
      </CardContent>
    </Card>
  );
}
