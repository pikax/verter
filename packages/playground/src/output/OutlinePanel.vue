<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import { getDocumentSymbols, type HostDocumentSymbol } from "../core/compiler";

const props = defineProps<{ store: Store }>();

const symbols = computed<HostDocumentSymbol[]>(() => {
  const file = props.store.activeFile;
  if (!file) return [];
  // Access analysis to create reactive dependency on recompilation
  void file.compiled.analysis;
  return getDocumentSymbols(file.filename);
});

const SYMBOL_KIND_LABELS: Record<number, string> = {
  1: "module",
  4: "class",
  6: "property",
  11: "function",
  12: "variable",
  19: "key",
  22: "struct",
};

function kindLabel(kind: number): string {
  return SYMBOL_KIND_LABELS[kind] ?? `kind(${kind})`;
}
</script>

<template>
  <div class="outline-panel">
    <div v-if="symbols.length === 0" class="empty-state">No document symbols available.</div>
    <ul v-else class="symbol-list">
      <li v-for="(sym, i) in symbols" :key="i" class="symbol-item">
        <div class="symbol-header">
          <span class="symbol-kind">{{ kindLabel(sym.kind) }}</span>
          <span class="symbol-name">{{ sym.name }}</span>
          <span v-if="sym.detail" class="symbol-detail">{{ sym.detail }}</span>
        </div>
        <ul v-if="sym.children.length > 0" class="symbol-children">
          <li v-for="(child, j) in sym.children" :key="j" class="symbol-child">
            <span class="symbol-kind">{{ kindLabel(child.kind) }}</span>
            <span class="symbol-name">{{ child.name }}</span>
            <span v-if="child.detail" class="symbol-detail">{{ child.detail }}</span>
          </li>
        </ul>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.outline-panel {
  height: 100%;
  overflow: auto;
  padding: 8px 12px;
  font-size: 13px;
}
.empty-state {
  color: var(--text-secondary);
  text-align: center;
  padding: 32px;
}
.symbol-list {
  list-style: none;
  padding: 0;
  margin: 0;
}
.symbol-item {
  margin-bottom: 8px;
}
.symbol-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
  font-weight: 600;
}
.symbol-kind {
  display: inline-block;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  border-radius: 3px;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  text-transform: uppercase;
}
.symbol-name {
  color: var(--text-primary);
}
.symbol-detail {
  color: var(--text-secondary);
  font-weight: 400;
  font-size: 12px;
}
.symbol-children {
  list-style: none;
  padding-left: 20px;
  margin: 2px 0;
}
.symbol-child {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 0;
}
</style>
