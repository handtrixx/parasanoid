"use client";

import Grid from "@mui/material/Grid";
import Card from "@mui/material/Card";
import CardContent from "@mui/material/CardContent";
import CardHeader from "@mui/material/CardHeader";
import Divider from "@mui/material/Divider";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import Chip from "@mui/material/Chip";
import Typography from "@mui/material/Typography";
import { formatBytes, uptime, strategyLabel } from "@/lib/format";
import type { Inventory } from "@/lib/types";

export default function InventoryCard({ inv }: { inv: Inventory }) {
  const h = inv.host;
  return (
    <Grid container spacing={3}>
      <Grid item xs={12} md={6}>
        <Card variant="outlined">
          <CardHeader title="Host" titleTypographyProps={{ variant: "h2" }} />
          <Divider />
          <CardContent>
            <Table size="small">
              <TableBody>
                <TableRow><TableCell>hostname</TableCell><TableCell>{h.hostname}</TableCell></TableRow>
                <TableRow><TableCell>OS</TableCell><TableCell>{h.os}</TableCell></TableRow>
                <TableRow><TableCell>release</TableCell><TableCell>{h.release}</TableCell></TableRow>
                <TableRow><TableCell>arch</TableCell><TableCell>{h.architecture}</TableCell></TableRow>
                <TableRow><TableCell>uptime</TableCell><TableCell>{uptime(h.uptime_seconds)}</TableCell></TableRow>
                <TableRow><TableCell>memory</TableCell><TableCell>{formatBytes(h.ram_bytes)}</TableCell></TableRow>
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </Grid>
      <Grid item xs={12} md={6}>
        <Card variant="outlined">
          <CardHeader title="Tools" titleTypographyProps={{ variant: "h2" }} />
          <Divider />
          <CardContent>
            <Table size="small">
              <TableBody>
                {inv.tools.map((t) => (
                  <TableRow key={t.name}>
                    <TableCell sx={{ fontWeight: 600 }}>{t.name}</TableCell>
                    <TableCell>
                      <Chip
                        size="small"
                        color={t.available ? "success" : "default"}
                        label={t.available ? t.version ?? "available" : "missing"}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </Grid>
      <Grid item xs={12}>
        <Card variant="outlined">
          <CardHeader title="Filesystems" titleTypographyProps={{ variant: "h2" }} />
          <Divider />
          <CardContent>
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell>Type</TableCell>
                  <TableCell>Count</TableCell>
                  <TableCell>Strategy</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {inv.filesystems.map((f) => (
                  <TableRow key={f.fs_type}>
                    <TableCell sx={{ fontWeight: 600 }}>{f.fs_type}</TableCell>
                    <TableCell>{f.count}</TableCell>
                    <TableCell>{strategyLabel(f.strategy)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </Grid>
    </Grid>
  );
}
