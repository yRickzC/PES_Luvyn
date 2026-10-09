import { ProjectsGate } from "./Projects";
import {
  useState,
  useEffect,
  useRef,
  useCallback,
  type ReactNode,
} from "react";
import { createRoot } from "react-dom/client";
import {
  Files,
  FileCode2,
  Network,
  BookOpen,
  MoreHorizontal,
  Save,
  Plus,
  Play,
  AlertTriangle,
  Folder,
  ChevronDown,
  ChevronRight,
  Search,
  FolderOpen,
  RefreshCw,
} from "lucide-react";
import { api, type Snapshot, type Symbol } from "./api";
import { MobileEditor, type TouchEditor } from "./MobileEditor";
import { Graph } from "./Graph";
import { useDictionary, LanguageSidebar, LanguageDetails } from "./Dictionary";
import "./style.css";
import "./mobile.css";

function fuzzyScore(value: string, query: string) {
  const candidate = value.toLowerCase();
  const needle = query.trim().toLowerCase();
  if (!needle) return 0;
  if (candidate === needle) return 1000;
  if (candidate.startsWith(needle)) return 800 - candidate.length;
  if (candidate.includes(needle)) return 600 - candidate.indexOf(needle);
  let position = -1,
    gaps = 0;
  for (const char of needle) {
    const next = candidate.indexOf(char, position + 1);
    if (next < 0) return -1;
    if (position >= 0) gaps += next - position - 1;
    position = next;
  }
  return 300 - gaps - candidate.length * 0.1;
}

function Mobile({
  onProjects,
  onSync,
  currentIsCloud,
}: {
  onProjects: () => void;
  onSync: () => Promise<void>;
  currentIsCloud: boolean;
}) {
  const [page, setPage] = useState("files"),
    [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [file, setFile] = useState(""),
    [initial, setInitial] = useState(""),
    [hash, setHash] = useState<string | null>(null),
    [dirty, setDirty] = useState(false),
    [opened, setOpened] = useState(0);
  const [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false),
    [focus, setFocus] = useState(""),
    [keyword, setKeyword] = useState(""),
    [fileSearch, setFileSearch] = useState(""),
    [graphDialog, setGraphDialog] = useState(false),
    [graphSearch, setGraphSearch] = useState(""),
    [graphDepth, setGraphDepth] = useState(2);
  const [dialog, setDialog] = useState<{ op: string; file?: string } | null>(
      null,
    ),
    [name, setName] = useState("");
  const [selectedFolder, setSelectedFolder] = useState(""),
    [createMenu, setCreateMenu] = useState(false),
    [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [logs, setLogs] = useState<string[]>([]),
    [problems, setProblems] = useState(false),
    [info, setInfo] = useState<any>(null);
  const editor = useRef<TouchEditor | null>(null),
    navigation = useRef<{ line: number; column: number } | null>(null);
  const dictionary = useDictionary();
  const refresh = useCallback(async () => {
    setSnapshot(await api<Snapshot>("snapshot"));
    if ((window as any).LuvynNative) setInfo(await api("platform-info"));
  }, []);
  const run = async (operation: () => Promise<void>) => {
    setBusy(true);
    try {
      await operation();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void refresh().catch((error) => setMessage(String(error)));
    const changed = (event: Event) => {
      const detail = (event as CustomEvent).detail;
      if (detail.error) setMessage(detail.error);
      else {
        setFile("");
        setDirty(false);
        setOpened((n) => n + 1);
        void refresh().catch((error) => setMessage(String(error)));
      }
    };
    const error = (event: Event) => setMessage((event as CustomEvent).detail);
    window.addEventListener("luvyn-workspace", changed);
    window.addEventListener("luvyn-error", error);
    return () => {
      window.removeEventListener("luvyn-workspace", changed);
      window.removeEventListener("luvyn-error", error);
    };
  }, [refresh]);
  useEffect(() => {
    const back = () => {
      if (dirty) {
        setMessage("Salve alterações antes de trocar de projeto.");
        return;
      }
      onProjects();
    };
    window.addEventListener("luvyn-projects-request", back);
    return () => window.removeEventListener("luvyn-projects-request", back);
  }, [dirty, onProjects]);
  useEffect(() => {
    const request = (event: Event) => {
      if (dirty) {
        setMessage("Salve alterações antes de sincronizar.");
        return;
      }
      (event as CustomEvent).detail.run();
    };
    const synced = () => {
      setFile("");
      setDirty(false);
      void refresh();
    };
    window.addEventListener("luvyn-sync-request", request);
    window.addEventListener("luvyn-sync", synced);
    return () => {
      window.removeEventListener("luvyn-sync-request", request);
      window.removeEventListener("luvyn-sync", synced);
    };
  }, [dirty, refresh]);
  const open = async (path: string, line = 1, column = 1) => {
    if (dirty && file !== path) {
      setMessage("Salve alterações antes de abrir outro arquivo.");
      return;
    }
    if (file === path && editor.current) {
      const at = editor.current.view.state.doc.line(
        Math.min(line, editor.current.view.state.doc.lines),
      );
      const pos = Math.min(at.to, at.from + column - 1);
      editor.current.view.dispatch({ selection: { anchor: pos } });
      setPage("editor");
      return;
    }
    const data = await api("file", { file: path });
    navigation.current = { line, column };
    setFile(path);
    setInitial(data.text);
    setHash(data.hash);
    setDirty(false);
    setOpened((n) => n + 1);
    setPage("editor");
  };
  const ready = useCallback((value: TouchEditor | null) => {
    editor.current = value;
    if (value && navigation.current) {
      const at = value.view.state.doc.line(
        Math.min(navigation.current.line, value.view.state.doc.lines),
      );
      value.view.dispatch({
        selection: {
          anchor: Math.min(at.to, at.from + navigation.current.column - 1),
        },
      });
      navigation.current = null;
    }
  }, []);
  const save = () =>
    run(async () => {
      if (!editor.current) return;
      const data = await api("save", {
        file,
        text: editor.current.text(),
        hash,
      });
      setHash(data.hash);
      setDirty(false);
      setMessage("Arquivo salvo");
      await refresh();
    });
  const build = () =>
    run(async () => {
      if (dirty) throw new Error("Salve arquivo antes de Build.");
      try {
        const data = await api("build");
        if (data.pending || data.target_required) {
          setMessage("Selecione o Target para continuar o Build.");
          return;
        }
        setLogs([
          ...(data.logs || []),
          ...(data.output ? [`Wrote: ${data.output}`] : []),
        ]);
        setMessage("Build concluído");
        await refresh();
      } catch (error) {
        if (
          (window as any).LuvynNative &&
          String(error).toLowerCase().includes("target")
        ) {
          await api("open-target");
          setMessage("Selecione o Target para continuar o Build.");
          return;
        }
        throw error;
      }
    });
  const mutate = () =>
    run(async () => {
      if (!dialog) return;
      if (dirty) throw new Error("Salve arquivo antes de alterar Files.");
      const op = dialog.op;
      await api(
        op,
        op === "move"
          ? { file: dialog.file, to: name }
          : {
              file: op === "delete" ? dialog.file : name,
              text: "",
              recursive: op === "delete",
            },
      );
      const removedActive =
        op === "delete" &&
        !!dialog.file &&
        (file === dialog.file || file.startsWith(`${dialog.file}/`));
      const movedActive =
        op === "move" &&
        !!dialog.file &&
        !!name &&
        (file === dialog.file || file.startsWith(`${dialog.file}/`));
      const movedTo = movedActive
        ? `${name}${file.slice(dialog.file!.length)}`
        : "";
      if (removedActive) {
        setFile("");
        setInitial("");
      }
      setDialog(null);
      await refresh();
      if (movedTo) await open(movedTo);
    });
  const selectSymbol = (symbol: Symbol) =>
    void run(() =>
      open(symbol.location.file, symbol.location.line, symbol.location.column),
    );
  const nodes =
    snapshot?.symbols.filter(
      (s) => s.kind !== "field" && s.kind !== "module",
    ) || [];
  const focusedSymbol = nodes.find((symbol) => symbol.qualified === focus);
  const symbolMatches = nodes
    .map((symbol) => ({
      symbol,
      score: Math.max(
        fuzzyScore(symbol.name, graphSearch),
        fuzzyScore(symbol.qualified, graphSearch),
        fuzzyScore(symbol.kind, graphSearch),
        fuzzyScore(symbol.namespace, graphSearch),
      ),
    }))
    .filter((item) => item.score >= 0)
    .sort(
      (a, b) =>
        b.score - a.score ||
        a.symbol.qualified.localeCompare(b.symbol.qualified),
    )
    .slice(0, 24);
  const treePaths = [
    ...(snapshot?.folders || []).map((path) => ({ path, folder: true })),
    ...(snapshot?.files || []).map((entry) => ({
      path: entry.path,
      folder: false,
    })),
  ].sort(
    (a, b) =>
      a.path.localeCompare(b.path) || Number(b.folder) - Number(a.folder),
  );
  const parentOf = (path: string) =>
    path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
  const fileTree = (parent = "", depth = 0): ReactNode =>
    treePaths
      .filter((entry) => parentOf(entry.path) === parent)
      .filter(
        (entry) =>
          !fileSearch ||
          entry.folder ||
          entry.path.toLowerCase().includes(fileSearch.toLowerCase()),
      )
      .map((entry) => {
        const closed = collapsed.has(entry.path) && !fileSearch;
        return (
          <div key={entry.path}>
            <div
              className={`mobile-tree-row ${selectedFolder === entry.path || file === entry.path ? "selected" : ""}`}
              style={{ paddingLeft: 8 + depth * 16 }}
            >
              <button
                className="mobile-tree-label"
                onClick={() => {
                  if (entry.folder) {
                    setSelectedFolder(entry.path);
                    setCollapsed((current) => {
                      const next = new Set(current);
                      if (next.has(entry.path)) next.delete(entry.path);
                      else next.add(entry.path);
                      return next;
                    });
                  } else {
                    setSelectedFolder(parentOf(entry.path));
                    void run(() => open(entry.path));
                  }
                }}
              >
                {entry.folder ? (
                  closed ? (
                    <ChevronRight size={17} />
                  ) : (
                    <ChevronDown size={17} />
                  )
                ) : (
                  <span className="mobile-tree-spacer" />
                )}
                {entry.folder ? <Folder size={18} /> : <FileCode2 size={18} />}
                <span>
                  {entry.path.split("/").at(-1)}
                  {entry.folder ? "/" : ""}
                </span>
              </button>
              <button
                aria-label={`Renomear ${entry.folder ? "pasta" : "arquivo"} ${entry.path}`}
                onClick={() => {
                  setDialog({ op: "move", file: entry.path });
                  setName(entry.path);
                }}
              >
                Renomear
              </button>
              <button
                aria-label={`Excluir ${entry.folder ? "pasta" : "arquivo"} ${entry.path}`}
                onClick={() => setDialog({ op: "delete", file: entry.path })}
              >
                Excluir
              </button>
            </div>
            {entry.folder && !closed && fileTree(entry.path, depth + 1)}
          </div>
        );
      });
  return (
    <div className="mobile-app">
      <header className="mobile-header">
        <strong>Luvyn</strong>
        <span>{page === "editor" ? file || "Editor" : "Documentação"}</span>
        {page === "editor" && file ? (
          <button aria-label="Salvar" disabled={busy || !dirty} onClick={save}>
            <Save size={22} />
            {dirty ? "Salvar" : "Salvo"}
          </button>
        ) : (
          <button aria-label="Build" disabled={busy} onClick={build}>
            <Play size={20} />
          </button>
        )}
      </header>
      <main className="mobile-content">
        {page === "files" && (
          <section className="mobile-files">
            <div className="mobile-title">
              <h1>Files</h1>
              <div className="mobile-create-wrap">
                <button
                  aria-label="Criar"
                  aria-expanded={createMenu}
                  onClick={() => setCreateMenu(!createMenu)}
                >
                  <Plus size={22} />
                </button>
                {createMenu && (
                  <div className="mobile-create-menu">
                    <button
                      onClick={() => {
                        setCreateMenu(false);
                        setDialog({ op: "create" });
                        setName(
                          `${selectedFolder ? `${selectedFolder}/` : ""}new.lyn`,
                        );
                      }}
                    >
                      Novo arquivo
                    </button>
                    <button
                      onClick={() => {
                        setCreateMenu(false);
                        setDialog({ op: "mkdir" });
                        setName(
                          `${selectedFolder ? `${selectedFolder}/` : ""}nova-pasta`,
                        );
                      }}
                    >
                      Nova pasta
                    </button>
                  </div>
                )}
              </div>
            </div>
            <input
              aria-label="Filtrar arquivos e pastas"
              placeholder="Buscar .lyn"
              value={fileSearch}
              onChange={(e) => setFileSearch(e.target.value)}
            />
            {!snapshot?.files.length && !snapshot?.folders.length && (
              <div className="mobile-empty">
                <Files size={36} />
                <h2>Abra sua documentação</h2>
                <p>
                  Selecione pasta .lyn. O Target recebe somente artefatos
                  compilados.
                </p>
                <button
                  disabled={dirty}
                  onClick={() =>
                    void run(async () => {
                      await api("open-docs");
                    })
                  }
                >
                  Abrir pasta
                </button>
              </div>
            )}
            {snapshot &&
            (snapshot.files.length > 0 || snapshot.folders.length > 0)
              ? fileTree()
              : null}
          </section>
        )}
        <section hidden={page !== "editor"} className="mobile-editor-page">
          {file ? (
            <MobileEditor
              file={file}
              initial={initial}
              opened={opened}
              changed={() => setDirty(true)}
              analyzed={setSnapshot}
              ready={ready}
              symbols={snapshot?.symbols || []}
            />
          ) : (
            <div className="mobile-empty">
              <FileCode2 size={36} />
              <h2>Selecione arquivo em Files</h2>
            </div>
          )}
        </section>
        {page === "graph" && (
          <section className="mobile-graph">
            <div className="mobile-title">
              <h1>Graph</h1>
              <div className="mobile-graph-modes">
                <button
                  aria-pressed={!focus}
                  onClick={() => {
                    setFocus("");
                    setGraphDepth(2);
                  }}
                >
                  Geral
                </button>
                <button
                  onClick={() => {
                    setGraphSearch("");
                    setGraphDialog(true);
                  }}
                >
                  <Search size={16} />
                  Focar símbolo
                </button>
              </div>
            </div>
            {focus && (
              <div className="mobile-graph-focus">
                <span>
                  <b>{focusedSymbol?.name || focus}</b>
                  {focusedSymbol && (
                    <small>
                      {focusedSymbol.kind} · {focusedSymbol.namespace || "raiz"}
                    </small>
                  )}
                </span>
                <label>
                  Depth{" "}
                  <select
                    aria-label="Profundidade do grafo focado"
                    value={graphDepth}
                    onChange={(e) => setGraphDepth(Number(e.target.value))}
                  >
                    {[1, 2, 3, 4].map((depth) => (
                      <option key={depth} value={depth}>
                        {depth}
                      </option>
                    ))}
                  </select>
                </label>
                <button onClick={() => setFocus("")}>Grafo geral</button>
              </div>
            )}
            <Graph
              open={selectSymbol}
              focus={focus}
              theme="dark"
              revision={snapshot?.revision || 0}
              mobile
              depth={graphDepth}
              onDepthChange={setGraphDepth}
              onFocusChange={setFocus}
            />
            {graphDialog && (
              <div
                className="mobile-dialog graph-search-sheet"
                onMouseDown={(event) => {
                  if (event.target === event.currentTarget)
                    setGraphDialog(false);
                }}
              >
                <section
                  role="dialog"
                  aria-modal="true"
                  aria-labelledby="graph-search-title"
                >
                  <h2 id="graph-search-title">Buscar símbolo</h2>
                  <label className="graph-symbol-search">
                    <Search size={17} />
                    <input
                      autoFocus
                      placeholder="Nome, tipo ou módulo…"
                      value={graphSearch}
                      onChange={(event) => setGraphSearch(event.target.value)}
                    />
                  </label>
                  <div className="graph-symbol-results">
                    {symbolMatches.map(({ symbol }) => (
                      <button
                        key={symbol.id}
                        onClick={() => {
                          setFocus(symbol.qualified);
                          setGraphDepth(2);
                          setGraphDialog(false);
                        }}
                      >
                        <span>
                          <b>{symbol.name}</b>
                          <small>
                            {symbol.kind} · {symbol.namespace || "raiz"}
                          </small>
                        </span>
                        <ChevronRight size={18} />
                      </button>
                    ))}
                    {!symbolMatches.length && <p>Nenhum símbolo encontrado.</p>}
                  </div>
                  <button
                    className="graph-search-close"
                    onClick={() => setGraphDialog(false)}
                  >
                    Fechar
                  </button>
                </section>
              </div>
            )}
          </section>
        )}
        {page === "language" && (
          <section className="mobile-language">
            <div className="mobile-title">
              <h1>Language</h1>
              {keyword && (
                <button onClick={() => setKeyword("")}>Categorias</button>
              )}
            </div>
            {keyword ? (
              <LanguageDetails
                entry={dictionary.entries.find((e) => e.keyword === keyword)}
                select={setKeyword}
              />
            ) : (
              <LanguageSidebar
                entries={dictionary.entries}
                selected={keyword}
                select={setKeyword}
                error={dictionary.error}
              />
            )}
          </section>
        )}
        {page === "more" && (
          <section className="mobile-more">
            <h1>More</h1>
            <button onClick={() => setProblems(!problems)}>
              <AlertTriangle size={20} />
              Problems ({snapshot?.diagnostics.length || 0})
            </button>
            <button
              onClick={() =>
                window.dispatchEvent(new Event("luvyn-projects-request"))
              }
            >
              <FolderOpen size={20} />
              Projetos
            </button>
            {currentIsCloud && (
              <button
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await onSync();
                    setMessage("Cloud sincronizado");
                    await refresh();
                  })
                }
              >
                <RefreshCw size={20} />
                Sincronizar Cloud
              </button>
            )}
            {problems &&
              snapshot?.diagnostics.map((d, i) => (
                <button
                  className="mobile-problem"
                  key={i}
                  onClick={() =>
                    void run(() =>
                      open(
                        d.diagnostic.location.file,
                        d.range.startLineNumber,
                        d.range.startColumn,
                      ),
                    )
                  }
                >
                  <b>
                    {d.diagnostic.code} · {d.diagnostic.severity}
                  </b>
                  <span>{d.diagnostic.message}</span>
                  <small>
                    {d.diagnostic.location.file}:{d.diagnostic.location.line}
                  </small>
                </button>
              ))}
            <dl>
              <dt>Documentation</dt>
              <dd>{info?.documentation || snapshot?.documentation_root}</dd>
              <dt>Target</dt>
              <dd>{info?.target || snapshot?.target_project_root}</dd>
            </dl>
            <button
              disabled={dirty || busy}
              onClick={() =>
                void run(async () => {
                  await api("open-docs");
                })
              }
            >
              Abrir documentação (SAF)
            </button>
            <button
              disabled={dirty || busy}
              onClick={() =>
                void run(async () => {
                  await api("open-target");
                })
              }
            >
              Selecionar Target (SAF)
            </button>
            <button
              disabled={dirty || busy}
              onClick={() =>
                void run(async () => {
                  await api("reload-provider");
                  setFile("");
                  await refresh();
                })
              }
            >
              Reimportar documentos
            </button>
            <button disabled={busy} onClick={build}>
              Build para Target
            </button>
            {logs.length > 0 && (
              <pre className="mobile-build-output">{logs.join("\n")}</pre>
            )}
          </section>
        )}
      </main>
      {message && (
        <div className="mobile-message" role="status">
          <span>{message}</span>
          <button aria-label="Fechar mensagem" onClick={() => setMessage("")}>
            ×
          </button>
        </div>
      )}
      {dialog && (
        <div className="mobile-dialog">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void mutate();
            }}
          >
            <h2>
              {dialog.op === "delete"
                ? "Excluir item?"
                : dialog.op === "create"
                  ? "Novo arquivo"
                  : dialog.op === "mkdir"
                    ? "Nova pasta"
                    : "Renomear"}
            </h2>
            {dialog.op === "delete" ? (
              <>
                <p>{dialog.file}</p>
                <p>
                  Esta ação exclui{" "}
                  {snapshot?.folders.includes(dialog.file || "")
                    ? "a pasta e todo o seu conteúdo"
                    : "o arquivo"}
                  .
                </p>
              </>
            ) : (
              <label>
                Path relativo
                <input
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
              </label>
            )}
            <div>
              <button type="button" onClick={() => setDialog(null)}>
                Cancelar
              </button>
              <button disabled={busy} type="submit">
                {dialog.op === "delete" ? "Excluir" : "Salvar"}
              </button>
            </div>
          </form>
        </div>
      )}
      <nav className="mobile-nav" aria-label="Navegação">
        {[
          ["files", Files, "Files"],
          ["editor", FileCode2, "Editor"],
          ["graph", Network, "Graph"],
          ["language", BookOpen, "Language"],
          ["more", MoreHorizontal, "More"],
        ].map(([id, Icon, label]) => {
          const Component = Icon as typeof Files;
          return (
            <button
              key={String(id)}
              aria-current={page === id ? "page" : undefined}
              onClick={() => setPage(String(id))}
            >
              <Component size={22} />
              <span>{String(label)}</span>
            </button>
          );
        })}
      </nav>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <ProjectsGate mobile>
    {(back, sync, currentIsCloud) => (
      <Mobile onProjects={back} onSync={sync} currentIsCloud={currentIsCloud} />
    )}
  </ProjectsGate>,
);
