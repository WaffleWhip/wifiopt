"use client";

import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";

type H = { date: string; reg: string; rows: number | null; bytes: number };

export default function HistoryPage() {
  const [history, setHistory] = useState<H[]>([]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const r = await fetch("/api/runs");
      const j = await r.json();
      if (cancelled) return;
      setHistory(j.runs ?? []);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="h-full min-h-0 flex flex-col gap-4 overflow-y-auto">
      <Card tone="white" className="flex-1 min-h-0 flex flex-col">
        <CardHeader>
          <CardTitle>Output history</CardTitle>
        </CardHeader>
        <CardContent className="flex-1 min-h-0">
          <ScrollArea className="h-full">
          <Accordion>
            {[...new Map(history.map((h) => [h.date, h])).keys()].map((date) => {
              const regs = history.filter((h) => h.date === date);
              const total = regs.reduce((s, h) => s + (h.rows ?? 0), 0);
              return (
                <AccordionItem key={date} value={date}>
                  <AccordionTrigger>
                    {date} — {total.toLocaleString()} rows
                  </AccordionTrigger>
                  <AccordionContent>
                    <div className="overflow-x-auto"><Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Region</TableHead>
                          <TableHead>Rows</TableHead>
                          <TableHead>Size</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {regs.map((h) => (
                          <TableRow key={h.reg}>
                            <TableCell>
                              <Badge variant="secondary">{h.reg}</Badge>
                            </TableCell>
                            <TableCell>{h.rows?.toLocaleString() ?? "-"}</TableCell>
                            <TableCell>{(h.bytes / 1024 / 1024).toFixed(1)} MB</TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table></div>
                  </AccordionContent>
                </AccordionItem>
              );
            })}
          </Accordion>
          </ScrollArea>
        </CardContent>
      </Card>
    </div>
  );
}
