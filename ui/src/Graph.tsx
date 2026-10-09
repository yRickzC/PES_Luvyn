import { useEffect, useMemo, useState, useCallback, useRef } from "react";
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
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react";
import { Search, ExternalLink, Focus, Network } from "lucide-react";
import "@xyflow/react/dist/style.css";
import { api, type Symbol, type GraphResult } from "./api";
import { layoutGraph } from "./graphLayout";

function SymbolNode({ data, selected }: NodeProps) {
  const d = data as { symbol: Symbol };
  return (
    <div className={`symbol-node ${selected ? "selected" : ""}`}>
      <Handle id="in" type="target" position={Position.Left} />
      <span className={`kind-dot ${d.symbol.kind}`} />
      <div>
        <small>{d.symbol.kind}</small>
        <strong>{d.symbol.name}</strong>
      </div>
      <Handle id="out" type="source" position={Position.Right} />
    </div>
  );
}
const nodeTypes = { symbol: SymbolNode };
function RelationEdge(props: EdgeProps) {
  const path = String(props.data?.path || "");
  const x = Number(props.data?.labelX || 0),
    y = Number(props.data?.labelY || 0);
  return (
    <>
      <BaseEdge
        id={props.id}
        path={path}
        markerEnd={props.markerEnd}
        style={props.style}
        interactionWidth={16}
      />
      <EdgeLabelRenderer>
        <span
          className="relation-label nodrag nopan"
          style={{
            position: "absolute",
            opacity: props.style?.opacity,
            pointerEvents: "all",
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
  mobile?: boolean;
  depth?: number;
  onDepthChange?: (depth: number) => void;
  onFocusChange?: (focus: string) => void;
};
export function Graph(props: Props) {
  return (
    <ReactFlowProvider>
      <GraphInner {...props} />
    </ReactFlowProvider>
  );
}
function GraphInner({
  open,
  focus,
  theme,
  revision,
  mobile = false,
  depth: externalDepth,
  onDepthChange,
  onFocusChange,
}: Props) {
  const [query, setQuery] = useState(focus),
    [internalDepth, setInternalDepth] = useState(1),
    [kind, setKind] = useState("all"),
    [relation, setRelation] = useState("all");
  const depth = mobile ? (externalDepth ?? internalDepth) : internalDepth;
  const changeDepth = (value: number) => {
    setInternalDepth(value);
    onDepthChange?.(value);
  };
  const [graph, setGraph] = useState<GraphResult>({
      symbols: [],
      edges: [],
      truncated: false,
    }),
    [selected, setSelected] = useState<Symbol | null>(null),
    [error, setError] = useState("");
  const flow = useReactFlow();
  const [layout, setLayout] = useState<Awaited<ReturnType<typeof layoutGraph>>>(
    { nodes: [], edges: [], truncated: false },
  );
  const [layoutBusy, setLayoutBusy] = useState(false);
  const [selectedEdge, setSelectedEdge] = useState("");
  const graphFingerprint = useRef("");
  const depthRef = useRef(depth);
  const load = useCallback(
    async (q = query, d = depth) => {
      try {
        const data = await api("graph", {
          query: q ? `neighbors ${q}` : "",
          depth: d,
          limit: mobile ? 80 : 120,
        });
        // Revisions can concern other documents. Keep viewport and avoid
        // another worker layout when this bounded subgraph did not change.
        const fingerprint = JSON.stringify(data.result);
        if (fingerprint !== graphFingerprint.current) {
          graphFingerprint.current = fingerprint;
          setGraph(data.result);
        }
        setSelected((s) =>
          s
            ? data.result.symbols.find((n: Symbol) => n.id === s.id) || null
            : null,
        );
        setError("");
      } catch (e) {
        setError(String(e));
      }
    },
    [query, depth, mobile],
  );
  useEffect(() => {
    setQuery(focus);
    setSelected(null);
    setSelectedEdge("");
    void load(focus, depth);
  }, [focus]);
  useEffect(() => {
    if (depthRef.current === depth) return;
    depthRef.current = depth;
    void load(query, depth);
  }, [depth]);
  useEffect(() => {
    void load();
  }, [revision]);
  const kinds = [...new Set(graph.symbols.map((s) => s.kind))].sort(),
    relations = [...new Set(graph.edges.map((e) => e.relation))].sort();
  const visible = useMemo(
    () => graph.symbols.filter((s) => kind === "all" || s.kind === kind),
    [graph, kind],
  );
  useEffect(() => {
    let active = true;
    setLayoutBusy(true);
    layoutGraph(
      visible,
      graph.edges.filter((e) => relation === "all" || e.relation === relation),
    )
      .then((value) => {
        if (active) {
          setLayout(value);
          setLayoutBusy(false);
        }
      })
      .catch((e) => {
        if (active) {
          setError(String(e));
          setLayoutBusy(false);
        }
      });
    return () => {
      active = false;
    };
  }, [visible, graph.edges, relation]);
  const focusIds = useMemo(() => {
    const ids = new Set<string>();
    if (selected) {
      ids.add(selected.id);
      for (const e of layout.edges)
        if (e.source === selected.id || e.target === selected.id) {
          ids.add(e.source);
          ids.add(e.target);
        }
    }
    if (selectedEdge) {
      const e = layout.edges.find((e) => e.id === selectedEdge);
      if (e) {
        ids.add(e.source);
        ids.add(e.target);
      }
    }
    return ids;
  }, [selected, selectedEdge, layout]);
  const nodes = layout.nodes.map((n) => ({
    ...n,
    selected: selected?.id === n.id,
    style: { opacity: focusIds.size && !focusIds.has(n.id) ? 0.25 : 1 },
  }));
  const edges = layout.edges.map((e) => {
    const emphasized = selectedEdge
      ? e.id === selectedEdge
      : selected
        ? e.source === selected.id || e.target === selected.id
        : true;
    return {
      ...e,
      selected: e.id === selectedEdge,
      style: {
        stroke: emphasized ? "#8b9bc3" : "#657398",
        strokeWidth: emphasized && focusIds.size ? 2 : 1.3,
        opacity: emphasized ? 1 : 0.15,
      },
      markerEnd: {
        type: "arrowclosed" as any,
        color: emphasized ? "#8b9bc3" : "#657398",
        width: 12,
        height: 12,
      },
    };
  });
  useEffect(() => {
    requestAnimationFrame(() => {
      const focused =
        mobile && layout.nodes.find((n) => n.data.symbol.qualified === focus);
      if (focused)
        void flow.setCenter(focused.position.x + 100, focused.position.y + 32, {
          zoom: 1,
        });
      else if (mobile) void flow.fitView({ padding: 0.2, minZoom: 0.65 });
      else void flow.fitView({ padding: 0.2 });
    });
  }, [layout, mobile, focus]);
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
              changeDepth(d);
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
      {layoutBusy && <div className="graph-notice">Organizando grafo…</div>}
      {layout.truncated && (
        <div className="graph-notice">
          Até 600 relações visuais. Reduza profundidade ou filtre uma relação.
        </div>
      )}
      {graph.truncated && (
        <div className="graph-notice">
          Visão geral limitada para manter o grafo fluido. Foque um símbolo para
          ver relações próximas.
        </div>
      )}
      <div className="graph-canvas">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodeClick={(_, node) => {
            setSelected(node.data.symbol);
            setSelectedEdge("");
          }}
          onEdgeClick={(_, edge) => {
            setSelected(null);
            setSelectedEdge(edge.id);
          }}
          onPaneClick={() => {
            setSelected(null);
            setSelectedEdge("");
          }}
          nodesDraggable={false}
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
          <Controls
            fitViewOptions={{ padding: 0.2, minZoom: mobile ? 0.65 : 0.1 }}
          />
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
            <button onClick={() => setSelected(null)}>Fechar inspector</button>
            <small>{selected.kind}</small>
            <h3>{selected.qualified}</h3>
            {selected.signature && <code>{selected.signature}</code>}
            {(selected.annotations || []).map((a, i) => (
              <p key={`annotation-${i}`}>
                <code>
                  @{a.name}
                  {a.arguments.length ? `(${a.arguments.join(", ")})` : ""}
                </code>
              </p>
            ))}
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
                if (onFocusChange) onFocusChange(selected.qualified);
                else {
                  setQuery(selected.qualified);
                  void load(selected.qualified, depth);
                }
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
