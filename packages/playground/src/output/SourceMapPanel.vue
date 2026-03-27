<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Store } from "../core/store";
import { lookupSource, lookupGenerated, parseMappings, type Segment } from "../core/sourcemap";

const props = defineProps<{ store: Store }>();

/** Which generated output to visualize: "js" or "types" */
const mapSource = ref<"js" | "types">("js");

const vueCode = computed(() => props.store.activeFile?.code ?? "");

const generatedCode = computed(() => {
  const file = props.store.activeFile;
  if (!file) return "";
  return mapSource.value === "types" ? file.compiled.types : file.compiled.js;
});

const sourceMapJson = computed(() => {
  const file = props.store.activeFile;
  if (!file) return "";
  return mapSource.value === "types" ? file.compiled.typesSourceMap : file.compiled.verterSourceMap;
});

const hasJsMap = computed(() => !!props.store.activeFile?.compiled.verterSourceMap);
const hasTypesMap = computed(() => !!props.store.activeFile?.compiled.typesSourceMap);

// Auto-switch to types if JS map unavailable
watch(
  [hasJsMap, hasTypesMap],
  () => {
    if (mapSource.value === "js" && !hasJsMap.value && hasTypesMap.value) {
      mapSource.value = "types";
    } else if (mapSource.value === "types" && !hasTypesMap.value && hasJsMap.value) {
      mapSource.value = "js";
    }
  },
  { immediate: true },
);

const vueLines = computed(() => vueCode.value.split("\n"));
const genLines = computed(() => generatedCode.value.split("\n"));

/** Parse all mapping segments for coloring */
const allSegments = computed(() => {
  if (!sourceMapJson.value) return [];
  return parseMappings(
    (() => {
      try {
        return JSON.parse(sourceMapJson.value).mappings ?? "";
      } catch {
        return "";
      }
    })(),
  );
});

// Hover state
const hoveredVueLine = ref<number | null>(null);
const hoveredGenLine = ref<number | null>(null);

/** Highlighted generated line when hovering a Vue source line */
const highlightedGenLine = computed<number | null>(() => {
  if (hoveredVueLine.value == null || !sourceMapJson.value) return null;
  const result = lookupGenerated(sourceMapJson.value, hoveredVueLine.value, 0);
  return result?.line ?? null;
});

/** Highlighted Vue source line when hovering a generated line */
const highlightedVueLine = computed<number | null>(() => {
  if (hoveredGenLine.value == null || !sourceMapJson.value) return null;
  const result = lookupSource(sourceMapJson.value, hoveredGenLine.value, 0);
  return result?.line ?? null;
});

/** Color palette for mapping segments (cycles through) */
const SEGMENT_COLORS = [
  "rgba(59, 130, 246, 0.2)", // blue
  "rgba(139, 92, 246, 0.2)", // purple
  "rgba(6, 182, 212, 0.2)", // cyan
  "rgba(34, 197, 94, 0.2)", // green
  "rgba(234, 179, 8, 0.2)", // yellow
  "rgba(249, 115, 22, 0.2)", // orange
  "rgba(236, 72, 153, 0.2)", // pink
  "rgba(168, 85, 247, 0.2)", // violet
];

/** Compute background colors for generated lines based on mapping density */
function genLineBackground(lineIdx: number): string {
  if (lineIdx === highlightedGenLine.value) return "rgba(59, 130, 246, 0.25)";
  const segs = allSegments.value[lineIdx];
  if (!segs || segs.length === 0) return "transparent";
  return SEGMENT_COLORS[lineIdx % SEGMENT_COLORS.length];
}

/** Compute background colors for vue lines based on mapping */
function vueLineBackground(lineIdx: number): string {
  if (lineIdx === highlightedVueLine.value) return "rgba(59, 130, 246, 0.25)";
  // Check if any generated segment maps to this source line
  for (let genLine = 0; genLine < allSegments.value.length; genLine++) {
    const segs = allSegments.value[genLine];
    if (!segs) continue;
    for (const seg of segs) {
      if (seg[2] === lineIdx) {
        return SEGMENT_COLORS[genLine % SEGMENT_COLORS.length];
      }
    }
  }
  return "transparent";
}

/** Summary: total segment count */
const segmentCount = computed(() => {
  let count = 0;
  for (const line of allSegments.value) {
    count += line.length;
  }
  return count;
});
</script>

<template>
  <div class="sourcemap-panel">
    <div v-if="!sourceMapJson" class="empty-state">
      No source map available for the current output.
    </div>
    <template v-else>
      <div class="map-toolbar">
        <div class="map-toggle">
          <button
            v-if="hasJsMap"
            class="toggle-btn"
            :class="{ active: mapSource === 'js' }"
            @click="mapSource = 'js'"
          >
            JS
          </button>
          <button
            v-if="hasTypesMap"
            class="toggle-btn"
            :class="{ active: mapSource === 'types' }"
            @click="mapSource = 'types'"
          >
            Types
          </button>
        </div>
        <span class="map-stats">
          {{ segmentCount }} segment{{ segmentCount !== 1 ? "s" : "" }} ·
          {{ genLines.length }} generated line{{ genLines.length !== 1 ? "s" : "" }}
        </span>
      </div>
      <div class="map-split">
        <div class="map-pane">
          <div class="pane-header">Vue Source</div>
          <div class="pane-content">
            <div
              v-for="(line, i) in vueLines"
              :key="i"
              class="code-line"
              :style="{ background: vueLineBackground(i) }"
              @mouseenter="hoveredVueLine = i"
              @mouseleave="hoveredVueLine = null"
            >
              <span class="line-number">{{ i + 1 }}</span>
              <pre class="line-text">{{ line }}</pre>
            </div>
          </div>
        </div>
        <div class="map-divider" />
        <div class="map-pane">
          <div class="pane-header">Generated {{ mapSource === "types" ? "TSX" : "JS" }}</div>
          <div class="pane-content">
            <div
              v-for="(line, i) in genLines"
              :key="i"
              class="code-line"
              :style="{ background: genLineBackground(i) }"
              @mouseenter="hoveredGenLine = i"
              @mouseleave="hoveredGenLine = null"
            >
              <span class="line-number">{{ i + 1 }}</span>
              <pre class="line-text">{{ line }}</pre>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.sourcemap-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  font-size: 12px;
  font-family: ui-monospace, "Cascadia Code", "Source Code Pro", Menlo, Consolas, monospace;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.map-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.map-toggle {
  display: flex;
  gap: 2px;
}

.toggle-btn {
  padding: 3px 10px;
  font-size: 11px;
  font-weight: 600;
  border-radius: 3px;
  cursor: pointer;
  color: var(--text-secondary);
  background: transparent;
}

.toggle-btn.active {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.map-stats {
  color: var(--text-secondary);
  font-size: 11px;
}

.map-split {
  flex: 1;
  display: flex;
  min-height: 0;
}

.map-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.pane-header {
  padding: 4px 12px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--text-secondary);
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.pane-content {
  flex: 1;
  overflow: auto;
  padding: 4px 0;
}

.map-divider {
  width: 1px;
  background: var(--border-color);
  flex-shrink: 0;
}

.code-line {
  display: flex;
  align-items: baseline;
  padding: 0 8px;
  min-height: 18px;
  cursor: default;
  transition: background 0.1s;
}

.code-line:hover {
  background: rgba(59, 130, 246, 0.15) !important;
}

.line-number {
  flex-shrink: 0;
  width: 36px;
  text-align: right;
  padding-right: 8px;
  color: var(--text-secondary);
  opacity: 0.5;
  user-select: none;
}

.line-text {
  margin: 0;
  white-space: pre;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
