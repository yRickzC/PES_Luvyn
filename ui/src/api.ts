export type Location = {
  file: string;
  line: number;
  column: number;
  length: number;
};
export type Symbol = {
  id: string;
  name: string;
  qualified: string;
  kind: string;
  parent: string | null;
  location: Location;
  end_line: number;
  sections?: Record<string, string[]>;
  signature?: string;
};
export type Range = {
  startLineNumber: number;
  startColumn: number;
  endLineNumber: number;
  endColumn: number;
};
export type Edit = { file: string; range: Range; text: string };
export type Diagnostic = {
  diagnostic: {
    severity: string;
    code: string;
    message: string;
    location: Location;
    suggestion?: string;
  };
  range: Range;
};
export type Snapshot = {
  workspace: string;
  files: { path: string; hash: string | null }[];
  folders: string[];
  diagnostics: Diagnostic[];
  symbols: Symbol[];
  symbol_count: number;
  edge_count: number;
  revision: number;
  autosave: boolean;
  stats: { parsed: number; reused: number };
};
export type GraphResult = {
  symbols: Symbol[];
  edges: { from: string; to: string; relation: string }[];
  truncated: boolean;
};
let token =
  new URLSearchParams(location.hash.slice(1)).get("token") ||
  sessionStorage.getItem("luvyn-token") ||
  "";
if (token) {
  sessionStorage.setItem("luvyn-token", token);
  history.replaceState(null, "", location.pathname);
}
export async function api<T = any>(
  op: string,
  data: Record<string, unknown> = {},
): Promise<T> {
  const response = await fetch("/api/action", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Luvyn-Token": token },
    body: JSON.stringify({ op, ...data }),
  });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || `HTTP ${response.status}`);
  return value;
}
