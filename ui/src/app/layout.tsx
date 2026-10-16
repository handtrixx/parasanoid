import CssBaseline from "@mui/material/CssBaseline";
import type { Metadata } from "next";
import { Inter } from "next/font/google";
import { NextJSLoniMuiProvider } from "@mui/material-nextjs";
import "./globals.css";
import { AppTheme } from "@/components/theme";

export const metadata: Metadata = {
  title: "Paranoid — Backup Manager",
  description: "Centralized ZFS/rsync backup management.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const inter = Inter({
    subsets: ["latin"],
    variable: "--font-inter",
    display: "swap",
  });
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${inter.variable} font-sans`} suppressHydrationWarning>
        <NextJSLoniMuiProvider theme={AppTheme}>
          <CssBaseline />
          {children}
        </NextJSLoniMuiProvider>
      </body>
    </html>
  );
}
