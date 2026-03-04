<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import type { OutputMode } from "../core/types";
import Preview from "./Preview.vue";
import CodeOutput from "./CodeOutput.vue";
import AnalysisPanel from "./AnalysisPanel.vue";
import LintPanel from "./LintPanel.vue";
import OutlinePanel from "./OutlinePanel.vue";
import VirtualFilesPanel from "./VirtualFilesPanel.vue";
import CssMatchPanel from "./CssMatchPanel.vue";
import SourceMapPanel from "./SourceMapPanel.vue";
import DiagnosticsPanel from "./DiagnosticsPanel.vue";

const props = defineProps<{
  store: Store;
}>();

const allTabs: { mode: OutputMode; label: string }[] = [
  { mode: "preview", label: "Preview" },
  { mode: "files", label: "Files" },
  { mode: "analysis", label: "Analysis" },
  { mode: "lint", label: "Lint" },
  { mode: "outline", label: "Outline" },
  { mode: "cssMatch", label: "CSS Match" },
  { mode: "map", label: "Map" },
  { mode: "diagnostics", label: "Diagnostics" },
];

const tabs = computed(() => {
  if (props.store.compilerOptions.ssr) {
    const list = [...allTabs];
    const filesIdx = list.findIndex((t) => t.mode === "files");
    list.splice(filesIdx + 1, 0, { mode: "ssr", label: "SSR" });
    return list;
  }
  return allTabs;
});

function openSourceMapVisualization() {
  const file = props.store.activeFile;
  if (!file) return;

  // The combined source map covers the full assembled JS (file.compiled.js),
  // matching exactly what's displayed in the Files tab (script node).
  const code = file.compiled.js;
  const map = file.compiled.verterSourceMap;
  if (!map) return;

  // evanw's source-map-visualization uses length-prefixed format:
  // btoa(`${utf8CodeLen}\0${utf8Code}${utf8MapLen}\0${utf8Map}`)
  const enc = new TextEncoder();
  const codeBytes = enc.encode(code);
  const mapBytes = enc.encode(map);
  let binary = "";
  const codeLenStr = String(codeBytes.length);
  binary += codeLenStr + "\0";
  for (const b of codeBytes) binary += String.fromCharCode(b);
  const mapLenStr = String(mapBytes.length);
  binary += mapLenStr + "\0";
  for (const b of mapBytes) binary += String.fromCharCode(b);
  const encoded = btoa(binary);
  window.open(`https://evanw.github.io/source-map-visualization/#${encoded}`, "_blank");
}

/** Whether the source map button should be visible */
const showSourceMapButton = computed(() => {
  const file = props.store.activeFile;
  if (!file) return false;
  return props.store.outputMode === "files" && !!file.compiled.verterSourceMap;
});

const lintCount = computed(() => {
  return props.store.activeFile?.compiled.lintDiagnostics?.length ?? 0;
});

const diagnosticCount = computed(() => {
  const file = props.store.activeFile;
  if (!file) return 0;
  return (
    file.compiled.compilerDiagnostics.length +
    file.compiled.lintDiagnostics.length +
    props.store.tsDiagnostics.length
  );
});

function getTabTiming(mode: OutputMode): string | null {
  const { verterNewJs } = props.store.compileTiming;
  switch (mode) {
    case "files":
      return verterNewJs !== null ? `${verterNewJs.toFixed(1)}ms` : null;
    case "analysis":
      return verterNewJs !== null ? `${verterNewJs.toFixed(1)}ms` : null;
    default:
      return null;
  }
}

function getTabBadge(mode: OutputMode): string | null {
  if (mode === "lint" && lintCount.value > 0) {
    return String(lintCount.value);
  }
  if (mode === "diagnostics" && diagnosticCount.value > 0) {
    return String(diagnosticCount.value);
  }
  return null;
}

function isEditableMode(_mode: OutputMode): boolean {
  return false;
}

function isEditedMode(_mode: OutputMode): boolean {
  return false;
}
</script>

<template>
  <div class="output-panel">
    <div class="output-tabs">
      <button
        v-for="tab in tabs"
        :key="tab.mode"
        class="output-tab"
        :class="{ active: store.outputMode === tab.mode }"
        @click="store.setOutputMode(tab.mode)"
      >
        {{ tab.label }}
        <span v-if="getTabTiming(tab.mode)" class="timing-pill">
          {{ getTabTiming(tab.mode) }}
        </span>
        <span v-if="getTabBadge(tab.mode)" class="lint-badge">
          {{ getTabBadge(tab.mode) }}
        </span>
        <span v-if="isEditedMode(tab.mode)" class="edited-badge">edited</span>
      </button>
      <button
        v-if="showSourceMapButton"
        class="sourcemap-btn"
        @click="openSourceMapVisualization"
        title="Visualize Source Map"
      >
        Source Map
      </button>
    </div>
    <div class="output-content">
      <Preview v-if="store.outputMode === 'preview'" :store="store" />
      <AnalysisPanel v-else-if="store.outputMode === 'analysis'" :store="store" />
      <LintPanel v-else-if="store.outputMode === 'lint'" :store="store" />
      <OutlinePanel v-else-if="store.outputMode === 'outline'" :store="store" />
      <VirtualFilesPanel v-else-if="store.outputMode === 'files'" :store="store" />
      <CssMatchPanel v-else-if="store.outputMode === 'cssMatch'" :store="store" />
      <SourceMapPanel v-else-if="store.outputMode === 'map'" :store="store" />
      <DiagnosticsPanel v-else-if="store.outputMode === 'diagnostics'" :store="store" />
      <CodeOutput v-else :store="store" :mode="store.outputMode" :editable="isEditableMode(store.outputMode)" />
    </div>
  </div>
</template>

<style scoped>
.output-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
}

.output-tabs {
  display: flex;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  padding: 0 8px;
  height: 36px;
  align-items: center;
  gap: 2px;
}

.output-tab {
  padding: 6px 12px;
  font-size: 13px;
  color: var(--text-secondary);
  background: var(--tab-inactive-bg);
  border-radius: 4px 4px 0 0;
}

.output-tab.active {
  background: var(--tab-active-bg);
  color: var(--text-primary);
}

.output-tab:hover {
  color: var(--text-primary);
}

.timing-pill {
  margin-left: 6px;
  padding: 2px 6px;
  font-size: 10px;
  font-family: monospace;
  background: var(--bg-tertiary);
  border-radius: 10px;
  color: var(--text-muted, var(--text-secondary));
}

.output-tab.active .timing-pill {
  background: var(--accent-color-light, rgba(66, 153, 225, 0.2));
  color: var(--accent-color);
}

.lint-badge {
  margin-left: 4px;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
  border-radius: 10px;
  min-width: 16px;
  text-align: center;
}

.edited-badge {
  margin-left: 4px;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
  border-radius: 10px;
}

.sourcemap-btn {
  margin-left: auto;
  padding: 4px 10px;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  cursor: pointer;
}

.sourcemap-btn:hover {
  color: var(--text-primary);
  background: var(--border-color);
}

.output-content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
</style>
