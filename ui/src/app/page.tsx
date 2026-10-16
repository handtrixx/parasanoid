import Box from "@mui/material/Box";
import Grid from "@mui/material/Grid";
import Card from "@mui/material/Card";
import CardContent from "@mui/material/CardContent";
import Typography from "@mui/material/Typography";
import Container from "@mui/material/Container";
import { Shell } from "@/components/layout";
import ClientTable from "@/components/ClientTable";
import { fetchClients } from "@/lib/api";
import type { ClientSummary } from "@/lib/types";

export const dynamic = "force-dynamic";

export default async function Dashboard() {
  let clients: ClientSummary[] = [];
  try {
    clients = (await fetchClients()).clients;
  } catch (e) {
    // Server unreachable (e.g. not deployed yet) — surface the error.
    return (
      <Shell>
        <Container maxWidth="md">
          <Typography color="error">
            Could not reach the paranoid server. Is it running? {e instanceof Error ? e.message : ""}
          </Typography>
        </Container>
      </Shell>
    );
  }

  const reachable = clients.filter((c) => c.last_seen && Date.now() - new Date(c.last_seen).getTime() < 60_000).length;
  const stats = [
    { label: "Clients", value: clients.length },
    { label: "Reachable", value: reachable },
    { label: "Unreachable", value: clients.length - reachable },
  ];

  return (
    <Shell>
      <Container maxWidth="lg" disableGutters>
        <Box sx={{ mb: 3 }}>
          <Typography variant="h1">Dashboard</Typography>
          <Typography color="text.secondary">Overview of discovered backup targets.</Typography>
        </Box>
        <Grid container spacing={3} sx={{ mb: 1 }}>
          {stats.map((s) => (
            <Grid item xs={12} sm={4} key={s.label}>
              <Card variant="outlined">
                <CardContent>
                  <Typography color="text.secondary" gutterBottom>{s.label}</Typography>
                  <Typography variant="h1">{s.value}</Typography>
                </CardContent>
              </Card>
            </Grid>
          ))}
        </Grid>
        <ClientTable clients={clients} />
      </Container>
    </Shell>
  );
}
