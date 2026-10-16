"use client";

import Box from "@mui/material/Box";
import Toolbar from "@mui/material/Toolbar";
import Divider from "@mui/material/Divider";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemText from "@mui/material/ListItemText";
import Typography from "@mui/material/Typography";
import Chip from "@mui/material/Chip";
import AppBar from "@mui/material/AppBar";
import Link as MuiLink from "@mui/material/Link";
import StorageIcon from "@mui/icons-material/Storage";
import { usePathname } from "next/navigation";

const nav = [
  { label: "Dashboard", href: "/" },
  { label: "Clients", href: "/clients" },
];

export function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  return (
    <Box sx={{ display: "flex", minHeight: "100vh" }}>
      <AppBar position="fixed" elevation={0} sx={{ width: "var(--nav-w)", zIndex: (t) => t.zIndex.drawer }}>
        <Toolbar variant="dense">
          <StorageIcon />
          <Typography variant="h6" sx={{ ml: 1, fontWeight: 700, letterSpacing: 0.5 }}>
            PARANOID
          </Typography>
        </Toolbar>
        <Divider />
        <List>
          {nav.map((item) => (
            <ListItem key={item.href} disablePadding>
              <ListItemButton
                selected={pathname === item.href || (item.href !== "/" && pathname?.startsWith(item.href))}
                component={MuiLink}
                href={item.href}
              >
                <ListItemText primary={item.label} />
              </ListItemButton>
            </ListItem>
          ))}
        </List>
      </AppBar>
      <Box
        component="main"
        sx={{
          flexGrow: 1,
          width: { xs: `calc(100% - var(--nav-w))`, md: `calc(100% - var(--nav-w))` },
          bgcolor: "background.default",
          minHeight: "100vh",
        }}
      >
        <Toolbar />
        <Box sx={{ p: 3 }}>{children}</Box>
      </Box>
    </Box>
  );
}
