"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
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

export default function TrendsPage() {
  const [days, setDays] = useState("14");
  const [prod, setProd] = useState<Day[]>([]);
  const [ours, setOurs] = useState<Day[]>([]);

  async function load() {
    const r = await fetch(`/api/trends?days=${days}`);
    const j = await r.json();
    setProd(j.mysql ?? []);
    setOurs(j.ours ?? []);
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/set-state-in-effect
  }, []);

  const cost = prod.map((p) => {
    const o = ours.find((x) => x.date === p.date);
    return {
      date: p.date.slice(4),
      prodAfter2g: Number(p.ca2?.toFixed(3) ?? 0),
      oursAfter2g: o ? Number(o.ca2.toFixed(3)) : null,
      prodAfter5g: Number(p.ca5?.toFixed(4) ?? 0),
    };
  });

  const change = prod.map((p) => ({
    date: p.date.slice(4),
    ch2: p.ch2,
    ch5: p.ch5,
    n: p.n,
  }));

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Day to day</CardTitle>
        </CardHeader>
        <CardContent className="flex gap-2">
          {["7", "14", "30"].map((d) => (
            <Button key={d} variant={days === d ? "default" : "outline"} onClick={() => setDays(d)}>
              {d}d
            </Button>
          ))}
          <Button variant="secondary" onClick={load}>
            Reload
          </Button>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Avg cost after, 2G</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer
            config={{
              prodAfter2g: { label: "prod", color: "var(--chart-1)" },
              oursAfter2g: { label: "ours", color: "var(--chart-2)" },
            }}
          >
            <LineChart data={cost}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="date" tickLine={false} axisLine={false} />
              <YAxis />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Legend />
              <Line type="monotone" dataKey="prodAfter2g" stroke="var(--color-prodAfter2g)" dot={false} />
              <Line type="monotone" dataKey="oursAfter2g" stroke="var(--color-oursAfter2g)" dot={false} />
            </LineChart>
          </ChartContainer>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Changes per day</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer
            config={{
              ch2: { label: "2g change", color: "var(--chart-1)" },
              ch5: { label: "5g change", color: "var(--chart-2)" },
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
