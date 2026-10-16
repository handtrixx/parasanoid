"use client";

import Box from "@mui/material/Box";
import Card from "@mui/material/Card";
import CardContent from "@mui/material/CardContent";
import Table from "@mui/material/Table";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import TableBody from "@mui/material/TableBody";
import Chip from "@mui/material/Chip";
import Link from "next/link";
import { relativeTime } from "@/lib/format";
import type { ClientSummary } from "@/lib/types";

export default function ClientTable({ clients }: { clients: ClientSummary[] }) {
  if (clients.length === 0) {
    return (
      <Card variant="outlined">
        <CardContent>
          <Typography color="text.secondary">
            No clients discovered yet. Deploy a client via
            <code style={{ mx: 4 }}>docker compose -f docker/client.compose.yaml up -d</code>
            and it will appear here once the server reaches out to it.
          </Typography>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card variant="outlined">
      <CardContent sx={{ p: 0 }}>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell>Name</TableCell>
              <TableCell>URL</TableCell>
              <TableCell>Healthy</TableCell>
              <TableCell>Inventory</TableCell>
              <TableCell>Last seen</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {clients.map((c) => (
              <TableRow key={c.id} hover component={Link} href={`/clients/${encodeURIComponent(c.id)}`}>
                <TableCell sx={{ fontWeight: 600 }}>{c.name}</TableCell>
                <TableCell>{c.url}</TableCell>
                <TableCell>
                  <Chip
                    size="small"
                    color={hasSeen(c.last_seen) ? "success" : "warning"}
                    label={hasSeen(c.last_seen) ? "reachable" : "unreachable"}
                  />
                </TableCell>
                <TableCell>
                  {c.last_inventory_taken_at ? relativeTime(c.last_inventory_taken_at) : "—"}
                </TableCell>
                <TableCell>{hasSeen(c.last_seen) ? relativeTime(c.last_seen) : "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

function hasSeen(lastSeen: string | null): boolean {
  if (!lastSeen) return false;
  const diff = (Date.now() - new Date(lastSeen).getTime()) / 1000;
  return diff < 60;
}
