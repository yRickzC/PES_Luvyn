import * as monaco from "monaco-editor/editor/editor.api";
import "monaco-editor/features/register.all";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import { api, type Edit, type Symbol, type Range } from "./api";
export { monaco };
(self as any).MonacoEnvironment = { getWorker: () => new EditorWorker() };
export const fileOf = (model: monaco.editor.ITextModel) =>
  model.uri.path.slice(1);
export const uri = (file: string) =>
  monaco.Uri.from({ scheme: "luvyn", path: `/${file}` });
export let bridge: {
  open: (
    file: string,
    line?: number,
    column?: number,
    reveal?: boolean,
  ) => Promise<monaco.editor.ITextModel>;
  apply: (edits: Edit[]) => Promise<void>;
  sync: (model: monaco.editor.ITextModel) => Promise<void>;
  report: (message: string) => void;
};
export function setBridge(value: typeof bridge) {
  bridge = value;
}
let registered = false;
export function registerLanguage() {
  if (registered) return;
  registered = true;
  monaco.languages.register({ id: "lyn", extensions: [".lyn"] });
  monaco.languages.setLanguageConfiguration("lyn", {
    comments: { lineComment: "#" },
    brackets: [
      ["(", ")"],
      ["[", "]"],
      // Angle operators are validated by Core, not Monaco's bracket scanner:
      // -> contains a > that is not a closing generic bracket.
    ],
    autoClosingPairs: [
      { open: "(", close: ")" },
      { open: "[", close: "]" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    surroundingPairs: [
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    indentationRules: {
      increaseIndentPattern: /^\s*[a-z]+:\s*$/,
      decreaseIndentPattern: /^\S/,
    },
    wordPattern: /[\p{L}_][\p{L}\p{N}_.:]*/u,
  });
  const configureTokens = (
    entries: { keyword: string; category: string; aliases: string[] }[],
  ) =>
    monaco.languages.setMonarchTokensProvider("lyn", {
      keywords: entries
        .filter((e) => !e.category.endsWith("Types"))
        .flatMap((e) => [e.keyword, ...e.aliases]),
      types: entries
        .filter((e) => e.category.endsWith("Types"))
        .map((e) => e.keyword),
      tokenizer: {
        root: [
          [/#.*$|\/\/.*$/, "comment"],
          [/^\s*[a-z]+(?=:)/, "type.identifier"],
          [/"[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*'/, "string"],
          [
            /[a-zA-Z_][\w]*/,
            {
              cases: {
                "@keywords": "keyword",
                "@types": "type",
                "@default": "identifier",
              },
            },
          ],
          [/@[a-zA-Z_][\w.]*/, "annotation"],
          [/->|::|>>|[.:?<>|=]/, "operator"],
          [/[()[\]]/, "delimiter"],
        ],
      },
    });
  configureTokens([]);
  void api("language")
    .then((data) => configureTokens(data.entries))
    .catch((e) => console.error(e));
  monaco.editor.defineTheme("luvyn-dark", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "keyword", foreground: "AFA6F8" },
      { token: "annotation", foreground: "D3B781" },
      { token: "type", foreground: "86BCD9" },
      { token: "type.identifier", foreground: "92A7D2" },
      { token: "comment", foreground: "717D98" },
      { token: "string", foreground: "B6CA9E" },
      { token: "operator", foreground: "ADB7CF" },
    ],
    colors: {
      "editor.background": "#181C29",
      "editor.foreground": "#CCD3E5",
      "editorLineNumber.foreground": "#58627B",
      "editorLineNumber.activeForeground": "#BAC4DC",
      "editor.lineHighlightBackground": "#212737",
      "editor.selectionBackground": "#3A456A",
      "editorCursor.foreground": "#B5A9FF",
      "editorIndentGuide.background1": "#2A3042",
      "editorWidget.background": "#252B3E",
      "editorWidget.border": "#434C6A",
      "editorGutter.background": "#181C29",
      "editorHoverWidget.background": "#252B3E",
    },
  });
  monaco.editor.defineTheme("luvyn-light", {
    base: "vs",
    inherit: true,
    rules: [
      { token: "keyword", foreground: "6C4FBC" },
      { token: "type", foreground: "236788" },
    ],
    colors: {
      "editor.background": "#F8F9FC",
      "editor.foreground": "#283248",
      "editor.lineHighlightBackground": "#EDF0F7",
    },
  });
  monaco.languages.registerCompletionItemProvider("lyn", {
    triggerCharacters: [".", ":"],
    provideCompletionItems: async (model, position) => {
      await bridge.sync(model);
      const { items } = await api("completion", {
        file: fileOf(model),
        line: position.lineNumber,
        column: position.column,
      });
      const word = model.getWordUntilPosition(position);
      return {
        suggestions: items.map((item: any) => ({
          label: item.label,
          detail: item.detail,
          kind: item.snippet
            ? monaco.languages.CompletionItemKind.Snippet
            : word.word.startsWith("self.")
              ? monaco.languages.CompletionItemKind.Field
              : monaco.languages.CompletionItemKind.Class,
          insertText: item.insert,
          insertTextRules: item.snippet
            ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
            : undefined,
          range: {
            startLineNumber: position.lineNumber,
            endLineNumber: position.lineNumber,
            startColumn: word.word.startsWith("self.")
              ? word.startColumn + 5
              : word.startColumn,
            endColumn: word.endColumn,
          },
          additionalTextEdits: item.import
            ? [{ range: item.import.range, text: item.import.text }]
            : undefined,
        })),
      };
    },
  });
  async function symbol(
    model: monaco.editor.ITextModel,
    position: monaco.Position,
  ) {
    await bridge.sync(model);
    return api("symbol", {
      file: fileOf(model),
      line: position.lineNumber,
      column: position.column,
    });
  }
  monaco.languages.registerHoverProvider("lyn", {
    provideHover: async (model, position) => {
      try {
        const data = await symbol(model, position);
        return { contents: [{ value: "```lyn\n" + data.context + "\n```" }] };
      } catch {
        return null;
      }
    },
  });
  monaco.languages.registerDefinitionProvider("lyn", {
    provideDefinition: async (model, position) => {
      try {
        const data = await symbol(model, position);
        await bridge.open(
          data.symbol.location.file,
          undefined,
          undefined,
          false,
        );
        return { uri: uri(data.symbol.location.file), range: data.range };
      } catch {
        return null;
      }
    },
  });
  monaco.languages.registerReferenceProvider("lyn", {
    provideReferences: async (model, position) => {
      try {
        const data = await symbol(model, position);
        for (const ref of data.references)
          await bridge.open(ref.file, undefined, undefined, false);
        return data.references.map((r: any) => ({
          uri: uri(r.file),
          range: r.range,
        }));
      } catch {
        return [];
      }
    },
  });
  monaco.languages.registerDocumentSymbolProvider("lyn", {
    provideDocumentSymbols: async (model) => {
      await bridge.sync(model);
      const data = await api("snapshot");
      const symbols: Symbol[] = data.symbols.filter(
        (s: Symbol) => s.location.file === fileOf(model),
      );
      const outline = (s: Symbol): monaco.languages.DocumentSymbol => ({
        name: s.name,
        detail: s.kind,
        tags: [],
        kind:
          s.kind === "module"
            ? monaco.languages.SymbolKind.Module
            : s.kind === "func"
              ? monaco.languages.SymbolKind.Function
              : s.kind === "field"
                ? monaco.languages.SymbolKind.Field
                : monaco.languages.SymbolKind.Class,
        range: {
          startLineNumber: s.location.line,
          startColumn: 1,
          endLineNumber: s.end_line,
          endColumn: model.getLineMaxColumn(
            Math.min(s.end_line, model.getLineCount()),
          ),
        },
        selectionRange: {
          startLineNumber: s.location.line,
          startColumn: s.location.column,
          endLineNumber: s.location.line,
          endColumn: s.location.column + s.location.length,
        },
        children: symbols
          .filter((child) =>
            s.kind === "module"
              ? child.kind !== "module" &&
                !child.parent &&
                child.namespace === s.namespace
              : child.parent === s.id,
          )
          .map(outline),
      });
      return symbols
        .filter((s) => s.kind === "module" || s.kind === "main")
        .map(outline);
    },
  });
  monaco.languages.registerDocumentFormattingEditProvider("lyn", {
    provideDocumentFormattingEdits: async (model) => {
      const result = await api("format", { text: model.getValue() });
      return [{ range: model.getFullModelRange(), text: result.text }];
    },
  });
  monaco.languages.registerRenameProvider("lyn", {
    resolveRenameLocation: async (model, position) => {
      try {
        const data = await symbol(model, position);
        const word = model.getWordAtPosition(position);
        return {
          text: data.symbol.name,
          range: {
            startLineNumber: position.lineNumber,
            endLineNumber: position.lineNumber,
            startColumn:
              word?.word.startsWith("self.") && data.symbol.kind === "field"
                ? word.startColumn + 5
                : word?.startColumn || position.column,
            endColumn: word?.endColumn || position.column,
          },
        };
      } catch (e) {
        return {
          text: "",
          range: new monaco.Range(1, 1, 1, 1),
          rejectReason: String(e),
        };
      }
    },
    provideRenameEdits: async (model, position, newName) => {
      try {
        await bridge.sync(model);
        const data = await api("rename", {
          file: fileOf(model),
          line: position.lineNumber,
          column: position.column,
          name: newName,
        });
        for (const edit of data.edits)
          await bridge.open(edit.file, undefined, undefined, false);
        return {
          edits: data.edits.map((e: Edit) => ({
            resource: uri(e.file),
            versionId: undefined,
            textEdit: { range: e.range, text: e.text },
          })),
        };
      } catch (e) {
        return {
          edits: [],
          rejectReason: e instanceof Error ? e.message : String(e),
        };
      }
    },
  });
  monaco.languages.registerCodeActionProvider("lyn", {
    provideCodeActions: async (model, _range, context) => {
      const actions: monaco.languages.CodeAction[] = [];
      for (const marker of context.markers.filter((m) => m.code === "S003")) {
        const data = await api("ambiguous-options", {
          file: fileOf(model),
          line: marker.startLineNumber,
        });
        for (const option of data.options)
          actions.push({
            title: option.title,
            kind: "quickfix",
            diagnostics: [marker],
            edit: {
              edits: [
                {
                  resource: model.uri,
                  versionId: undefined,
                  textEdit: { range: option.range, text: option.text },
                },
              ],
            },
          });
      }
      for (const marker of context.markers.filter((m) =>
        ["L102", "L103", "L104"].includes(String(m.code)),
      )) {
        const old = model.getValueInRange(marker);
        const replacement =
          marker.code === "L102"
            ? "@module\nclass"
            : marker.code === "L103"
              ? `Option<${old.slice(0, -1)}>`
              : `->${old.slice(1)}`;
        actions.push({
          title: `Migrar para ${replacement}`,
          kind: "quickfix",
          diagnostics: [marker],
          edit: {
            edits: [
              {
                resource: model.uri,
                versionId: undefined,
                textEdit: { range: marker, text: replacement },
              },
            ],
          },
        });
      }
      for (const marker of context.markers.filter((m) => m.code === "S005")) {
        const suggestion = marker.message.match(/Did you mean ([\w.]+)\?/);
        if (suggestion)
          actions.push({
            title: `Usar ${suggestion[1]}`,
            kind: "quickfix",
            diagnostics: [marker],
            edit: {
              edits: [
                {
                  resource: model.uri,
                  versionId: undefined,
                  textEdit: { range: marker, text: suggestion[1] },
                },
              ],
            },
          });
        const missing = marker.message.match(
          /Unresolved reference: ([\p{L}_][\p{L}\p{N}_]*)/u,
        );
        if (missing)
          actions.push({
            title: `Declarar concept ${missing[1]}`,
            kind: "quickfix",
            diagnostics: [marker],
            edit: {
              edits: [
                {
                  resource: model.uri,
                  versionId: undefined,
                  textEdit: {
                    range: new monaco.Range(
                      model.getLineCount(),
                      model.getLineMaxColumn(model.getLineCount()),
                      model.getLineCount(),
                      model.getLineMaxColumn(model.getLineCount()),
                    ),
                    text: `\n\nconcept ${missing[1]}\n\npurpose:\n    Descreva o conceito\n`,
                  },
                },
              ],
            },
          });
      }
      return { actions, dispose() {} };
    },
  });
}
