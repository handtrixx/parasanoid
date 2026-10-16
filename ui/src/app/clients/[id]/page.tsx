import Box from "@mui/material/Box";
import Card from "@mui/material/Card";
import CardContent from "@mui/material/CardContent";
import Typography from "@mui/material/Typography";
import Container from "@mui/material/Container";
import Link from "next/link";
import BackIcon from "@mui/icons-material/ArrowBack";
import { Shell } from "@/components/layout";
import InventoryCard from "@/components/InventoryCard";
import { fetchClient, fetchInventory } from "@/lib/api";
import type { Inventory } from "@/lib/types";

export const dynamic = "force-dynamic";

export default async function ClientDetail({ params }: { params: { id: string } }) {
  const id = decodeURIComponent(params.id);
  const [client, inv] = await Promise.all([fetchClient(id), fetchInventory(id).catch(() => null)]);

  return (
    <Shell>
      <Container maxWidth="lg" disableGutters>
        <Box sx={{ mb: 3 }}>
          <Link href="/clients" sx={{ display: "inline-flex", alignItems: "center" }}>
            <BackIcon fontSize="small" /> Clients
          </Link>
          <Typography variant="h1" sx={{ mt: 1 }}>{client.name}</Typography>
          <Typography color="text.secondary">{client.url}</Typography>
        </Box>

        {!inv ? (
          <Card variant="outlined">
            <CardContent>
              <Typography color="text.secondary">
                No inventory yet — the server has not successfully reached this client.
              </Typography>
            </CardContent>
          </Card>
        ) : (
          <InventoryCard inv={inv} />
        )}
      </Container>
    </Shell>
  );
}
