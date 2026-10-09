import { useState, useEffect, useRef, type ReactNode } from "react";
import {
  FolderOpen,
  Cloud,
  Plus,
  Trash2,
  RefreshCw,
  ArrowLeft,
  Folder,
} from "lucide-react";
import {
  api,
  startInProjects,
  ApiConnectionError,
  connectGoogleDrive,
} from "./api";
import "./projects.css";
type Recent = {
  id: string;
  name: string;
  path: string;
  kind: string;
  last_access: number;
  cloud_id?: string;
  unavailable: boolean;
};
type State = {
  host?: "server" | "desktop";
  current: string | null;
  recent: Recent[];
  cloud: { connected: boolean; configured: boolean; authorization: string };
};
type ProjectRow = {
  key: string;
  id?: string;
  cloudId?: string;
  name: string;
  path: string;
  kind: "local" | "cloud";
  lastAccess: number;
  unavailable: boolean;
};
export function ProjectsGate({
  mobile = false,
  children,
}: {
  mobile?: boolean;
  children: (
    showProjects: () => void,
    syncCloud: () => Promise<void>,
    currentIsCloud: boolean,
  ) => ReactNode;
}) {
  const [screen, setScreen] = useState(mobile || startInProjects),
    [loaded, setLoaded] = useState(false),
    [state, setState] = useState<State | null>(null),
    [cloud, setCloud] = useState<{ id: string; name: string }[]>([]);
  const [search, setSearch] = useState(""),
    [busy, setBusy] = useState(""),
    [error, setError] = useState(""),
    [dialog, setDialog] = useState(""),
    [path, setPath] = useState("");
  const [connectionError, setConnectionError] = useState("");
  const connectionLost = useRef(false);
  const native = !!(window as any).LuvynNative;
  const viewRevision = useRef(0);
  useEffect(() => {
    if (native) return;
    const timer = setInterval(
      () =>
        void api<{ revision: number }>("project-view")
          .then(async (data) => {
            if (connectionLost.current) {
              const recovered = await api<State>("projects");
              setState(recovered);
              connectionLost.current = false;
              setConnectionError("");
            }
            if (data.revision > viewRevision.current) {
              viewRevision.current = data.revision;
              window.dispatchEvent(new Event("luvyn-projects-request"));
            }
          })
          .catch((e) => {
            if (e instanceof ApiConnectionError) {
              connectionLost.current = true;
              setConnectionError(e.message);
            }
          }),
      2000,
    );
    return () => clearInterval(timer);
  }, [native]);
  const loadCloud = async () => {
    const data = await api("drive-projects");
    setCloud(data.projects);
  };
  const refresh = async () => {
    const data = await api<State>("projects");
    setState(data);
    connectionLost.current = false;
    setConnectionError("");
    return data;
  };
  useEffect(() => {
    let active = true;
    api<State>("projects")
      .then((data) => {
        if (active) {
          setState(data);
          setScreen(mobile || startInProjects || !data.current);
          setLoaded(true);
        }
      })
      .catch((e) => {
        if (active) {
          if (e instanceof ApiConnectionError) {
            connectionLost.current = true;
            setConnectionError(e.message);
          } else setError(String(e));
          setScreen(true);
          setLoaded(true);
        }
      });
    const changed = (event: Event) => {
      if ((event as CustomEvent).detail.workspaceChanged) {
        setScreen(false);
        void refresh();
      } else {
        void refresh();
      }
    };
    window.addEventListener("luvyn-workspace", changed);
    return () => {
      active = false;
      window.removeEventListener("luvyn-workspace", changed);
    };
  }, []);
  useEffect(() => {
    if (!screen || !state?.cloud.connected) return;
    void loadCloud().catch((e) => setError(String(e)));
  }, [screen, state?.cloud.connected]);
  useEffect(() => {
    if (state?.cloud.authorization !== "pending") return;
    const timer = setInterval(
      () => void refresh().catch((e) => setError(String(e))),
      1500,
    );
    return () => clearInterval(timer);
  }, [state?.cloud.authorization]);
  const run = async (label: string, action: () => Promise<void>) => {
    if (busy) return;
    setBusy(label);
    setError("");
    try {
      await action();
    } catch (e) {
      if (e instanceof ApiConnectionError) {
        connectionLost.current = true;
        setConnectionError(e.message);
      } else setError(String(e));
    } finally {
      setBusy("");
    }
  };
  const open = async (op: string, data: Record<string, unknown>) => {
    const result = await api(op, data);
    if (result.conflicts?.length)
      throw new Error(
        `Conflitos: ${result.conflicts.join(", ")}. Cache preservado: ${result.cache}`,
      );
    setDialog("");
    if (!result.pending) {
      setState(result);
      setScreen(false);
    }
  };
  const currentIsCloud = !!state?.recent.some(
    (project) => project.kind === "cloud" && project.path === state.current,
  );
  const syncCloud = async () => {
    if (!currentIsCloud) throw new Error("O workspace atual não é Cloud.");
    if (busy) throw new Error("Sincronização já em andamento.");
    setBusy("Sincronizando");
    setError("");
    try {
      const data = await api("project-sync");
      if (data.report.conflicts.length)
        throw new Error(
          `Conflitos: ${data.report.conflicts.join(", ")}. Nenhum arquivo sobrescrito.`,
        );
      window.dispatchEvent(new Event("luvyn-sync"));
    } catch (error) {
      setError(String(error));
      throw error;
    } finally {
      setBusy("");
    }
  };
  const projectRows: ProjectRow[] = (() => {
    const recentByCloud = new Map(
      (state?.recent || [])
        .filter((project) => project.kind === "cloud" && project.cloud_id)
        .map((project) => [project.cloud_id!, project]),
    );
    const seenCloud = new Set<string>();
    const rows: ProjectRow[] = (state?.recent || [])
      .filter((project) => project.kind === "local")
      .map((project) => ({
        key: `local:${project.id}`,
        id: project.id,
        name: project.name,
        path: project.path,
        kind: "local",
        lastAccess: project.last_access,
        unavailable: project.unavailable,
      }));
    for (const project of cloud) {
      seenCloud.add(project.id);
      const recent = recentByCloud.get(project.id);
      rows.push({
        key: `cloud:${project.id}`,
        id: recent?.id,
        cloudId: project.id,
        name: project.name,
        path: "Google Drive",
        kind: "cloud",
        lastAccess: recent?.last_access || 0,
        unavailable: false,
      });
    }
    for (const [cloudId, recent] of recentByCloud) {
      if (seenCloud.has(cloudId)) continue;
      rows.push({
        key: `cloud:${cloudId}`,
        id: recent.id,
        cloudId,
        name: recent.name,
        path: "Google Drive",
        kind: "cloud",
        lastAccess: recent.last_access,
        unavailable: false,
      });
    }
    return rows.sort(
      (a, b) => b.lastAccess - a.lastAccess || a.name.localeCompare(b.name),
    );
  })();
  const filtered = (name: string, path = "") =>
    `${name} ${path}`.toLowerCase().includes(search.toLowerCase());
  if (!loaded)
    return <div className="projects-loading">Luvyn · Carregando projetos…</div>;
  if (!screen)
    return (
      <div
        className={mobile ? "projects-mobile-host" : "projects-desktop-host"}
      >
        {children(
          () => {
            setScreen(true);
            void refresh();
          },
          syncCloud,
          currentIsCloud,
        )}
      </div>
    );
  return (
    <main className={`projects-screen ${mobile ? "projects-mobile" : ""}`}>
      <div className="projects-shell">
        <header>
          <div className="projects-brand">
            L<span>UVYN</span>
          </div>
          <div className="projects-heading">
            <h1>Projects</h1>
            <p>
              Documentação estruturada. Um editor para todos os seus projetos.
            </p>
          </div>
        </header>
        <div className="projects-toolbar">
          <button
            className="projects-primary"
            disabled={!!busy}
            onClick={() => {
              setDialog("create");
              setPath("");
            }}
          >
            <Plus size={18} />
            Novo projeto
          </button>
          <button
            disabled={!!busy}
            onClick={() =>
              native
                ? void run("Abrindo", async () => {
                    await api("open-docs");
                  })
                : (setDialog("open"), setPath(""))
            }
          >
            <FolderOpen size={18} />
            Abrir local
          </button>
          {state?.cloud.connected ? (
            <>
              <button
                disabled={!!busy}
                onClick={() => void run("Atualizando Drive", loadCloud)}
              >
                <RefreshCw size={16} /> Drive
              </button>
              <button
                disabled={!!busy}
                onClick={() => {
                  setDialog("cloud");
                  setPath("");
                }}
              >
                <Cloud size={17} /> Novo Cloud
              </button>
            </>
          ) : (
            <button
              disabled={!!busy || state?.cloud.authorization === "pending"}
              onClick={() =>
                void run("Conectando Google", async () => {
                  await connectGoogleDrive(
                    !native && state?.host !== "desktop",
                  );
                  await refresh();
                })
              }
            >
              <Cloud size={17} /> Conectar Drive
            </button>
          )}
          <input
            aria-label="Pesquisar projetos"
            placeholder="Pesquisar projetos…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        {(connectionError || error || busy) && (
          <div
            role="status"
            className={
              connectionError || error ? "projects-error" : "projects-status"
            }
          >
            {connectionError || error || `${busy}…`}
            {connectionError && (
              <button
                disabled={!!busy}
                onClick={() =>
                  void run("Reconectando", async () => {
                    await refresh();
                  })
                }
              >
                Tentar novamente
              </button>
            )}
            {error && !connectionError && (
              <button aria-label="Fechar erro" onClick={() => setError("")}>
                ×
              </button>
            )}
          </div>
        )}
        {state?.cloud.authorization &&
          !["pending", "connected"].includes(state.cloud.authorization) && (
            <p className="projects-error">{state.cloud.authorization}</p>
          )}
        <section>
          <div className="projects-section-title">
            <div>
              <h2>Projects</h2>
              <p>
                Recentes locais e Google Drive · {projectRows.length} projetos
              </p>
            </div>
            {state?.cloud.connected && <small>Google Drive conectado</small>}
          </div>
          <div className="projects-list">
            {projectRows
              .filter((project) => filtered(project.name, project.path))
              .map((project) => (
                <article key={project.key}>
                  <button
                    className="project-row"
                    disabled={!!busy || project.unavailable}
                    onClick={() =>
                      void run(
                        project.kind === "cloud"
                          ? "Abrindo Cloud"
                          : "Abrindo projeto",
                        () =>
                          open(
                            project.kind === "cloud"
                              ? "drive-open"
                              : "project-open",
                            project.kind === "cloud"
                              ? { id: project.cloudId }
                              : { path: project.path },
                          ),
                      )
                    }
                  >
                    {project.kind === "cloud" ? (
                      <Cloud size={21} />
                    ) : (
                      <Folder size={21} />
                    )}
                    <span>
                      <b>{project.name}</b>
                      <small title={project.path}>{project.path}</small>
                    </span>
                    <em>{project.kind === "cloud" ? "Cloud" : "Local"}</em>
                    <time
                      title={
                        project.lastAccess
                          ? new Date(project.lastAccess * 1000).toLocaleString()
                          : "Ainda não aberto"
                      }
                    >
                      {project.unavailable
                        ? "Indisponível"
                        : project.lastAccess
                          ? new Date(
                              project.lastAccess * 1000,
                            ).toLocaleDateString()
                          : "Novo"}
                    </time>
                  </button>
                  {project.id && (
                    <button
                      className="forget-project"
                      aria-label={`Remover ${project.name} dos recentes`}
                      title="Remover dos recentes; arquivos preservados"
                      disabled={!!busy}
                      onClick={() =>
                        void run("Removendo recente", async () =>
                          setState(
                            await api("project-forget", { id: project.id }),
                          ),
                        )
                      }
                    >
                      <Trash2 size={17} />
                    </button>
                  )}
                </article>
              ))}
            {!projectRows.filter((project) =>
              filtered(project.name, project.path),
            ).length && (
              <div className="projects-empty">
                <FolderOpen size={28} />
                <p>
                  {state?.cloud.connected
                    ? "Abra um projeto ou crie um projeto Cloud."
                    : "Abra uma pasta ou conecte Google Drive."}
                </p>
                <small>Projetos ordenados pelo último acesso.</small>
              </div>
            )}
          </div>
        </section>
        {state?.current && (
          <button className="projects-return" onClick={() => setScreen(false)}>
            <ArrowLeft size={17} />
            Voltar ao projeto aberto
          </button>
        )}
        {dialog && (
          <div className="projects-dialog">
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void run("Abrindo projeto", () =>
                  native && dialog === "create"
                    ? api("project-create", { name: path }).then(() => {
                        setDialog("");
                      })
                    : open(
                        dialog === "cloud"
                          ? "drive-create"
                          : dialog === "create"
                            ? "project-create"
                            : "project-open",
                        dialog === "cloud" ? { name: path } : { path },
                      ),
                );
              }}
            >
              <h2>
                {dialog === "open"
                  ? "Abrir projeto Local"
                  : dialog === "cloud"
                    ? "Novo projeto Cloud"
                    : "Novo projeto Local"}
              </h2>
              <label>
                {dialog === "cloud" || native
                  ? "Nome do projeto"
                  : "Caminho absoluto da pasta"}
                <input
                  autoFocus
                  required
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  placeholder={
                    dialog === "cloud" || native
                      ? "MyProject"
                      : "C:\\Projects\\MyProject"
                  }
                />
              </label>
              <p>
                {native && dialog === "create"
                  ? "Selecione a pasta de destino no Android."
                  : dialog === "create"
                    ? "A pasta será criada com main.lyn e configuração mínima."
                    : dialog === "cloud"
                      ? "Criado em Luvyn/ no Drive, identificado por metadata."
                      : "Abre os arquivos existentes sem copiá-los."}
              </p>
              <div>
                <button type="button" onClick={() => setDialog("")}>
                  Cancelar
                </button>
                <button
                  className="projects-primary"
                  disabled={!!busy || !path.trim()}
                  type="submit"
                >
                  {dialog === "open" ? "Abrir" : "Criar"}
                </button>
              </div>
            </form>
          </div>
        )}
      </div>
    </main>
  );
}
