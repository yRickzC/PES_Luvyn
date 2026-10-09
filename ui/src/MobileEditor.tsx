import {useEffect, useRef} from "react";
import {EditorState, StateEffect, StateField} from "@codemirror/state";
import {EditorView, Decoration, type DecorationSet, keymap, lineNumbers, highlightActiveLine, drawSelection} from "@codemirror/view";
import {history, historyKeymap, defaultKeymap, indentWithTab, undo, redo} from "@codemirror/commands";
import {autocompletion, completionKeymap, startCompletion} from "@codemirror/autocomplete";
import {setDiagnostics, lintGutter} from "@codemirror/lint";
import {searchKeymap, openSearchPanel} from "@codemirror/search";
import {api, type Snapshot, type Symbol} from "./api";

const highlights = StateEffect.define<DecorationSet>();
const colors = StateField.define<DecorationSet>({create: () => Decoration.none, update: (value, transaction) => {
  value = value.map(transaction.changes);
  for (const effect of transaction.effects) if (effect.is(highlights)) value = effect.value;
  return value;
}, provide: field => EditorView.decorations.from(field)});
export type TouchEditor = {view: EditorView; text: () => string};
export function MobileEditor({file, initial, opened, changed, analyzed, ready, symbols}: {
  file: string; initial: string; opened: number; changed: (text: string) => void;
  analyzed: (snapshot: Snapshot) => void; ready: (editor: TouchEditor | null) => void; symbols: Symbol[];
}) {
  const host = useRef<HTMLDivElement>(null), viewRef = useRef<EditorView | null>(null);
  const callbacks = useRef({changed, analyzed, ready}); callbacks.current = {changed, analyzed, ready};
  useEffect(() => {
    if (!host.current || !file) return;
    let timer: ReturnType<typeof setTimeout> | undefined, disposed = false;
    let generation = 0;
    const analyze = async () => {
      const version = ++generation, text = view.state.doc.toString();
      try {
        await api("edit", {file, text});
        const [snapshot, lexical] = await Promise.all([api<Snapshot>("snapshot"), api("tokens", {text})]);
        if (disposed || version !== generation || text !== view.state.doc.toString()) return;
        const decorations = lexical.tokens.filter((t: any) => t.from < t.to && t.to <= view.state.doc.length).map((t: any) => Decoration.mark({class: `lyn-${t.kind}`}).range(t.from, t.to));
        view.dispatch({effects: highlights.of(Decoration.set(decorations, true))});
        const diagnostics = snapshot.diagnostics.filter(d => d.diagnostic.location.file === file).map(d => {
          const line = view.state.doc.line(Math.min(d.range.startLineNumber, view.state.doc.lines));
          return {from: Math.min(line.to, line.from + d.range.startColumn - 1), to: Math.min(line.to, line.from + d.range.endColumn - 1), severity: d.diagnostic.severity === "error" ? "error" as const : "warning" as const, message: `${d.diagnostic.code}: ${d.diagnostic.message}`};
        });
        view.dispatch(setDiagnostics(view.state, diagnostics));
        callbacks.current.analyzed(snapshot);
      } catch (error) { if (!disposed) window.dispatchEvent(new CustomEvent("luvyn-error", {detail: String(error)})); }
    };
    const view = new EditorView({parent: host.current, state: EditorState.create({doc: initial, extensions: [
      lineNumbers(), history(), drawSelection(), highlightActiveLine(), colors, lintGutter(),
      keymap.of([...defaultKeymap, ...historyKeymap, ...completionKeymap, ...searchKeymap, indentWithTab]),
      EditorView.lineWrapping,
      autocompletion({activateOnTyping: true, maxRenderedOptions: 80, override: [async context => {
        const prefix = context.matchBefore(/[\w.]*$/); if (!prefix) return null;
        const before = context.state.doc.sliceString(Math.max(0, context.pos - 80), context.pos);
        const sectionContext = /(?:^|\n)[ \t]*(?:fields|depends|main|rules|export|implements):[ \t]*(?:\n[ \t]*)*$/.test(before);
        const valueContext = /->\s*$|[A-Za-z_]\w*:\s*$/.test(before);
        if (!context.explicit && !prefix.text && !sectionContext && !valueContext) return null;
        const text = context.state.doc.toString(), line = context.state.doc.lineAt(context.pos);
        await api("edit", {file, text});
        const data = await api("completion", {file, line: line.number, column: context.pos - line.from + 1});
        const self = prefix.text.startsWith("self.");
        return {from: self ? prefix.from + 5 : prefix.from, validFor: /^[\w.]*$/, options: data.items.map((item: any) => ({label: item.label, detail: item.detail, type: self ? "property" : "text", apply: (editor: EditorView, _completion: unknown, from: number, to: number) => {
          const insert = item.insert.replace(/\$\{\d+:([^}]+)\}/g, "$1").replace(/\$\{\d+\}/g, "");
          const changes: {from: number; to?: number; insert: string}[] = [{from, to, insert}];
          if (item.import) {const at = editor.state.doc.line(Math.min(item.import.range.startLineNumber, editor.state.doc.lines)); changes.push({from: at.from, insert: item.import.text});}
          editor.dispatch({changes}); editor.focus();
        }}))};
      }]}),
      EditorView.updateListener.of(update => {if (update.docChanged) {
        callbacks.current.changed(update.state.doc.toString()); clearTimeout(timer); timer = setTimeout(() => void analyze(), 350);
        const position = update.state.selection.main.head;
        const before = update.state.doc.sliceString(Math.max(0, position - 80), position);
        const context = /(?:^|\n)[ \t]*(?:fields|depends|main|rules|export|implements):[ \t]*(?:\n[ \t]*)*$/.test(before) || /(?:->|[A-Za-z_]\w*:)[ \t]*$/.test(before);
        if (context) setTimeout(() => {if (viewRef.current === update.view && update.view.hasFocus) startCompletion(update.view);}, 0);
      }}),
      EditorView.theme({"&": {height: "100%", fontSize: "15px", backgroundColor: "#181c29", color: "#ccd3e5"}, ".cm-scroller": {overflow: "auto", fontFamily: "Consolas, monospace"}, ".cm-content": {paddingBottom: "80px"}, ".cm-gutters": {backgroundColor: "#181c29", color: "#77839c", border: "none"}, ".cm-cursor": {borderLeftColor: "#b5a9ff"}}, {dark: true})
    ]})});
    viewRef.current = view; callbacks.current.ready({view, text: () => view.state.doc.toString()});
    void analyze();
    const viewport = () => {const height = window.visualViewport?.height; if (height) document.documentElement.style.setProperty("--phone-height", `${height}px`); view.requestMeasure(); const position = view.state.selection.main.head; view.dispatch({effects: EditorView.scrollIntoView(position, {y: "nearest"})});};
    window.visualViewport?.addEventListener("resize", viewport); viewport();
    return () => {disposed = true; clearTimeout(timer); window.visualViewport?.removeEventListener("resize", viewport); callbacks.current.ready(null); viewRef.current = null; view.destroy();};
  }, [file, opened]);
  return <div className="touch-editor">
    <div className="touch-tools">
      <button aria-label="Undo" onClick={() => viewRef.current && undo(viewRef.current)}>Undo</button>
      <button aria-label="Redo" onClick={() => viewRef.current && redo(viewRef.current)}>Redo</button>
      <button onClick={() => viewRef.current && openSearchPanel(viewRef.current)}>Buscar</button>
      <button onClick={() => viewRef.current && startCompletion(viewRef.current)}>Completar</button>
      <select aria-label="Símbolos" value="" onChange={e => {const symbol = symbols.find(s => s.id === e.target.value); if (symbol && viewRef.current) { const pos = viewRef.current.state.doc.line(symbol.location.line).from; viewRef.current.dispatch({selection: {anchor: pos}, effects: EditorView.scrollIntoView(pos)}); }}}><option value="">Símbolos</option>{symbols.filter(s => s.kind !== "module" && s.location.file === file).map(s => <option key={s.id} value={s.id}>{s.kind} {s.name}</option>)}</select>
    </div>
    <div className="touch-editor-host" ref={host}/>
  </div>;
}
