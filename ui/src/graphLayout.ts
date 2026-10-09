import ELK, { type ElkNode } from "elkjs/lib/elk-api";
import workerUrl from "elkjs/lib/elk-worker.min.js?url";
import { Position } from "@xyflow/react";
import type { Symbol, GraphResult } from "./api";
const elk = new ELK({ workerUrl });
export async function layoutGraph(
  symbols: Symbol[],
  edges: GraphResult["edges"],
) {
  const ids = new Set(symbols.map((s) => s.id));
  const unique = new Map<string, GraphResult["edges"][number]>();
  for (const edge of edges)
    if (ids.has(edge.from) && ids.has(edge.to))
      unique.set(`${edge.from}:${edge.to}:${edge.relation}`, edge);
  const list = [...unique.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .slice(0, 600);
  const graph: ElkNode = {
    id: "workspace",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.edgeRouting": "ORTHOGONAL",
      "elk.spacing.nodeNode": "45",
      "elk.layered.spacing.nodeNodeBetweenLayers": "110",
      "elk.layered.spacing.edgeEdgeBetweenLayers": "20",
      "elk.spacing.edgeNode": "22",
      "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
    },
    children: symbols.map((s) => ({
      id: s.id,
      width: 200,
      height: 64,
      layoutOptions: { "elk.portConstraints": "FIXED_POS" },
      ports: [
        {
          id: `${s.id}:in`,
          x: 0,
          y: 32,
          width: 0,
          height: 0,
          layoutOptions: { "elk.port.side": "WEST" },
        },
        {
          id: `${s.id}:out`,
          x: 200,
          y: 32,
          width: 0,
          height: 0,
          layoutOptions: { "elk.port.side": "EAST" },
        },
      ],
    })),
    edges: list.map(([id, e]) => ({
      id,
      sources: [`${e.from}:out`],
      targets: [`${e.to}:in`],
      labels: [
        {
          text: e.relation,
          width: e.relation.length * 6 + 12,
          height: 18,
          layoutOptions: { "elk.edgeLabels.placement": "CENTER" },
        },
      ],
    })),
  };
  const result = await elk.layout(graph);
  return {
    truncated: unique.size > 600,
    nodes: (result.children || []).map((n) => ({
      id: n.id,
      type: "symbol",
      position: { x: n.x || 0, y: n.y || 0 },
      // Layout owns fixed geometry. Retain dimensions/ports across controlled
      // updates so selection, zoom and workspace refresh cannot hide nodes.
      width: 200,
      height: 64,
      measured: { width: 200, height: 64 },
      handles: [
        {
          id: "in",
          type: "target" as const,
          position: Position.Left,
          x: 0,
          y: 32,
          width: 0,
          height: 0,
        },
        {
          id: "out",
          type: "source" as const,
          position: Position.Right,
          x: 200,
          y: 32,
          width: 0,
          height: 0,
        },
      ],
      data: { symbol: symbols.find((s) => s.id === n.id)! },
    })),
    edges: (result.edges || []).map((e) => {
      const edge = unique.get(e.id)!;
      const section = e.sections?.[0];
      const points = section
        ? [section.startPoint, ...(section.bendPoints || []), section.endPoint]
        : [];
      const label = e.labels?.[0];
      return {
        id: e.id,
        source: edge.from,
        target: edge.to,
        sourceHandle: "out",
        targetHandle: "in",
        label: edge.relation,
        type: "relation",
        data: {
          path: points
            .map((p, i) => `${i ? "L" : "M"} ${p.x} ${p.y}`)
            .join(" "),
          labelX: (label?.x || 0) + (label?.width || 0) / 2,
          labelY: (label?.y || 0) + 9,
        },
      };
    }),
  };
}
