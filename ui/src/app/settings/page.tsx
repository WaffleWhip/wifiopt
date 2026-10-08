"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";

export default function SettingsPage() {
  const [cron, setCron] = useState("");
  const [hour, setHour] = useState("5");
  const [minute, setMinute] = useState("30");
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);

  async function refresh() {
    const c = await fetch("/api/cron").then((r) => r.json());
    setCron(c.line ?? "none");
    const h = await fetch("/api/health").then((r) => r.json());
    setHealth(h);
  }

  useEffect(() => {
    // fetch-on-mount: load cron and health once when page opens
    // eslint-disable-next-line react-hooks/set-state-in-effect
    refresh();
  }, []);

  async function save() {
    try {
      const r = await fetch("/api/cron", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hour: Number(hour), minute: Number(minute) }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error ?? "save failed");
      toast.success("cron updated");
      refresh();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Daily cron</CardTitle>
          <CardDescription>current wifiopt schedule line</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <code className="rounded-md bg-muted p-2 text-xs">{cron}</code>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="hour">Hour (0-23 WIB server time)</FieldLabel>
              <Input id="hour" value={hour} onChange={(e) => setHour(e.target.value)} />
            </Field>
            <Field>
              <FieldLabel htmlFor="minute">Minute (0-59)</FieldLabel>
              <Input id="minute" value={minute} onChange={(e) => setMinute(e.target.value)} />
            </Field>
            <Button onClick={save}>Save schedule</Button>
          </FieldGroup>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Health</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          <div className="flex gap-2">
            <Badge variant={health?.bin ? "default" : "secondary"}>
              binary {health?.bin ? "ok" : "missing"}
            </Badge>
            <Badge variant={health?.out ? "default" : "secondary"}>
              out dir {health?.out ? "ok" : "missing"}
            </Badge>
            <Badge variant={health?.db === "ok" ? "default" : "secondary"}>
              db {String(health?.db ?? "?")}
            </Badge>
          </div>
          <code className="rounded-md bg-muted p-2 text-xs">{JSON.stringify(health)}</code>
        </CardContent>
      </Card>
    </div>
  );
}
