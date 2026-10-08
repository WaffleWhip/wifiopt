"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { CartesianGrid, Legend, Line, LineChart, XAxis, YAxis } from "recharts";

type Day = {
  date: string;
  n: number;
  cb2: number;
  ca2: number;
  cb5: number;
  ca5: number;
  ch2: number;
  ch5: number;
};

export default function TrendsChart() {
  const [prod, setProd] = useState<Day[]>([]);
  const [ours, setOurs] = useState<Day[]>([]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const r = await fetch("/api/trends?days=14");
      const j = await r.json();
      if (cancelled) return;
      setProd(j.mysql ?? []);
      setOurs(j.ours ?? []);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const cost = prod.map((p) => {
    const o = ours.find((x) => x.date === p.date);
    return {
      date: p.date.slice(4),
      prod: Number(p.ca2?.toFixed(3) ?? 0),
      ours: o ? Number(o.ca2.toFixed(3)) : null,
    };
  });

  const change = prod.map((p) => ({
    date: p.date.slice(4),
    ch2: p.ch2,
    ch5: p.ch5,
  }));

  if (!cost.length) return null;

  return (
    <div className="grid gap-4 xl:grid-cols-2 [&>*]:min-w-0">
      <Card tone="white">
        <CardHeader>
          <CardTitle>Cost after 2G, daily</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer
            config={{
              prod: { label: "prod", color: "var(--chart-1)" },
              ours: { label: "ours", color: "var(--chart-2)" },
            }}
          >
            <LineChart data={cost}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="date" tickLine={false} axisLine={false} />
              <YAxis />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Legend />
              <Line type="monotone" dataKey="prod" stroke="var(--color-prod)" dot={false} />
              <Line type="monotone" dataKey="ours" stroke="var(--color-ours)" dot={false} />
            </LineChart>
          </ChartContainer>
        </CardContent>
      </Card>
      <Card tone="white">
        <CardHeader>
          <CardTitle>Changes per day</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer
            config={{
              ch2: { label: "2g", color: "var(--chart-1)" },
              ch5: { label: "5g", color: "var(--chart-2)" },
            }}
          >
            <LineChart data={change}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="date" tickLine={false} axisLine={false} />
              <YAxis />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Legend />
              <Line type="monotone" dataKey="ch2" stroke="var(--color-ch2)" dot={false} />
              <Line type="monotone" dataKey="ch5" stroke="var(--color-ch5)" dot={false} />
            </LineChart>
          </ChartContainer>
        </CardContent>
      </Card>
    </div>
  );
}
