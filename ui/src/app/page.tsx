import { Suspense } from "react";
import { connection } from "next/server";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { listJobs, listRuns, readSummary } from "@/lib/wifiopt";

async function DashboardData() {
  await connection();
  const runs = listRuns();
  const summary = readSummary();
  const jobs = listJobs();
  const latest = runs[0]?.date ?? summary?.date ?? "-";
  const totalRows = runs.filter((r) => r.date === latest).reduce((s, r) => s + (r.rows ?? 0), 0);

  return (
    <>
      <div className="grid gap-4 sm:grid-cols-3">
        <Card>
          <CardHeader>
            <CardTitle>Latest date</CardTitle>
          </CardHeader>
          <CardContent className="text-2xl font-semibold">{latest}</CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Total rows</CardTitle>
          </CardHeader>
          <CardContent className="text-2xl font-semibold">{totalRows.toLocaleString()}</CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Jobs</CardTitle>
          </CardHeader>
          <CardContent className="text-2xl font-semibold">{jobs.length}</CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>Latest run per region</CardTitle>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Region</TableHead>
                <TableHead>Rows</TableHead>
                <TableHead>Size</TableHead>
                <TableHead>Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {runs
                .filter((r) => r.date === latest)
                .map((r) => (
                  <TableRow key={r.reg}>
                    <TableCell>
                      <Badge variant="secondary">{r.reg}</Badge>
                    </TableCell>
                    <TableCell>{r.rows?.toLocaleString() ?? "-"}</TableCell>
                    <TableCell>{(r.bytes / 1024 / 1024).toFixed(1)} MB</TableCell>
                    <TableCell>{new Date(r.mtime).toLocaleString()}</TableCell>
                  </TableRow>
                ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </>
  );
}

export default function Dashboard() {
  return (
    <div className="flex flex-col gap-4">
      <Suspense fallback={<p className="text-sm text-muted-foreground">Loading...</p>}>
        <DashboardData />
      </Suspense>
    </div>
  );
}
