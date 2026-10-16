// Client-side mirrors of the server's JSON API shapes.

export interface ClientSummary {
  id: string;
  name: string;
  url: string;
  up_since: string | null;
  created_at: string;
  last_seen: string | null;
  last_inventory_taken_at: string | null;
}

export interface ApiClientsResponse {
  clients: ClientSummary[];
}

export interface HostInfo {
  hostname: string;
  os: string;
  release: string;
  architecture: string;
  uptime_seconds: number;
  ram_bytes: number;
}

export interface DetectedFs {
  fs_type: string;
  count: number;
  strategy: "snapshot" | "copy" | "unsupported";
}

export interface DetectedTool {
  name: string;
  available: boolean;
  version: string | null;
}

export interface Inventory {
  client_id: string;
  taken_at: string;
  host: HostInfo;
  filesystems: DetectedFs[];
  tools: DetectedTool[];
}

export interface ClientDetail {
  id: string;
  name: string;
  url: string;
  up_since: string | null;
  created_at: string;
  last_seen: string | null;
  last_inventory_taken_at: string | null;
}
