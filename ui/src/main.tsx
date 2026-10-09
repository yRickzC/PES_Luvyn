import { ProjectsGate } from "./Projects";
import React, { useState, useEffect, useRef, useCallback } from "react";
import { createRoot } from "react-dom/client";
import {
  Files,
  BookOpen,
  Search,
  Network,
  Settings,
  Command,
  ChevronRight,
  ChevronDown,
  Plus,
  FolderPlus,
  RefreshCw,
  X,
  FileCode2,
  Folder,
  Play,
  Check,
  Package,
  PanelBottom,
  Sun,
  Moon,
  Save,
  GitBranch,
  AlertTriangle,
  ArrowRight,
  Braces,
  MoreHorizontal,
  ExternalLink,
  FilePlus,
  CheckCircle2,
} from "lucide-react";
import { api, type Snapshot, type Symbol, type Edit } from "./api";
import {
  monaco,
  uri,
  fileOf,
  registerLanguage,
  setBridge,
  bridge,
} from "./language";
import { Graph } from "./Graph";
import {
  useDictionary,
  LanguageSidebar,
  LanguageDetails,
  type LanguageEntry,
} from "./Dictionary";
import "./style.css";

registerLanguage();
type Tab = {
  file: string;
  model: monaco.editor.ITextModel;
  hash: string | null;
  dirty: boolean;
  conflict: boolean;
  version: number;
  view?: monaco.editor.ICodeEditorViewState | null;
  markerFingerprint?: string;
  subscription?: monaco.IDisposable;
};
type Modal = { type: "file" | "folder" | "move"; file?: string };
type Palette = { mode: "commands" | "symbols"; query: string };

function App({
  onProjects,
  onSync,
  currentIsCloud,
}: {
  onProjects: () => void;
  onSync: () => Promise<void>;
  currentIsCloud: boolean;
}) {
  const dictionary = useDictionary();
  const [languageKeyword, setLanguageKeyword] = useState("self");
  const operationRef = useRef(false);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [tabs, setTabs] = useState<Tab[]>([]),
    [active, setActive] = useState("");
  const [section, setSection] = useState("files"),
    [mode, setMode] = useState("editor"),
    [graphFocus, setGraphFocus] = useState("");
  const [panel, setPanel] = useState(true),
    [panelTab, setPanelTab] = useState("problems"),
    [logs, setLogs] = useState<string[]>([]);
  const [autosave, setAutosave] = useState(
      localStorage.getItem("luvyn-autosave") === "true",
    ),
    [theme, setTheme] = useState(localStorage.getItem("luvyn-theme") || "dark");
  const [palette, setPalette] = useState<Palette | null>(null),
    [paletteIndex, setPaletteIndex] = useState(0),
    [modal, setModal] = useState<Modal | null>(null),
    [toast, setToast] = useState(""),
    [busy, setBusy] = useState("");
  const [search, setSearch] = useState(""),
    [searchKind, setSearchKind] = useState("text"),
    [results, setResults] = useState<any[]>([]),
    [context, setContext] = useState("");
  const [filter, setFilter] = useState(""),
    [collapsed, setCollapsed] = useState<Set<string>>(new Set()),
    [selectedPath, setSelectedPath] = useState(""),
    [position, setPosition] = useState({ lineNumber: 1, column: 1 });
  const [fileMenu, setFileMenu] = useState("");
  const editorHost = useRef<HTMLDivElement>(null),
    editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const tabsRef = useRef<Tab[]>([]),
    activeRef = useRef(""),
    snapshotRef = useRef<Snapshot | null>(null),
    autosaveRef = useRef(autosave);
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>()),
    openPending = useRef(new Map<string, Promise<monaco.editor.ITextModel>>()),
    savePending = useRef(new Map<string, Promise<void>>()),
    refreshing = useRef(false),
    mounted = useRef(true);
  const notify = useCallback((text: string) => {
    setToast(text);
    setTimeout(() => setToast(""), 5500);
  }, []);
  const touch = useCallback(() => setTabs([...tabsRef.current]), []);
  const log = useCallback((message: string) => {
    setLogs((l) => [
      ...l.slice(-150),
      `${new Date().toLocaleTimeString()}  ${message}`,
    ]);
    setPanel(true);
  }, []);
  useEffect(() => {
    const back = () => {
      if (tabsRef.current.some((tab) => tab.dirty)) {
        alert("Salve alterações antes de trocar de projeto.");
        return;
      }
      onProjects();
    };
    window.addEventListener("luvyn-projects-request", back);
    return () => window.removeEventListener("luvyn-projects-request", back);
  }, [onProjects]);
  useEffect(() => {
    const request = (event: Event) => {
      if (tabsRef.current.some((tab) => tab.dirty)) {
        notify("Salve alterações antes de sincronizar.");
        return;
      }
      (event as CustomEvent).detail.run();
    };
    window.addEventListener("luvyn-sync-request", request);
    return () => window.removeEventListener("luvyn-sync-request", request);
  }, [notify]);
  const sync = useCallback(async (model: monaco.editor.ITextModel) => {
    if (!fileOf(model).endsWith(".lyn")) return;
    await api("edit", { file: fileOf(model), text: model.getValue() });
  }, []);
  const refresh = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const data = await api<Snapshot & { unchanged?: boolean }>("snapshot", {
        revision: snapshotRef.current?.revision,
      });
      if (!mounted.current || data.unchanged) return;
      if (
        !snapshotRef.current &&
        localStorage.getItem("luvyn-autosave") === null
      )
        setAutosave(data.autosave);
      snapshotRef.current = data;
      setSnapshot(data);
      for (const tab of tabsRef.current) {
        const disk = [...data.files, ...(data.configs || [])].find(
          (f) => f.path === tab.file,
        );
        if ((disk?.hash ?? null) !== tab.hash) {
          if (tab.dirty) {
            tab.conflict = true;
            touch();
          } else if (disk) {
            const read = await api("file", { file: tab.file });
            const view =
              editor.current?.getModel() === tab.model
                ? editor.current.saveViewState()
                : null;
            tab.hash = read.hash;
            tab.model.setValue(read.text);
            tab.version = tab.model.getAlternativeVersionId();
            tab.dirty = false;
            tab.conflict = false;
            if (view) editor.current?.restoreViewState(view);
            touch();
          } else {
            tab.conflict = true;
            tab.dirty = true;
            touch();
          }
        }
        const diagnostics = data.diagnostics.filter(
          (d) => d.diagnostic.location.file === tab.file,
        );
        const fingerprint = JSON.stringify(diagnostics);
        if (fingerprint === tab.markerFingerprint) continue;
        tab.markerFingerprint = fingerprint;
        monaco.editor.setModelMarkers(
          tab.model,
          "luvyn",
          diagnostics
            .filter((d) => d.diagnostic.location.file === tab.file)
            .map((d) => ({
              ...d.range,
              message:
                d.diagnostic.message +
                (d.diagnostic.suggestion ? `\n${d.diagnostic.suggestion}` : ""),
              severity:
                d.diagnostic.severity === "error"
                  ? monaco.MarkerSeverity.Error
                  : monaco.MarkerSeverity.Warning,
              code: d.diagnostic.code,
              source: "Luvyn",
            })),
        );
      }
    } catch (e) {
      notify(
        `Conexão com workspace: ${e instanceof Error ? e.message : String(e)}`,
      );
    } finally {
      refreshing.current = false;
    }
  }, [notify, touch]);
  const activate = useCallback((file: string) => {
    const old = tabsRef.current.find((t) => t.file === activeRef.current);
    if (old) old.view = editor.current?.saveViewState();
    const tab = tabsRef.current.find((t) => t.file === file);
    if (!tab) return;
    activeRef.current = file;
    setActive(file);
    setSelectedPath(file);
    setMode("editor");
    editor.current?.setModel(tab.model);
    if (tab.view) editor.current?.restoreViewState(tab.view);
    editor.current?.focus();
  }, []);
  const save = useCallback(
    async (file = activeRef.current) => {
      // Serialize Ctrl+S/autosave while a previous Cloud upload is still running.
      const previous = savePending.current.get(file) || Promise.resolve();
      const pending = previous
        .catch(() => {})
        .then(async () => {
          const tab = tabsRef.current.find((t) => t.file === file);
          if (!tab) return;
          if (!tab.dirty) {
            if (currentIsCloud) {
              try {
                await onSync();
                notify("Salvo no Google Drive");
              } catch (error) {
                notify(
                  `Conteúdo local preservado; envio ao Drive pendente: ${String(error)}`,
                );
              }
            }
            return;
          }
          if (tab.conflict) {
            notify(
              "Arquivo alterado externamente. Revise conflito ou recarregue antes de salvar.",
            );
            return;
          }
          const text = tab.model.getValue(),
            version = tab.model.getAlternativeVersionId();
          try {
            const data = await api("save", { file, text, hash: tab.hash });
            tab.hash = data.hash;
            tab.version = version;
            tab.dirty = tab.model.getAlternativeVersionId() !== version;
            tab.conflict = false;
            if (data.cloud_error)
              notify(
                `Salvo localmente; envio ao Drive pendente: ${data.cloud_error}`,
              );
            else if (data.cloud_synced) notify("Salvo no Google Drive");
            touch();
            if (tab.dirty) await sync(tab.model);
            void refresh();
          } catch (e) {
            notify(e instanceof Error ? e.message : String(e));
          }
        });
      savePending.current.set(file, pending);
      try {
        await pending;
      } finally {
        if (savePending.current.get(file) === pending)
          savePending.current.delete(file);
      }
    },
    [notify, refresh, sync, touch, currentIsCloud, onSync],
  );
  const open = useCallback(
    async (
      file: string,
      line?: number,
      column?: number,
      reveal = true,
    ): Promise<monaco.editor.ITextModel> => {
      const found = tabsRef.current.find((t) => t.file === file);
      if (found) {
        if (reveal) activate(file);
        if (reveal && line) {
          editor.current?.setPosition({
            lineNumber: line,
            column: column || 1,
          });
          editor.current?.revealLineInCenter(line);
        }
        return found.model;
      }
      const pending = openPending.current.get(file);
      if (pending) {
        const model = await pending;
        if (reveal) {
          activate(file);
          if (line) {
            editor.current?.setPosition({
              lineNumber: line,
              column: column || 1,
            });
            editor.current?.revealLineInCenter(line);
          }
        }
        return model;
      }
      const request = (async () => {
        const data = await api("file", { file });
        const model =
          monaco.editor.getModel(uri(file)) ||
          monaco.editor.createModel(
            data.text,
            file.endsWith(".lyn") ? "lyn" : "plaintext",
            uri(file),
          );
        const tab: Tab = {
          file,
          model,
          hash: data.hash,
          dirty: false,
          conflict: false,
          version: model.getAlternativeVersionId(),
        };
        tab.subscription = model.onDidChangeContent(() => {
          tab.dirty = model.getAlternativeVersionId() !== tab.version;
          touch();
          const timer = timers.current.get(file);
          if (timer) clearTimeout(timer);
          timers.current.set(
            file,
            setTimeout(async () => {
              try {
                await sync(model);
                await refresh();
                if (autosaveRef.current && tab.dirty) await save(file);
              } catch (e) {
                notify(String(e));
              }
            }, 450),
          );
        });
        tabsRef.current = [...tabsRef.current, tab];
        touch();
        if (reveal) activate(file);
        if (reveal && line) {
          editor.current?.setPosition({
            lineNumber: line,
            column: column || 1,
          });
          editor.current?.revealLineInCenter(line);
        }
        return model;
      })();
      openPending.current.set(file, request);
      try {
        return await request;
      } finally {
        openPending.current.delete(file);
      }
    },
    [activate, notify, refresh, save, sync, touch],
  );
  const apply = useCallback(
    async (edits: Edit[]) => {
      const grouped = new Map<string, Edit[]>();
      for (const edit of edits)
        grouped.set(edit.file, [...(grouped.get(edit.file) || []), edit]);
      for (const [file, list] of grouped) {
        const model = await open(file);
        model.pushStackElement();
        model.pushEditOperations(
          [],
          list.map((e) => ({ range: e.range, text: e.text })),
          () => null,
        );
        model.pushStackElement();
        await sync(model);
      }
      await refresh();
    },
    [open, refresh, sync],
  );
  const openSymbol = useCallback(
    (symbol: Symbol) => {
      void open(
        symbol.location.file,
        symbol.location.line,
        symbol.location.column,
      ).catch((e) => notify(String(e)));
    },
    [open, notify],
  );
  async function close(file: string) {
    const tab = tabsRef.current.find((t) => t.file === file);
    if (!tab) return;
    if (tab.dirty && !confirm(`Descartar alterações em ${file}?`)) return;
    await api("discard", { file });
    tab.subscription?.dispose();
    tab.model.dispose();
    clearTimeout(timers.current.get(file));
    tabsRef.current = tabsRef.current.filter((t) => t !== tab);
    touch();
    if (activeRef.current === file) {
      activeRef.current = "";
      setActive("");
      editor.current?.setModel(null);
      if (tabsRef.current.length) activate(tabsRef.current.at(-1)!.file);
    }
    void refresh();
  }
  async function reload() {
    const tab = tabsRef.current.find((t) => t.file === activeRef.current);
    if (!tab) return;
    if (
      tab.dirty &&
      !confirm("Recarregar descarta alterações locais. Continuar?")
    )
      return;
    await api("discard", { file: tab.file });
    const data = await api("file", { file: tab.file });
    tab.model.setValue(data.text);
    tab.hash = data.hash;
    tab.version = tab.model.getAlternativeVersionId();
    tab.dirty = false;
    tab.conflict = false;
    touch();
    await refresh();
  }
  async function compareConflict() {
    const tab = tabsRef.current.find((t) => t.file === activeRef.current);
    if (!tab) return;
    try {
      if (!snapshotRef.current?.files.some((f) => f.path === tab.file)) {
        setConflict({
          file: tab.file,
          local: tab.model.getValue(),
          disk: "",
          hash: null,
        });
        return;
      }
      const disk = await api("file", { file: tab.file });
      setConflict({
        file: tab.file,
        local: tab.model.getValue(),
        disk: disk.text,
        hash: disk.hash,
      });
    } catch (e) {
      notify(String(e));
    }
  }
  const [conflict, setConflict] = useState<{
    file: string;
    local: string;
    disk: string;
    hash: string | null;
  } | null>(null);
  async function run(name: string) {
    if (operationRef.current) return;
    operationRef.current = true;
    setBusy(name);
    setPanel(true);
    setPanelTab("output");
    log(`[${name}] iniciado`);
    let poll: ReturnType<typeof setInterval> | undefined;
    let received = 0;
    const progress = async () => {
      const data = await api("build-status");
      for (const line of data.logs.slice(received)) log(line);
      received = data.logs.length;
    };
    try {
      if (["build", "export"].includes(name)) {
        for (const tab of tabsRef.current) if (tab.dirty) await save(tab.file);
        if (tabsRef.current.some((t) => t.dirty))
          throw new Error("Salve ou resolva conflitos antes de continuar.");
      }
      if (name === "build" || name === "export")
        poll = setInterval(() => void progress().catch(() => {}), 200);
      const data = await api(name);
      if (poll) await progress();
      log(
        name === "check"
          ? `Check: ${data.ok ? "sem erros" : `${data.diagnostics.length} diagnostics`}`
          : name === "export"
            ? `Export: ${data.output}`
            : `Build: ${data.stats.symbols} símbolos, ${data.stats.edges} relações. ${data.stats.parsed} arquivos parseados; ${data.stats.reused} reutilizados.`,
      );
      if (name !== "check") setPanelTab("output");
      await refresh();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      notify(message);
      log(message);
      setPanelTab("output");
    } finally {
      if (poll) {
        clearInterval(poll);
        await progress().catch(() => {});
      }
      await refresh();
      operationRef.current = false;
      setBusy("");
    }
  }
  const actions = [
    {
      label: "Luvyn: Build",
      shortcut: "Ctrl+Shift+B",
      execute: () => run("build"),
    },
    { label: "Luvyn: Check", shortcut: "", execute: () => run("check") },
    { label: "Luvyn: Export", shortcut: "", execute: () => run("export") },
    {
      label: "Luvyn: Open Graph",
      shortcut: "",
      execute: () => {
        setMode("graph");
        setGraphFocus("");
      },
    },
    {
      label: "Luvyn: Find Symbol",
      shortcut: "Ctrl+P",
      execute: () => setPalette({ mode: "symbols", query: "" }),
    },
    {
      label: "Luvyn: Format Document",
      shortcut: "Shift+Alt+F",
      execute: () =>
        editor.current?.getAction("editor.action.formatDocument")?.run(),
    },
    {
      label: "Luvyn: Add Missing Imports",
      shortcut: "",
      execute: async () => {
        const tab = tabsRef.current.find((t) => t.file === activeRef.current);
        if (tab) {
          await sync(tab.model);
          const data = await api("imports", { file: tab.file });
          await apply(data.edits);
          notify(
            data.edits.length
              ? "Imports adicionados."
              : "Nenhum import ausente.",
          );
        }
      },
    },
    {
      label: "File: New Document",
      shortcut: "",
      execute: () => setModal({ type: "file" }),
    },
    {
      label: "File: Save All",
      shortcut: "",
      execute: async () => {
        for (const tab of tabsRef.current) await save(tab.file);
      },
    },
    { label: "File: Reload From Disk", shortcut: "", execute: () => reload() },
    {
      label: "Editor: Find References",
      shortcut: "Shift+F12",
      execute: () =>
        editor.current
          ?.getAction("editor.action.referenceSearch.trigger")
          ?.run(),
    },
    {
      label: "Editor: Rename Symbol",
      shortcut: "F2",
      execute: () => editor.current?.getAction("editor.action.rename")?.run(),
    },
    {
      label: "View: Toggle Theme",
      shortcut: "",
      execute: () => setTheme((t) => (t === "dark" ? "light" : "dark")),
    },
    {
      label: "View: Toggle Problems",
      shortcut: "Ctrl+J",
      execute: () => setPanel((p) => !p),
    },
  ];
  const actionsRef = useRef(actions);
  actionsRef.current = actions;
  useEffect(() => {
    setBridge({ open, apply, sync, report: notify });
  }, [open, apply, sync, notify]);
  useEffect(() => {
    if (!editorHost.current) return;
    editor.current = monaco.editor.create(editorHost.current, {
      theme: "luvyn-dark",
      fontFamily: '"Cascadia Code", "JetBrains Mono", Consolas, monospace',
      fontSize: 13,
      lineHeight: 23,
      automaticLayout: true,
      minimap: { enabled: false },
      padding: { top: 18 },
      scrollBeyondLastLine: false,
      tabSize: 4,
      insertSpaces: true,
      renderLineHighlight: "all",
      smoothScrolling: false,
      wordWrap: "off",
      glyphMargin: true,
      folding: true,
      bracketPairColorization: { enabled: true },
      suggest: { showSnippets: true },
      quickSuggestions: { other: true, comments: false, strings: false },
    });
    const cursor = editor.current.onDidChangeCursorPosition((e) =>
      setPosition((previous) =>
        previous.lineNumber === e.position.lineNumber &&
        previous.column === e.position.column
          ? previous
          : e.position,
      ),
    );
    const opener = monaco.editor.registerEditorOpener({
      openCodeEditor: async (_editor, resource, selection) => {
        await bridge.open(
          resource.path.slice(1),
          selection &&
            ("startLineNumber" in selection
              ? selection.startLineNumber
              : selection.lineNumber),
          selection &&
            ("startColumn" in selection
              ? selection.startColumn
              : selection.column),
        );
        return true;
      },
    });
    void refresh();
    const poll = setInterval(() => void refresh(), 1500);
    const handle = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey) {
        if (e.key.toLowerCase() === "s") {
          e.preventDefault();
          void save();
        }
        if (e.key.toLowerCase() === "p") {
          e.preventDefault();
          setPalette({ mode: e.shiftKey ? "commands" : "symbols", query: "" });
          setPaletteIndex(0);
        }
        if (e.key.toLowerCase() === "b" && e.shiftKey) {
          e.preventDefault();
          void actionsRef.current[0].execute();
        }
        if (e.key.toLowerCase() === "j") {
          e.preventDefault();
          setPanel((p) => !p);
        }
      }
    };
    window.addEventListener("keydown", handle);
    const beforeUnload = (e: BeforeUnloadEvent) => {
      if (tabsRef.current.some((t) => t.dirty)) {
        e.preventDefault();
      }
    };
    window.addEventListener("beforeunload", beforeUnload);
    return () => {
      mounted.current = false;
      clearInterval(poll);
      window.removeEventListener("keydown", handle);
      window.removeEventListener("beforeunload", beforeUnload);
      cursor.dispose();
      opener.dispose();
      editor.current?.dispose();
    };
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    monaco.editor.setTheme(`luvyn-${theme}`);
    localStorage.setItem("luvyn-theme", theme);
  }, [theme]);
  useEffect(() => {
    autosaveRef.current = autosave;
    if (snapshotRef.current)
      localStorage.setItem("luvyn-autosave", String(autosave));
  }, [autosave]);
  useEffect(() => {
    if (snapshot && !tabsRef.current.length && snapshot.files.length) {
      void open(
        snapshot.files.find((f) => f.path.includes("UserService"))?.path ||
          snapshot.files[0].path,
      ).catch((e) => notify(String(e)));
    }
  }, [snapshot?.workspace]);
  const searchGeneration = useRef(0);
  const [searching, setSearching] = useState(false);
  function clearSearch() {
    searchGeneration.current++;
    setSearch("");
    setResults([]);
    setContext("");
    setSearching(false);
  }
  async function doSearch() {
    const generation = ++searchGeneration.current;
    if (!search.trim()) {
      setResults([]);
      setContext("");
      return;
    }
    setSearching(true);
    try {
      if (searchKind === "text") {
        const data = await api("search", { query: search });
        if (generation !== searchGeneration.current) return;
        setResults(data.matches);
        setContext(
          data.truncated ? "Busca limitada a 200 resultados / 32 MiB." : "",
        );
      } else {
        const data = await api("query", { query: search, depth: 1 });
        if (generation !== searchGeneration.current) return;
        setResults(data.result.symbols);
        setContext(data.context);
      }
    } catch (e) {
      if (generation !== searchGeneration.current) return;
      notify(String(e));
      setResults([]);
    } finally {
      if (generation === searchGeneration.current) setSearching(false);
    }
  }
  async function removePath(file: string) {
    if (!confirm(`Excluir ${file} e todo o seu conteúdo?`)) return;
    try {
      await api("delete", { file, recursive: true });
      const tab = tabsRef.current.find((t) => t.file === file);
      if (tab) await close(file);
      await refresh();
    } catch (e) {
      notify(String(e));
    }
  }
  function resize(
    e: React.PointerEvent,
    name: string,
    axis: "x" | "y",
    reverse = false,
  ) {
    const start = axis === "x" ? e.clientX : e.clientY;
    const current = parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue(name),
    );
    const move = (event: PointerEvent) => {
      const position = axis === "x" ? event.clientX : event.clientY;
      document.documentElement.style.setProperty(
        name,
        `${Math.max(axis === "x" ? 180 : 100, Math.min(axis === "x" ? 480 : 440, current + (position - start) * (reverse ? -1 : 1)))}px`,
      );
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
  const activeTab = tabs.find((t) => t.file === active),
    problems = snapshot?.diagnostics || [],
    errors = problems.filter((d) => d.diagnostic.severity === "error").length;
  const outline =
    snapshot?.symbols.filter((s) => s.location.file === active) || [];
  const paletteItems =
    palette?.mode === "symbols"
      ? (snapshot?.symbols || [])
          .filter((s) =>
            s.qualified.toLowerCase().includes(palette.query.toLowerCase()),
          )
          .map((s) => ({
            label: s.qualified,
            detail: s.kind,
            shortcut: "",
            execute: () => openSymbol(s),
          }))
      : actions
          .filter((a) =>
            a.label.toLowerCase().includes(palette?.query.toLowerCase() || ""),
          )
          .map((a) => ({ ...a, detail: "" }));
  const treePaths = [
    ...(snapshot?.folders || []).map((path) => ({ path, folder: true })),
    ...(snapshot?.files || []).map((f) => ({ path: f.path, folder: false })),
  ].sort((a, b) => {
    const ap = a.path.split("/"),
      bp = b.path.split("/");
    return (
      ap.slice(0, -1).join("/").localeCompare(bp.slice(0, -1).join("/")) ||
      Number(b.folder) - Number(a.folder) ||
      a.path.localeCompare(b.path)
    );
  });
  function tree(parent = "", depth = 0): React.ReactNode {
    const entries = treePaths.filter(
      (p) => p.path.split("/").slice(0, -1).join("/") === parent,
    );
    return entries.map((entry) => {
      const hasFilter =
        filter && !entry.path.toLowerCase().includes(filter.toLowerCase());
      if (hasFilter && !entry.folder) return null;
      const closed = collapsed.has(entry.path) && !filter;
      return (
        <React.Fragment key={entry.path}>
          <div
            className={`tree-entry ${selectedPath === entry.path ? "selected" : ""}`}
            style={{ paddingLeft: 12 + depth * 14 }}
            onContextMenu={(e) => {
              e.preventDefault();
              setSelectedPath(entry.path);
              setFileMenu(entry.path);
            }}
          >
            <button
              className="tree-label"
              onClick={() => {
                setSelectedPath(entry.path);
                if (entry.folder) {
                  const next = new Set(collapsed);
                  if (closed) next.delete(entry.path);
                  else next.add(entry.path);
                  setCollapsed(next);
                } else void open(entry.path).catch((e) => notify(String(e)));
              }}
            >
              {entry.folder ? (
                <>
                  {closed ? (
                    <ChevronRight size={13} />
                  ) : (
                    <ChevronDown size={13} />
                  )}
                  <Folder size={14} />
                </>
              ) : (
                <>
                  <span className="tree-spacer" />
                  <FileCode2 size={14} className="file-icon" />
                </>
              )}
              <span>{entry.path.split("/").at(-1)}</span>
            </button>
            <button
              className="row-menu"
              title={`Ações: ${entry.path}`}
              onClick={() => {
                setSelectedPath(entry.path);
                setFileMenu(fileMenu === entry.path ? "" : entry.path);
              }}
            >
              <MoreHorizontal size={14} />
            </button>
          </div>
          {fileMenu === entry.path && (
            <div className="file-menu">
              <button
                onClick={() => {
                  setModal({ type: "move", file: entry.path });
                  setFileMenu("");
                }}
              >
                Renomear / mover
              </button>
              <button
                onClick={() => {
                  setFileMenu("");
                  void removePath(entry.path);
                }}
              >
                Excluir
              </button>
            </div>
          )}
          {entry.folder && !closed && tree(entry.path, depth + 1)}
        </React.Fragment>
      );
    });
  }
  return (
    <div className="app-shell">
      <header className="titlebar">
        <div className="brand">
          <span className="brand-mark">L</span>
          <strong>Luvyn</strong>
          <span className="version">0.1</span>
        </div>
        <button
          title="Trocar projeto"
          className="workspace-command"
          onClick={() => {
            window.dispatchEvent(new Event("luvyn-projects-request"));
          }}
        >
          <Search size={14} />
          <span>
            {snapshot?.workspace.split(/[\\/]/).at(-1) ||
              "Conectando workspace…"}
          </span>
          <ChevronDown size={14} />
        </button>
        <div className="title-actions">
          {currentIsCloud && (
            <button
              title="Sincronizar Cloud"
              onClick={() => {
                if (tabsRef.current.some((tab) => tab.dirty)) {
                  notify("Salve alterações antes de sincronizar.");
                  return;
                }
                void onSync()
                  .then(() => notify("Cloud sincronizado"))
                  .catch((error) => notify(String(error)));
              }}
            >
              <RefreshCw size={16} />
            </button>
          )}
          <button
            title="Alternar tema"
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
          >
            {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          </button>
          <button
            title="Command palette"
            onClick={() => {
              setPalette({ mode: "commands", query: "" });
              setPaletteIndex(0);
            }}
          >
            <Command size={17} />
          </button>
        </div>
      </header>
      <div className="ide-body">
        <nav className="activity" aria-label="Atividades">
          {[
            { id: "files", icon: Files, title: "Explorer" },
            { id: "search", icon: Search, title: "Busca" },
            { id: "graph", icon: Network, title: "Grafo" },
            { id: "language", icon: BookOpen, title: "Linguagem Luvyn" },
          ].map((a) => (
            <button
              key={a.id}
              title={a.title}
              className={section === a.id ? "active" : ""}
              onClick={() => {
                setSection(a.id);
                if (a.id === "graph") {
                  setMode("graph");
                  setGraphFocus("");
                }
                if (a.id === "language") setMode("language");
              }}
            >
              <a.icon size={21} />
            </button>
          ))}
          <div className="activity-spacer" />
          <button
            title="Configurações"
            className={section === "settings" ? "active" : ""}
            onClick={() => setSection("settings")}
          >
            <Settings size={21} />
          </button>
        </nav>
        <aside className="sidebar">
          <div className="pane-title">
            <span>
              {section === "files"
                ? "Explorer"
                : section === "search"
                  ? "Busca"
                  : section === "language"
                    ? "Language"
                    : section === "settings"
                      ? "Preferências"
                      : "Grafo semântico"}
            </span>
            <div>
              {section === "files" && (
                <>
                  <button
                    title="Novo documento"
                    onClick={() => setModal({ type: "file" })}
                  >
                    <FilePlus size={15} />
                  </button>
                  <button
                    title="Nova pasta"
                    onClick={() => setModal({ type: "folder" })}
                  >
                    <FolderPlus size={15} />
                  </button>
                  <button title="Atualizar" onClick={() => void refresh()}>
                    <RefreshCw size={14} />
                  </button>
                </>
              )}
            </div>
          </div>
          {section === "files" && (
            <>
              <div className="workspace-heading">
                <ChevronDown size={13} />
                <strong>
                  {snapshot?.workspace.split(/[\\/]/).at(-1) || "Workspace"}
                </strong>
                <small>{snapshot?.files.length || 0}</small>
              </div>
              <div className="filter-field">
                <Search size={13} />
                <input
                  aria-label="Filtrar arquivos"
                  placeholder="Filtrar documentos"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              </div>
              <div className="file-tree">
                {tree()}
                {!snapshot?.files.length && (
                  <div className="sidebar-empty">
                    <p>Documente sua primeira ideia.</p>
                    <button onClick={() => setModal({ type: "file" })}>
                      <Plus size={14} />
                      Novo .lyn
                    </button>
                  </div>
                )}
              </div>
              <div className="sidebar-footer">
                <span className="kind-dot service" />
                .lyn fonte <span className="kind-dot interface" />
                .lu grafo
              </div>
            </>
          )}
          {section === "search" && (
            <div className="search-pane">
              <div className="segmented">
                <button
                  className={searchKind === "text" ? "active" : ""}
                  onClick={() => setSearchKind("text")}
                >
                  Texto
                </button>
                <button
                  className={searchKind === "graph" ? "active" : ""}
                  onClick={() => setSearchKind("graph")}
                >
                  Semântica
                </button>
              </div>
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void doSearch();
                }}
              >
                <input
                  aria-label="Busca no workspace"
                  onKeyDown={(e) => {
                    if (e.key === "Escape") clearSearch();
                  }}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={
                    searchKind === "text"
                      ? "Texto no workspace…"
                      : "quem depende de UserRepository"
                  }
                />
                <button
                  type="button"
                  aria-label="Limpar busca"
                  onClick={clearSearch}
                  disabled={!search && !searching}
                >
                  <X size={15} />
                </button>
                <button
                  type="submit"
                  aria-label="Executar busca"
                  disabled={searching}
                >
                  <Search size={15} />
                </button>
              </form>
              <small>{results.length} resultados</small>
              <div className="search-results">
                {results.map((result, i) =>
                  searchKind === "text" ? (
                    <button
                      key={i}
                      onClick={() =>
                        void open(
                          result.file,
                          result.line,
                          result.column,
                        ).catch((e) => notify(String(e)))
                      }
                    >
                      <b>
                        {result.file}:{result.line}
                      </b>
                      <span>{result.text}</span>
                    </button>
                  ) : (
                    <button key={result.id} onClick={() => openSymbol(result)}>
                      <b>{result.name}</b>
                      <span>
                        {result.kind} · {result.qualified}
                      </span>
                    </button>
                  ),
                )}
              </div>
              {context && <pre className="query-context">{context}</pre>}
            </div>
          )}
          {section === "graph" && (
            <div className="graph-sidebar">
              <p>Documentação conectada.</p>
              <div className="graph-counts">
                <strong>
                  {snapshot?.symbol_count || 0}
                  <small>símbolos</small>
                </strong>
                <strong>
                  {snapshot?.edge_count || 0}
                  <small>relações</small>
                </strong>
              </div>
              <button
                onClick={() => {
                  setMode("graph");
                  setGraphFocus("");
                }}
              >
                <Network size={16} />
                Explorar grafo
              </button>
              <input
                placeholder="Encontrar símbolo…"
                aria-label="Filtrar símbolos"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
              <div className="symbol-list">
                {snapshot?.symbols
                  .filter((s) =>
                    s.qualified.toLowerCase().includes(filter.toLowerCase()),
                  )
                  .slice(0, 200)
                  .map((s) => (
                    <button
                      key={s.id}
                      onClick={() => {
                        setGraphFocus(s.qualified);
                        setMode("graph");
                      }}
                    >
                      <span className={`kind-dot ${s.kind}`} />
                      <span>
                        {s.name}
                        <small>{s.kind}</small>
                      </span>
                    </button>
                  ))}
              </div>
            </div>
          )}
          {section === "language" && (
            <LanguageSidebar
              entries={dictionary.entries}
              error={dictionary.error}
              selected={languageKeyword}
              select={(k) => {
                setLanguageKeyword(k);
                setMode("language");
              }}
            />
          )}
          {section === "settings" && (
            <div className="settings-pane">
              <label>
                <input
                  type="checkbox"
                  checked={autosave}
                  onChange={(e) => setAutosave(e.target.checked)}
                />
                Autosave após edição
              </label>
              <label>
                Tema
                <select
                  value={theme}
                  onChange={(e) => setTheme(e.target.value)}
                >
                  <option value="dark">Escuro</option>
                  <option value="light">Claro</option>
                </select>
              </label>
              <p>
                Configuração do projeto: luvyn.toml. Atalhos e navegação pelo
                Command Palette.
              </p>
              <button
                onClick={() => {
                  void api("ignore-config")
                    .then(() => open(".ignore.luvyn"))
                    .catch((e) => notify(String(e)));
                }}
              >
                Editar .ignore.luvyn
              </button>
              <p>
                Ctrl+S salvar
                <br />
                Ctrl+P símbolos
                <br />
                Ctrl+Shift+P comandos
                <br />
                F12 definição
                <br />
                Shift+F12 referências
                <br />
                F2 renomear
                <br />
                Ctrl+F buscar
                <br />
                Ctrl+H substituir
                <br />
                Shift+Alt+F formatar
              </p>
            </div>
          )}
        </aside>
        <div
          className="resizer vertical"
          onPointerDown={(e) => resize(e, "--sidebar-width", "x")}
        />
        <main className="main-pane">
          <div className="tabs-row">
            <div className="tabs">
              {tabs.map((tab) => (
                <div
                  key={tab.file}
                  className={`tab ${active === tab.file && mode === "editor" ? "active" : ""}`}
                >
                  <button onClick={() => activate(tab.file)} title={tab.file}>
                    <FileCode2 size={14} />
                    <span>{tab.file.split("/").at(-1)}</span>
                    {tab.conflict ? (
                      <AlertTriangle size={12} />
                    ) : tab.dirty ? (
                      <span className="dirty-dot" />
                    ) : null}
                  </button>
                  <button
                    title="Fechar aba"
                    onClick={() =>
                      void close(tab.file).catch((e) => notify(String(e)))
                    }
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
              {mode === "language" && (
                <div className="tab active">
                  <button>
                    <BookOpen size={14} />
                    Language
                  </button>
                  <button
                    title="Fechar linguagem"
                    onClick={() => setMode("editor")}
                  >
                    <X size={13} />
                  </button>
                </div>
              )}
              {mode === "graph" && (
                <div className="tab active">
                  <button>
                    <Network size={14} />
                    Grafo
                  </button>
                  <button
                    title="Fechar grafo"
                    onClick={() => setMode("editor")}
                  >
                    <X size={13} />
                  </button>
                </div>
              )}
            </div>
            <div className="editor-actions">
              <button title="Salvar" onClick={() => void save()}>
                <Save size={15} />
              </button>
              <button
                title="Grafo do símbolo atual"
                onClick={() => {
                  setGraphFocus(
                    outline.find((s) => !s.parent)?.qualified || "",
                  );
                  setMode(mode === "graph" ? "editor" : "graph");
                }}
              >
                <Network size={15} />
              </button>
            </div>
          </div>
          <div className="editor-breadcrumb">
            <div>
              {mode === "language" ? (
                <>
                  <BookOpen size={13} />
                  Language / {languageKeyword}
                </>
              ) : mode === "graph" ? (
                <>
                  <Network size={13} />
                  Grafo do workspace
                </>
              ) : (
                <>
                  <Folder size={13} />
                  {active.split("/").join(" / ")}
                  {outline.find(
                    (s) =>
                      s.location.line <= position.lineNumber &&
                      s.end_line >= position.lineNumber,
                  ) && (
                    <>
                      <ChevronRight size={13} />
                      <Braces size={13} />
                      {
                        outline.find(
                          (s) =>
                            s.location.line <= position.lineNumber &&
                            s.end_line >= position.lineNumber,
                        )?.name
                      }
                    </>
                  )}
                </>
              )}
            </div>
            <div className="build-actions">
              <button disabled={!!busy} onClick={() => void run("check")}>
                <Check size={13} />
                Check
              </button>
              <button disabled={!!busy} onClick={() => void run("build")}>
                <Play size={12} />
                {busy === "build" ? "Compilando…" : "Build"}
              </button>
            </div>
          </div>
          {activeTab?.conflict && mode === "editor" && (
            <div className="conflict-banner">
              <AlertTriangle size={14} />
              Arquivo alterado fora da IDE.
              <button onClick={() => void compareConflict()}>
                Revisar conflito
              </button>
              <button
                onClick={() => void reload().catch((e) => notify(String(e)))}
              >
                Recarregar
              </button>
            </div>
          )}
          <div className="workspace-center">
            <div
              className={`editor-host ${mode !== "editor" ? "hidden" : ""}`}
              ref={editorHost}
            />
            {mode === "editor" && !active && (
              <div className="welcome">
                <span className="welcome-mark">L</span>
                <h1>Documentação com estrutura.</h1>
                <p>
                  Escreva intenção. Conecte símbolos.
                  <br />
                  Compile contexto para agentes.
                </p>
                <button onClick={() => setModal({ type: "file" })}>
                  <Plus size={16} />
                  Criar documento .lyn
                </button>
                <button
                  className="secondary"
                  onClick={() => {
                    setPalette({ mode: "commands", query: "" });
                    setPaletteIndex(0);
                  }}
                >
                  <Command size={15} />
                  Abrir comandos<kbd>Ctrl Shift P</kbd>
                </button>
                <div className="welcome-example">
                  <code>
                    class UserService
                    <br />
                    <span>purpose:</span>
                    <br />
                    &nbsp;&nbsp;&nbsp;&nbsp;gerenciar usuários
                    <br />
                    <span>depends:</span>
                    <br />
                    &nbsp;&nbsp;&nbsp;&nbsp;UserRepository
                  </code>
                </div>
                <small>.lyn editável · .lu compilado · contexto preciso</small>
              </div>
            )}
            {mode === "language" && (
              <LanguageDetails
                entry={dictionary.entries.find(
                  (e) => e.keyword === languageKeyword,
                )}
                select={setLanguageKeyword}
              />
            )}
            {mode === "graph" && (
              <Graph
                open={openSymbol}
                focus={graphFocus}
                theme={theme}
                revision={snapshot?.revision || 0}
              />
            )}
          </div>
          {panel && (
            <>
              <div
                className="resizer horizontal"
                onPointerDown={(e) => resize(e, "--panel-height", "y", true)}
              />
              <section className="bottom-panel">
                <div className="panel-heading">
                  <div>
                    <button
                      className={panelTab === "problems" ? "active" : ""}
                      onClick={() => setPanelTab("problems")}
                    >
                      Problems <span>{problems.length}</span>
                    </button>
                    <button
                      className={panelTab === "output" ? "active" : ""}
                      onClick={() => setPanelTab("output")}
                    >
                      Output
                    </button>
                  </div>
                  <button
                    title="Ocultar painel"
                    onClick={() => setPanel(false)}
                  >
                    <X size={14} />
                  </button>
                </div>
                {panelTab === "problems" ? (
                  <div className="problems-list">
                    {problems.length ? (
                      problems.map((d, i) => (
                        <button
                          key={i}
                          onClick={() =>
                            void open(
                              d.diagnostic.location.file,
                              d.range.startLineNumber,
                              d.range.startColumn,
                            ).catch((e) => notify(String(e)))
                          }
                        >
                          <AlertTriangle
                            size={13}
                            className={d.diagnostic.severity}
                          />
                          <span>{d.diagnostic.message}</span>
                          <code>{d.diagnostic.code}</code>
                          <small>
                            {d.diagnostic.location.file}:
                            {d.diagnostic.location.line}
                          </small>
                        </button>
                      ))
                    ) : (
                      <div className="no-problems">
                        <CheckCircle2 size={15} />
                        Sem problemas detectados.
                      </div>
                    )}
                  </div>
                ) : (
                  <pre className="output-log">
                    {logs.join("\n") || "Build, check e export aparecem aqui."}
                  </pre>
                )}
              </section>
            </>
          )}
        </main>
        <div
          className="resizer vertical"
          onPointerDown={(e) => resize(e, "--outline-width", "x", true)}
        />
        <aside className="outline">
          <div className="pane-title">
            <span>Outline</span>
            <Braces size={14} />
          </div>
          <div className="outline-list">
            {outline.map((s) => (
              <button
                key={s.id}
                onClick={() => openSymbol(s)}
                style={{ paddingLeft: s.parent ? 28 : 14 }}
              >
                <span className={`kind-letter ${s.kind}`}>
                  {s.kind === "func" ? "ƒ" : s.kind[0].toUpperCase()}
                </span>
                <span>{s.name}</span>
              </button>
            ))}
            {!outline.length && <p>Selecione um documento.</p>}
          </div>
          <div className="outline-details">
            <h4>Contexto conectado</h4>
            <p>{snapshot?.edge_count || 0} relações indexadas.</p>
            <button
              onClick={() => {
                setMode("graph");
                setGraphFocus(outline.find((s) => !s.parent)?.qualified || "");
              }}
            >
              <Network size={14} />
              Ver vizinhança
            </button>
          </div>
        </aside>
      </div>
      <footer className="statusbar">
        <div>
          <span className="status-brand">L</span>
          <GitBranch size={13} />
          <span
            title={`Documentation: ${snapshot?.documentation_root}\nTarget: ${snapshot?.target_project_root}`}
          >
            Docs: {snapshot?.workspace.split(/[\\/]/).at(-1)} · Target:{" "}
            {snapshot?.target_project_root.split(/[\\/]/).at(-1)}
          </span>
          <button
            onClick={() => {
              setPanel(true);
              setPanelTab("problems");
            }}
          >
            <AlertTriangle size={12} />
            {errors}
            <span className="warnings">{problems.length - errors} avisos</span>
          </button>
        </div>
        <div>
          <span>
            {busy ? `${busy}…` : `${snapshot?.symbol_count || 0} símbolos`}
          </span>
          <button onClick={() => setAutosave((a) => !a)}>
            Autosave {autosave ? "on" : "off"}
          </button>
          <span>
            Ln {position.lineNumber}, Col {position.column}
          </span>
          <span>UTF-8</span>
          <span>Luvyn</span>
          <button title="Alternar painel" onClick={() => setPanel((p) => !p)}>
            <PanelBottom size={14} />
          </button>
        </div>
      </footer>
      {toast && (
        <div className="toast" role="status">
          {toast}
          <button title="Fechar mensagem" onClick={() => setToast("")}>
            <X size={14} />
          </button>
        </div>
      )}
      {palette && (
        <div className="modal-overlay" onMouseDown={() => setPalette(null)}>
          <div
            className="palette"
            role="dialog"
            aria-label="Command Palette"
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="palette-input">
              <Search size={17} />
              <input
                autoFocus
                value={palette.query}
                placeholder={
                  palette.mode === "commands"
                    ? "Executar comando…"
                    : "Encontrar símbolo…"
                }
                onChange={(e) => {
                  setPalette({ ...palette, query: e.target.value });
                  setPaletteIndex(0);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setPalette(null);
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setPaletteIndex((i) =>
                      Math.min(i + 1, paletteItems.length - 1),
                    );
                  }
                  if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setPaletteIndex((i) => Math.max(i - 1, 0));
                  }
                  if (e.key === "Enter" && paletteItems[paletteIndex]) {
                    const item = paletteItems[paletteIndex];
                    setPalette(null);
                    Promise.resolve(item.execute()).catch((e) =>
                      notify(String(e)),
                    );
                  }
                }}
              />
              <kbd>Esc</kbd>
            </div>
            <div className="palette-results">
              {paletteItems.slice(0, 100).map((item, i) => (
                <button
                  key={item.label}
                  className={i === paletteIndex ? "active" : ""}
                  onMouseEnter={() => setPaletteIndex(i)}
                  onClick={() => {
                    setPalette(null);
                    Promise.resolve(item.execute()).catch((e) =>
                      notify(String(e)),
                    );
                  }}
                >
                  <span>{item.label}</span>
                  <small>{item.detail}</small>
                  <kbd>{item.shortcut}</kbd>
                </button>
              ))}
              {!paletteItems.length && <p>Nenhum resultado.</p>}
            </div>
          </div>
        </div>
      )}
      {modal && (
        <FileModal
          modal={modal}
          selected={selectedPath}
          roots={snapshot?.source_roots || ["."]}
          entries={dictionary.entries}
          close={() => setModal(null)}
          submit={async (file, text) => {
            try {
              if (modal.type === "move") {
                await api("move", { file: modal.file, to: file });
                for (const tab of [...tabsRef.current])
                  if (
                    tab.file === modal.file ||
                    tab.file.startsWith(`${modal.file}/`)
                  )
                    await close(tab.file);
              } else {
                const created = await api(
                  modal.type === "folder" ? "mkdir" : "create",
                  { file, text },
                );
                if (
                  modal.type === "file" &&
                  (typeof created.hash !== "string" || created.file !== file)
                )
                  throw new Error(
                    "Backend did not confirm filesystem creation",
                  );
              }
              setModal(null);
              await refresh();
              if (modal.type !== "folder") await open(file);
            } catch (e) {
              notify(String(e));
            }
          }}
        />
      )}
      {conflict && (
        <ConflictModal
          conflict={conflict}
          close={() => setConflict(null)}
          resolve={async (text) => {
            const tab = tabsRef.current.find((t) => t.file === conflict.file);
            if (!tab) return;
            tab.hash = conflict.hash;
            tab.conflict = false;
            tab.model.pushEditOperations(
              [],
              [{ range: tab.model.getFullModelRange(), text }],
              () => null,
            );
            tab.dirty = true;
            touch();
            setConflict(null);
            await sync(tab.model);
            notify("Conflito revisado. Salve para gravar a versão escolhida.");
          }}
        />
      )}
    </div>
  );
}

function FileModal({
  modal,
  selected,
  roots,
  entries,
  close,
  submit,
}: {
  modal: Modal;
  selected: string;
  roots: string[];
  entries: LanguageEntry[];
  close: () => void;
  submit: (file: string, text: string) => Promise<void>;
}) {
  const selectedParent = selected.endsWith(".lyn")
    ? selected.split("/").slice(0, -1).join("/")
    : selected;
  const parent =
    selectedParent &&
    roots.some(
      (root) =>
        root === "." ||
        selectedParent === root ||
        selectedParent.startsWith(`${root}/`),
    )
      ? selectedParent
      : roots.find((root) => root !== ".") || "";
  const [submitting, setSubmitting] = useState(false);
  const [path, setPath] = useState(
      modal.type === "move"
        ? modal.file || ""
        : `${parent ? `${parent}/` : ""}${modal.type === "file" ? "Untitled.lyn" : "new-folder"}`,
    ),
    [template, setTemplate] = useState("blank");
  const templates = [
    "blank",
    ...entries
      .filter((e) => e.category === "Declarations")
      .map((e) => e.keyword),
  ];
  const name =
    path
      .split("/")
      .at(-1)
      ?.replace(/\.lyn$/, "")
      .replace(/[^\p{L}\p{N}_]/gu, "") || "NewSymbol";
  return (
    <div className="modal-overlay">
      <form
        className="file-dialog"
        role="dialog"
        aria-label="Criar ou mover arquivo"
        onSubmit={async (e) => {
          e.preventDefault();
          if (submitting) return;
          setSubmitting(true);
          const snippet =
            entries.find((entry) => entry.keyword === template)?.completion ||
            "";
          const text =
            template === "blank"
              ? ""
              : snippet.replace(
                  /\$\{(\d+)(?::([^}]*))?\}/g,
                  (_all, index, value) => (index === "1" ? name : value || ""),
                ) + "\n";
          try {
            await submit(path, text);
          } finally {
            setSubmitting(false);
          }
        }}
      >
        <h2>
          {modal.type === "move"
            ? "Renomear / mover"
            : modal.type === "folder"
              ? "Nova pasta"
              : "Novo documento"}
        </h2>
        <label>
          Caminho no workspace
          <input
            autoFocus
            required
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && !submitting) close();
            }}
          />
        </label>
        {modal.type === "file" && (
          <label>
            Template opcional
            <select
              value={template}
              onChange={(e) => setTemplate(e.target.value)}
            >
              {templates.map((t) => (
                <option key={t} value={t}>
                  {t === "blank" ? "Documento vazio" : t}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="dialog-actions">
          <button type="button" disabled={submitting} onClick={close}>
            Cancelar
          </button>
          <button type="submit" className="primary" disabled={submitting}>
            {modal.type === "move" ? "Mover" : "Criar"}
          </button>
        </div>
      </form>
    </div>
  );
}
function ConflictModal({
  conflict,
  close,
  resolve,
}: {
  conflict: { file: string; local: string; disk: string };
  close: () => void;
  resolve: (text: string) => Promise<void>;
}) {
  const host = useRef<HTMLDivElement>(null),
    diff = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);
  useEffect(() => {
    if (!host.current) return;
    const original = monaco.editor.createModel(conflict.disk, "lyn"),
      modified = monaco.editor.createModel(conflict.local, "lyn");
    diff.current = monaco.editor.createDiffEditor(host.current, {
      automaticLayout: true,
      readOnly: false,
      originalEditable: false,
      minimap: { enabled: false },
      fontSize: 13,
    });
    diff.current.setModel({ original, modified });
    return () => {
      diff.current?.dispose();
      original.dispose();
      modified.dispose();
    };
  }, []);
  return (
    <div className="modal-overlay">
      <div
        className="conflict-dialog"
        role="dialog"
        aria-label="Revisar conflito"
      >
        <h2>{conflict.file}</h2>
        <p>Disco à esquerda. Versão local à direita, editável.</p>
        <div className="diff-host" ref={host} />
        <div className="dialog-actions">
          <button onClick={close}>Cancelar</button>
          <button
            onClick={() =>
              void resolve(diff.current?.getModel()?.modified.getValue() || "")
            }
          >
            Usar versão revisada
          </button>
        </div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <ProjectsGate>
    {(back, sync, currentIsCloud) => (
      <App onProjects={back} onSync={sync} currentIsCloud={currentIsCloud} />
    )}
  </ProjectsGate>,
);
