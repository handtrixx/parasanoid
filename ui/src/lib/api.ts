// Thin fetch-based client for the paranoid server API.
// Base URL comes from NEXT_PUBLIC_API_BASE (defaults to /api for same-origin).

const BASE = (process.env.NEXT_PUBLIC_API_BASE ?? "/api").replace(/\/$/, "");

async function json<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { Accept: "application/json" },
    cache: "no-store",
  });
  if (!res.ok) {
    let msg = `request to ${path} failed (${res.status})`;
    try {
      const body = await res.json();
      if (typeof body?.error === "string") msg = body.error;
    } catch {
      /* ignore body parse */
    }
    throw new Error(msg);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export async function fetchClients() {
  return json<{ clients: import("./types").ClientSummary[] }>("/api/discovery/clients");
}

export async function fetchClient(id: string) {
  return json<import("./types").ClientDetail>(`/api/discovery/clients/${encodeURIComponent(id)}`);
}

export async function fetchInventory(id: string) {
  return json<import("./types").Inventory>(
    `/api/discovery/clients/${encodeURIComponent(id)}/inventory`,
  );
}
