import Container from "@mui/material/Container";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { Shell } from "@/components/layout";
import ClientTable from "@/components/ClientTable";
import { fetchClients } from "@/lib/api";
import type { ClientSummary } from "@/lib/types";

export const dynamic = "force-dynamic";

export default async function ClientsPage() {
  const { clients } = await fetchClients();
  return (
    <Shell>
      <Container maxWidth="lg" disableGutters>
        <Box sx={{ mb: 3 }}>
          <Typography variant="h1">Clients</Typography>
          <Typography color="text.secondary">All backup targets reachable by the server.</Typography>
        </Box>
        <ClientTable clients={clients} />
      </Container>
    </Shell>
  );
}
