<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from "vue";
import type { Store } from "../core/store";
import { isCarrierFilename, allFrameworkExtensions } from "../core/frameworks";

const props = defineProps<{
  store: Store;
}>();

// Import-resolution extensions: every framework carrier/adapter-module
// extension (manifest-derived) plus the plain TS/JS module extensions and the
// no-extension case. Manifest-driven so a `.svelte` import resolves to its node.
const RESOLVE_EXTENSIONS: readonly string[] = ["", ...allFrameworkExtensions(), ".ts", ".js"];

const svgContainer = ref<HTMLElement>();
const containerWidth = ref(600);
const containerHeight = ref(400);

interface GraphNode {
  id: string;
  label: string;
  /** Whether this node is a framework CARRIER (component) file, e.g. .vue/.svelte. */
  isComponent: boolean;
  x: number;
  y: number;
}

interface GraphEdge {
  from: string;
  to: string;
  kind: "import" | "component";
}

const NODE_WIDTH = 140;
const NODE_HEIGHT = 40;
const PADDING = 30;

const graph = computed(() => {
  const files = props.store.files;
  const filenames = Object.keys(files);
  if (filenames.length === 0) return { nodes: [], edges: [] };

  // Build nodes in a grid layout
  const cols = Math.ceil(Math.sqrt(filenames.length));
  const colWidth = NODE_WIDTH + PADDING * 2;
  const rowHeight = NODE_HEIGHT + PADDING * 2;

  const nodes: GraphNode[] = filenames.map((filename, i) => {
    const col = i % cols;
    const row = Math.floor(i / cols);
    return {
      id: filename,
      label: filename.replace(/^\//, ""),
      isComponent: isCarrierFilename(filename),
      x: PADDING + col * colWidth + NODE_WIDTH / 2,
      y: PADDING + row * rowHeight + NODE_HEIGHT / 2,
    };
  });

  // Build edges from import analysis
  const edges: GraphEdge[] = [];
  const filenameSet = new Set(filenames);

  for (const [filename, file] of Object.entries(files)) {
    const analysis = file.compiled.analysis;
    if (!analysis) continue;

    // Import edges
    for (const imp of analysis.imports ?? []) {
      // Resolve relative imports
      const resolved = resolveImport(filename, imp.source);
      if (resolved && filenameSet.has(resolved)) {
        edges.push({ from: filename, to: resolved, kind: "import" });
      }
    }

    // Component usage edges (from template analysis)
    if (analysis.template) {
      for (const comp of analysis.template.components ?? []) {
        if (comp.importSource) {
          const resolved = resolveImport(filename, comp.importSource);
          if (resolved && filenameSet.has(resolved)) {
            // Avoid duplicate if already an import edge
            if (!edges.some((e) => e.from === filename && e.to === resolved)) {
              edges.push({ from: filename, to: resolved, kind: "component" });
            }
          }
        }
      }
    }
  }

  return { nodes, edges };
});

function resolveImport(from: string, source: string): string | null {
  if (!source.startsWith(".")) return null;
  const fromParts = from.split("/");
  fromParts.pop(); // remove filename
  const sourceParts = source.split("/");

  const result = [...fromParts];
  for (const part of sourceParts) {
    if (part === "..") result.pop();
    else if (part !== ".") result.push(part);
  }

  let resolved = result.join("/");
  // Try the manifest-derived framework extensions plus plain TS/JS.
  const allFiles = Object.keys(props.store.files);
  for (const ext of RESOLVE_EXTENSIONS) {
    if (allFiles.includes(resolved + ext)) return resolved + ext;
  }
  return null;
}

function edgePath(edge: GraphEdge): string {
  const fromNode = graph.value.nodes.find((n) => n.id === edge.from);
  const toNode = graph.value.nodes.find((n) => n.id === edge.to);
  if (!fromNode || !toNode) return "";

  const dx = toNode.x - fromNode.x;
  const dy = toNode.y - fromNode.y;
  const dist = Math.sqrt(dx * dx + dy * dy);
  if (dist === 0) return "";

  // Start/end at node border
  const nx = dx / dist;
  const ny = dy / dist;
  const x1 = fromNode.x + (nx * NODE_WIDTH) / 2;
  const y1 = fromNode.y + (ny * NODE_HEIGHT) / 2;
  const x2 = toNode.x - (nx * NODE_WIDTH) / 2;
  const y2 = toNode.y - (ny * NODE_HEIGHT) / 2;

  return `M ${x1} ${y1} L ${x2} ${y2}`;
}

function svgViewBox(): string {
  const nodes = graph.value.nodes;
  if (!nodes.length) return "0 0 600 400";
  const maxX = Math.max(...nodes.map((n) => n.x)) + NODE_WIDTH / 2 + PADDING;
  const maxY = Math.max(...nodes.map((n) => n.y)) + NODE_HEIGHT / 2 + PADDING;
  return `0 0 ${Math.max(maxX, 200)} ${Math.max(maxY, 100)}`;
}

function handleNodeClick(nodeId: string) {
  // Switch active file when clicking a node
  if (props.store.files[nodeId]) {
    props.store.setActiveFile(nodeId);
  }
}

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  if (svgContainer.value) {
    resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        containerWidth.value = entry.contentRect.width;
        containerHeight.value = entry.contentRect.height;
      }
    });
    resizeObserver.observe(svgContainer.value);
  }
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});
</script>

<template>
  <div ref="svgContainer" class="graph-panel">
    <div v-if="!graph.nodes.length" class="empty-state">No files to display</div>

    <svg v-else class="graph-svg" :viewBox="svgViewBox()" preserveAspectRatio="xMidYMid meet">
      <defs>
        <marker id="arrowhead" markerWidth="8" markerHeight="6" refX="8" refY="3" orient="auto">
          <polygon points="0 0, 8 3, 0 6" fill="var(--text-secondary, #888)" />
        </marker>
        <marker
          id="arrowhead-comp"
          markerWidth="8"
          markerHeight="6"
          refX="8"
          refY="3"
          orient="auto"
        >
          <polygon points="0 0, 8 3, 0 6" fill="#42b883" />
        </marker>
      </defs>

      <!-- Edges -->
      <path
        v-for="(edge, i) in graph.edges"
        :key="'edge-' + i"
        :d="edgePath(edge)"
        fill="none"
        :stroke="edge.kind === 'component' ? '#42b883' : 'var(--text-secondary, #888)'"
        :stroke-width="edge.kind === 'component' ? 2 : 1.5"
        :stroke-dasharray="edge.kind === 'component' ? '6 3' : 'none'"
        :marker-end="edge.kind === 'component' ? 'url(#arrowhead-comp)' : 'url(#arrowhead)'"
      />

      <!-- Nodes -->
      <g
        v-for="node in graph.nodes"
        :key="node.id"
        class="graph-node"
        :transform="`translate(${node.x - NODE_WIDTH / 2}, ${node.y - NODE_HEIGHT / 2})`"
        @click="handleNodeClick(node.id)"
      >
        <rect
          :width="NODE_WIDTH"
          :height="NODE_HEIGHT"
          rx="6"
          :fill="node.isComponent ? 'rgba(66, 184, 131, 0.15)' : 'var(--bg-secondary, #2d2d2d)'"
          :stroke="
            node.id === store.activeFilename
              ? 'var(--accent-color, #4299e1)'
              : 'var(--border-color, #555)'
          "
          :stroke-width="node.id === store.activeFilename ? 2 : 1"
        />
        <text
          :x="NODE_WIDTH / 2"
          :y="NODE_HEIGHT / 2"
          text-anchor="middle"
          dominant-baseline="central"
          :fill="node.isComponent ? '#42b883' : 'var(--text-primary, #ddd)'"
          font-size="11"
          font-family="ui-monospace, monospace"
        >
          {{ node.label.length > 18 ? node.label.slice(0, 16) + "..." : node.label }}
        </text>
      </g>
    </svg>

    <div class="graph-legend">
      <span class="legend-item">
        <svg width="20" height="10">
          <line x1="0" y1="5" x2="20" y2="5" stroke="#888" stroke-width="1.5" />
        </svg>
        Import
      </span>
      <span class="legend-item">
        <svg width="20" height="10">
          <line
            x1="0"
            y1="5"
            x2="20"
            y2="5"
            stroke="#42b883"
            stroke-width="2"
            stroke-dasharray="6 3"
          />
        </svg>
        Component
      </span>
    </div>
  </div>
</template>

<style scoped>
.graph-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  color: var(--text-primary);
  position: relative;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.graph-svg {
  flex: 1;
  min-height: 0;
  width: 100%;
}

.graph-node {
  cursor: pointer;
}

.graph-node:hover rect {
  stroke-width: 2;
  filter: brightness(1.2);
}

.graph-legend {
  display: flex;
  gap: 16px;
  padding: 8px 12px;
  font-size: 11px;
  color: var(--text-secondary);
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.legend-item {
  display: flex;
  align-items: center;
  gap: 4px;
}
</style>
