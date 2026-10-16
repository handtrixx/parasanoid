"use client";

import { createTheme } from "@mui/material/styles";

// Dark-only console theme, mirroring weboid's dark data-grid aesthetic.
export const AppTheme = createTheme({
  cssVariables: {
    colorSchemeSelector: "data-mui-color-scheme",
  },
  palette: {
    mode: "dark",
    background: {
      default: "#0d1117",
      paper: "#161b22",
    },
    primary: { main: "#58a6ff" },
    success: { main: "#3fb950" },
    warning: { main: "#d29922" },
    error: { main: "#f85149" },
    info: { main: "#58a6ff" },
  },
  typography: {
    fontFamily: "var(--font-inter), system-ui, sans-serif",
    h1: { fontSize: "1.75rem", fontWeight: 600 },
    h2: { fontSize: "1.25rem", fontWeight: 600 },
    body2: { fontSize: "0.875rem" },
  },
  shape: { borderRadius: 8 },
  components: {
    MuiCard: {
      styleOverrides: {
        root: { backgroundImage: "none", borderColor: "#21262d", borderWidth: 1 },
      },
    },
    MuiTableRow: {
      styleOverrides: {
        root: { "&:last-child td": { borderBottom: 0 } },
      },
    },
  },
});
