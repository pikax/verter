<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import type { OutputMode } from "../core/types";
import Preview from "./Preview.vue";
import CodeOutput from "./CodeOutput.vue";

const props = defineProps<{
  store: Store;
}>();

const allTabs: { mode: OutputMode; label: string }[] = [
  { mode: "preview", label: "Preview" },
  { mode: "ts", label: "TS" },
  { mode: "js", label: "JS" },
  { mode: "css", label: "CSS" },
];

/** Show TS tab only when showTS option is enabled and file contains TypeScript */
const tabs = computed(() => {
  const file = props.store.activeFile;
  const showTSTab = props.store.showTS && (file?.isTS ?? false);
  return allTabs.filter((tab) => tab.mode !== "ts" || showTSTab);
});

function openSourceMapVisualization() {
  const file = props.store.activeFile;
  if (!file?.compiled.sourceMap) return;
  const code = file.compiled.ts || file.compiled.js;
  const map = file.compiled.sourceMap;
  // evanw's source-map-visualization uses length-prefixed format:
  // btoa(`${utf8CodeLen}\0${utf8Code}${utf8MapLen}\0${utf8Map}`)
  const enc = new TextEncoder();
  const codeBytes = enc.encode(code);
  const mapBytes = enc.encode(map);
  // Build binary string from UTF-8 bytes (latin1 decoding preserves raw bytes)
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

function getTabTiming(mode: OutputMode): string | null {
  const { verter, stripTypes } = props.store.compileTiming;
  switch (mode) {
    case "preview": {
      const total = (verter ?? 0) + (stripTypes ?? 0);
      return total > 0 ? `${total.toFixed(1)}ms` : null;
    }
    case "ts":
      return verter !== null ? `${verter.toFixed(1)}ms` : null;
    case "js":
      // When showTS is on and file is TS, JS tab shows stripTypes timing
      if (props.store.showTS && (props.store.activeFile?.isTS ?? false)) {
        return stripTypes !== null ? `${stripTypes.toFixed(1)}ms` : null;
      }
      return verter !== null ? `${verter.toFixed(1)}ms` : null;
    case "css":
      return null;
  }
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
      </button>
      <button
        v-if="store.activeFile?.compiled.sourceMap"
        class="sourcemap-btn"
        @click="openSourceMapVisualization"
        title="Visualize Source Map"
      >
        Source Map
      </button>
    </div>
    <div class="output-content">
      <Preview v-if="store.outputMode === 'preview'" :store="store" />
      <CodeOutput v-else :store="store" :mode="store.outputMode" />
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
