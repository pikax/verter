<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import { matchCssSelectors, type HostSelectorMatchResult } from "../core/compiler";

const props = defineProps<{ store: Store }>();

const results = computed<HostSelectorMatchResult[]>(() => {
  const file = props.store.activeFile;
  if (!file) return [];
  // Access analysis to create reactive dependency on recompilation
  void file.compiled.analysis;
  return matchCssSelectors(file.filename);
});

/** Unique template element tags across all selectors */
const elements = computed(() => {
  if (results.value.length === 0) return [];
  const first = results.value[0];
  if (!first) return [];
  return first.matches.map((m) => m.tag);
});

function matchClass(result: string): string {
  if (result === "match") return "cell-match";
  if (result === "maybe") return "cell-maybe";
  return "cell-no";
}
</script>

<template>
  <div class="css-match-panel">
    <div v-if="results.length === 0" class="empty-state">
      No CSS selectors or template elements to match.
    </div>
    <div v-else class="matrix-wrapper">
      <table class="match-matrix">
        <thead>
          <tr>
            <th class="selector-header">Selector</th>
            <th v-for="(el, i) in elements" :key="i" class="element-header">
              {{ el }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(sel, si) in results" :key="si">
            <td class="selector-cell" :title="sel.selectorText">{{ sel.selectorText }}</td>
            <td
              v-for="(m, mi) in sel.matches"
              :key="mi"
              class="match-cell"
              :class="matchClass(m.result)"
              :title="`${sel.selectorText} vs <${m.tag}>: ${m.result}`"
            >
              {{ m.result === "match" ? "Y" : m.result === "maybe" ? "?" : "" }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.css-match-panel {
  height: 100%;
  overflow: auto;
  padding: 8px;
}
.empty-state {
  color: var(--text-secondary);
  text-align: center;
  padding: 32px;
}
.matrix-wrapper {
  overflow: auto;
}
.match-matrix {
  border-collapse: collapse;
  font-size: 12px;
}
.match-matrix th,
.match-matrix td {
  border: 1px solid var(--border-color);
  padding: 4px 8px;
  text-align: center;
  white-space: nowrap;
}
.selector-header {
  text-align: left;
  background: var(--bg-secondary);
  position: sticky;
  left: 0;
  z-index: 1;
}
.element-header {
  background: var(--bg-secondary);
  font-weight: 600;
}
.selector-cell {
  text-align: left;
  font-family: monospace;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  background: var(--bg-primary);
  position: sticky;
  left: 0;
}
.match-cell {
  width: 36px;
  min-width: 36px;
  font-weight: 600;
}
.cell-match {
  background: rgba(34, 197, 94, 0.15);
  color: #22c55e;
}
.cell-maybe {
  background: rgba(234, 179, 8, 0.15);
  color: #eab308;
}
.cell-no {
  background: transparent;
  color: var(--text-secondary);
}
</style>
