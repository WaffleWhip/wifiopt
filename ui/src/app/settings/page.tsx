"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";

export default function SettingsPage() {
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);
  const [db, setDb] = useState({ host: "", port: "", user: "", password: "", database: "" });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const h = await fetch("/api/health").then((r) => r.json());
      if (cancelled) return;
      setHealth(h);
      const s = await fetch("/api/settings").then((r) => r.json());
      if (cancelled) return;
      setDb({ host: s.host ?? "", port: s.port ?? "", user: s.user ?? "", password: "", database: s.database ?? "" });
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function saveDb() {
    try {
      const r = await fetch("/api/settings", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(db),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error ?? "save failed");
      toast.success("mysql saved, service restarting...");
      setTimeout(async () => {
        try {
          const s = await fetch("/api/settings").then((r) => r.json());
          setDb({ host: s.host ?? "", port: s.port ?? "", user: s.user ?? "", password: "", database: s.database ?? "" });
        } catch {
          // service still restarting
        }
      }, 8000);
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  return (
    <div className="h-full min-h-0 flex flex-col gap-4 overflow-y-auto">
      <Card tone="white">
        <CardHeader>
          <CardTitle>MySQL pointing</CardTitle>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="dbhost">IP / host</FieldLabel>
              <Input id="dbhost" value={db.host} onChange={(e) => setDb({ ...db, host: e.target.value })} />
            </Field>
            <Field>
              <FieldLabel htmlFor="dbport">Port</FieldLabel>
              <Input id="dbport" value={db.port} onChange={(e) => setDb({ ...db, port: e.target.value })} />
            </Field>
            <Field>
              <FieldLabel htmlFor="dbuser">User</FieldLabel>
              <Input id="dbuser" value={db.user} onChange={(e) => setDb({ ...db, user: e.target.value })} />
            </Field>
            <Field>
              <FieldLabel htmlFor="dbpass">Password</FieldLabel>
              <Input
                id="dbpass"
                type="password"
                value={db.password}
                onChange={(e) => setDb({ ...db, password: e.target.value })}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="dbdb">Database</FieldLabel>
              <Input id="dbdb" value={db.database} onChange={(e) => setDb({ ...db, database: e.target.value })} />
            </Field>
            <Button onClick={saveDb}>Save MySQL</Button>
          </FieldGroup>
        </CardContent>
      </Card>
      <Card tone="gray">
        <CardHeader>
          <CardTitle>Health</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-2">
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
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
