import type {
  NativeOriginGraph,
  NativeOriginNode,
  NativeOriginEdge,
} from "./native-component-meta.js";

export interface OriginWalkResult {
  node: NativeOriginNode;
  edges: NativeOriginEdge[];
  targets: NativeOriginNode[];
}

export interface OriginChainEntry {
  node: NativeOriginNode;
  edge?: NativeOriginEdge;
  depth: number;
}

export function getMetaOrigin(
  graph: NativeOriginGraph,
  nodeId: number,
): OriginWalkResult | undefined {
  const node = graph.nodes.find((n) => n.id === nodeId);
  if (!node) return undefined;
  const edges = graph.edges.filter((e) => e.source === nodeId);
  const targetIds = new Set(edges.map((e) => e.target));
  const targets = graph.nodes.filter((n) => targetIds.has(n.id));
  return { node, edges, targets };
}

export function walkOriginChain(graph: NativeOriginGraph, startId: number): OriginChainEntry[] {
  const result: OriginChainEntry[] = [];
  const visited = new Set<number>();
  const queue: { id: number; edge?: NativeOriginEdge; depth: number }[] = [
    { id: startId, depth: 0 },
  ];
  while (queue.length > 0) {
    const { id, edge, depth } = queue.shift()!;
    if (visited.has(id)) continue;
    visited.add(id);
    const node = graph.nodes.find((n) => n.id === id);
    if (!node) continue;
    result.push({ node, edge, depth });
    for (const e of graph.edges) {
      if (e.source === id && !visited.has(e.target)) {
        queue.push({ id: e.target, edge: e, depth: depth + 1 });
      }
    }
  }
  return result;
}

export function findOriginNodesByKind(graph: NativeOriginGraph, kind: string): NativeOriginNode[] {
  return graph.nodes.filter((n) => n.kind === kind);
}
