import { useEffect, useMemo, useState, useCallback } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  Handle,
  Position,
  type NodeProps,
  type EdgeProps,
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react";
import { Search, ExternalLink, Focus, Network } from "lucide-react";
import "@xyflow/react/dist/style.css";
import { api, type Symbol, type GraphResult } from "./api";

function SymbolNode({ data, selected }: NodeProps) {
  const d = data as { symbol: Symbol };
  return (
    <div className={`symbol-node ${selected ? "selected" : ""}`}>
      <Handle type="target" position={Position.Left} />
      <span className={`kind-dot ${d.symbol.kind}`} />
      <div>
        <small>{d.symbol.kind}</small>
        <strong>{d.symbol.name}</strong>
      </div>
      <Handle type="source" position={Position.Right} />
    </div>
  );
}
const nodeTypes = { symbol: SymbolNode };
function RelationEdge(props: EdgeProps) {
  const offset = Number(props.data?.offset || 0);
  let [path, x, y] = getBezierPath(props);
  if (offset) {
    const delta = Math.max(90, Math.abs(props.targetX - props.sourceX) * 0.5);
    path = `M ${props.sourceX} ${props.sourceY} C ${props.sourceX + delta} ${props.sourceY + offset} ${props.targetX - delta} ${props.targetY + offset} ${props.targetX} ${props.targetY}`;
    x = (props.sourceX + props.targetX) / 2;
    y = (props.sourceY + props.targetY) / 2 + offset * 0.75;
  }
  return (
    <>
      <BaseEdge path={path} markerEnd={props.markerEnd} style={props.style} />
      <EdgeLabelRenderer>
        <span
          className="relation-label nodrag nopan"
          style={{
            position: "absolute",
            transform: `translate(-50%,-50%) translate(${x}px,${y}px)`,
          }}
        >
          {props.label}
        </span>
      </EdgeLabelRenderer>
    </>
  );
}
const edgeTypes = { relation: RelationEdge };
type Props = {
  open: (s: Symbol) => void;
  focus: string;
  theme: string;
  revision: number;
};
export function Graph(props: Props) {
  return (
    <ReactFlowProvider>
      <GraphInner {...props} />
    </ReactFlowProvider>
  );
}
function GraphInner({ open, focus, theme, revision }: Props) {
  const [query, setQuery] = useState(focus),
    [depth, setDepth] = useState(1),
    [kind, setKind] = useState("all"),
    [relation, setRelation] = useState("all");
  const [graph, setGraph] = useState<GraphResult>({
      symbols: [],
      edges: [],
      truncated: false,
    }),
    [selected, setSelected] = useState<Symbol | null>(null),
    [error, setError] = useState("");
  const flow = useReactFlow();
  const load = useCallback(
    async (q = query, d = depth) => {
      try {
        const data = await api("graph", {
          query: q ? `neighbors ${q}` : "",
          depth: d,
          limit: 120,
        });
        setGraph(data.result);
        setError("");
      } catch (e) {
        setError(String(e));
      }
    },
    [query, depth],
  );
  useEffect(() => {
    setQuery(focus);
    void load(focus, depth);
  }, [focus, revision]);
  const kinds = [...new Set(graph.symbols.map((s) => s.kind))].sort(),
    relations = [...new Set(graph.edges.map((e) => e.relation))].sort();
  const visible = useMemo(
    () => graph.symbols.filter((s) => kind === "all" || s.kind === kind),
    [graph, kind],
  );
  const nodes = useMemo(() => {
    // Stable columns by namespace/kind; bounds stay predictable, even with cyclic dependencies.
    const columns = [...new Set(visible.map((s) => s.kind))];
    const counts: Record<string, number> = {};
    return visible.map((s) => {
      const row = counts[s.kind] || 0;
      counts[s.kind] = row + 1;
      return {
        id: s.id,
        type: "symbol",
        data: { symbol: s },
        position: { x: columns.indexOf(s.kind) * 290, y: row * 110 },
        selected: selected?.id === s.id,
      };
    });
  }, [visible, selected]);
  const edges = useMemo(() => {
    const ids = new Set(visible.map((s) => s.id));
    const filtered = graph.edges.filter(
      (e) =>
        ids.has(e.from) &&
        ids.has(e.to) &&
        (relation === "all" || relation === e.relation),
    );
    const pairs: Record<string, number> = {},
      seen: Record<string, number> = {};
    for (const e of filtered) {
      const key = `${e.from}:${e.to}`;
      pairs[key] = (pairs[key] || 0) + 1;
    }
    return filtered.map((e, i) => {
      const key = `${e.from}:${e.to}`,
        index = seen[key] || 0;
      seen[key] = index + 1;
      return {
        id: `${key}-${e.relation}-${i}`,
        source: e.from,
        target: e.to,
        label: e.relation,
        type: "relation",
        data: { offset: (index - (pairs[key] - 1) / 2) * 40 },
        style: { stroke: "#657398", strokeWidth: 1.3 },
        markerEnd: { type: "arrowclosed" as any, color: "#657398" },
      };
    });
  }, [graph, visible, relation]);
  useEffect(() => {
    requestAnimationFrame(() => void flow.fitView({ padding: 0.2 }));
  }, [graph, kind]);
  return (
    <div className="graph-workspace">
      <div className="graph-toolbar">
        <Network size={15} />
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void load();
          }}
        >
          <Search size={14} />
          <input
            aria-label="Pesquisar nodes"
            placeholder="Símbolo ou vizinhança…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </form>
        <label>
          Profundidade{" "}
          <select
            aria-label="Profundidade"
            value={depth}
            onChange={(e) => {
              const d = +e.target.value;
              setDepth(d);
              void load(query, d);
            }}
          >
            {[1, 2, 3, 4].map((d) => (
              <option key={d}>{d}</option>
            ))}
          </select>
        </label>
        <select
          aria-label="Tipo de node"
          value={kind}
          onChange={(e) => setKind(e.target.value)}
        >
          <option value="all">Todos os tipos</option>
          {kinds.map((k) => (
            <option key={k}>{k}</option>
          ))}
        </select>
        <select
          aria-label="Relação"
          value={relation}
          onChange={(e) => setRelation(e.target.value)}
        >
          <option value="all">Todas as relações</option>
          {relations.map((r) => (
            <option key={r}>{r}</option>
          ))}
        </select>
        <button title="Atualizar grafo" onClick={() => void load()}>
          Atualizar
        </button>
      </div>
      {error && <div className="graph-notice">{error}</div>}
      {graph.truncated && (
        <div className="graph-notice">
          Exibindo até 120 nodes. Pesquise um símbolo para focar contexto.
        </div>
      )}
      <div className="graph-canvas">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodeClick={(_, node) => setSelected(node.data.symbol)}
          onNodeDoubleClick={(_, node) => open(node.data.symbol)}
          fitView
          minZoom={0.1}
          maxZoom={2}
          colorMode={theme === "dark" ? "dark" : "light"}
          proOptions={{ hideAttribution: true }}
        >
          <Background
            color={theme === "dark" ? "#313A53" : "#CBD2E1"}
            gap={24}
            size={1}
          />
          <Controls />
          <MiniMap
            pannable
            zoomable
            nodeColor="#8298C9"
            maskColor={theme === "dark" ? "#161A2788" : "#FFFFFF88"}
          />
        </ReactFlow>
        {!graph.symbols.length && !error && (
          <div className="graph-empty">
            Crie um documento .lyn para explorar relações.
          </div>
        )}
        {selected && (
          <aside className="graph-inspector">
            <small>{selected.kind}</small>
            <h3>{selected.qualified}</h3>
            {Object.entries(selected.sections || {}).map(([k, v]) => (
              <p key={k}>
                <b>{k}</b>
                <br />
                {v.join(" | ")}
              </p>
            ))}
            <div className="graph-relations">
              {graph.edges
                .filter((e) => e.from === selected.id || e.to === selected.id)
                .map((e, i) => {
                  const reverse = e.to === selected.id;
                  const target = graph.symbols.find(
                    (s) => s.id === (reverse ? e.from : e.to),
                  );
                  return (
                    <button
                      key={i}
                      onClick={() => {
                        if (target) setSelected(target);
                      }}
                    >
                      <span>
                        {reverse ? "recebe" : "envia"} {e.relation}
                      </span>
                      <strong>{target?.name || "fora do contexto"}</strong>
                    </button>
                  );
                })}
            </div>
            <button onClick={() => open(selected)}>
              <ExternalLink size={14} />
              Abrir definição
            </button>
            <button
              onClick={() => {
                setQuery(selected.qualified);
                void load(selected.qualified, depth);
              }}
            >
              <Focus size={14} />
              Focar vizinhança
            </button>
          </aside>
        )}
      </div>
    </div>
  );
}
