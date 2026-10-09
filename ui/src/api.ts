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
  namespace: string;
  kind: string;
  parent: string | null;
  location: Location;
  end_line: number;
  sections?: Record<string, string[]>;
  signature?: string;
  annotations: { name: string; arguments: string[]; location: Location }[];
  generics: { name: string; bound: string | null; location: Location }[];
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
  source_roots: string[];
  documentation_root: string;
  target_project_root: string;
  artifact_root: string;
  files: { path: string; hash: string | null }[];
  configs: { path: string; hash: string | null }[];

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
const IDE_STORAGE_SCHEMA = 2;

/** Keep durable editor preferences; discard only Luvyn's disposable caches. */
export function migrateIdeStorage() {
  try {
    const version = Number(
      localStorage.getItem("luvyn-ide-storage-schema") || 0,
    );
    if (version === IDE_STORAGE_SCHEMA) return;
    for (const key of Object.keys(localStorage)) {
      if (key.startsWith("luvyn-cache:")) localStorage.removeItem(key);
    }
    if (!["dark", "light"].includes(localStorage.getItem("luvyn-theme") || ""))
      localStorage.removeItem("luvyn-theme");
    if (
      !["true", "false"].includes(localStorage.getItem("luvyn-autosave") || "")
    )
      localStorage.removeItem("luvyn-autosave");
    localStorage.setItem(
      "luvyn-ide-storage-schema",
      String(IDE_STORAGE_SCHEMA),
    );
  } catch {
    // Browser storage can be disabled. The IDE remains usable without its cache.
  }
}
migrateIdeStorage();
const launchOptions = new URLSearchParams(location.hash.slice(1));
export const startInProjects = launchOptions.get("view") === "projects";
if (launchOptions.has("token")) {
  launchOptions.delete("token");
  const remaining = launchOptions.toString();
  history.replaceState(
    null,
    "",
    `${location.pathname}${location.search}${remaining ? `#${remaining}` : ""}`,
  );
}
try {
  sessionStorage.removeItem("luvyn-token");
} catch {
  // Older session credentials are no longer used by the local IDE API.
}
export async function api<T = any>(
  op: string,
  data: Record<string, unknown> = {},
): Promise<T> {
  if ((window as any).LuvynNative) {
    return nativeRequest({ op, ...data }) as Promise<T>;
  }
  let response: Response;
  try {
    response = await fetch("/api/action", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ op, ...data }),
    });
  } catch {
    throw new ApiConnectionError();
  }
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || `HTTP ${response.status}`);
  return value;
}

export class ApiConnectionError extends Error {
  constructor() {
    super(
      "A IDE perdeu a conexão com o servidor local. Mantenha o terminal aberto. Se reiniciou o servidor, abra o novo endereço mostrado no terminal.",
    );
    this.name = "ConnectionError";
  }
}

/** Reserve the login tab during the click, before awaiting the backend. */
export async function connectGoogleDrive(inBrowser: boolean) {
  const loginTab = inBrowser ? window.open("about:blank", "_blank") : null;
  if (inBrowser && !loginTab)
    throw new Error(
      "Permita abrir uma aba neste navegador para conectar o Google Drive.",
    );
  try {
    const result = await api("drive-connect");
    if (loginTab) {
      if (!result.authorization_url)
        throw new Error(
          "O servidor não retornou o endereço de login do Google.",
        );
      loginTab.opener = null;
      loginTab.location.replace(result.authorization_url);
    }
    return result;
  } catch (error) {
    loginTab?.close();
    throw error;
  }
}

const pending = new Map<
  string,
  {
    resolve: (value: any) => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  }
>();
let requestId = 0;
function nativeRequest(request: Record<string, unknown>): Promise<any> {
  return new Promise((resolve, reject) => {
    const id = String(++requestId);
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("Android request timed out; reopen workspace"));
    }, 120000);
    pending.set(id, { resolve, reject, timer });
    (window as any).LuvynNative.request(id, JSON.stringify(request));
  });
}
(window as any).luvynReply = (id: string, json: string) => {
  const value = JSON.parse(json);
  if (id === "event") {
    window.dispatchEvent(new CustomEvent("luvyn-workspace", { detail: value }));
    return;
  }
  const task = pending.get(id);
  if (!task) return;
  pending.delete(id);
  clearTimeout(task.timer);
  if (value.error) task.reject(new Error(value.error));
  else task.resolve(value);
};
