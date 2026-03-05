<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";

const props = defineProps<{
  store: Store;
}>();

interface VarFlowEntry {
  name: string;
  /** CSS definition sites (from style customProperties) */
  cssDefinitions: Array<{ styleIndex: number }>;
  /** v-bind() usages in CSS (style → script bridge) */
  vBindUsages: Array<{ expression: string; styleIndex: number }>;
  /** Template inline style usage (from template cssVarNames) */
  templateUsages: boolean;
  /** Script DOM manipulations (setProperty/getPropertyValue/removeProperty) */
  scriptManipulations: Array<{ kind: string; valueExpr?: string | null }>;
}

const flowEntries = computed<VarFlowEntry[]>(() => {
  const analysis = props.store.activeFile?.compiled.analysis;
  if (!analysis) return [];

  const map = new Map<string, VarFlowEntry>();

  function getOrCreate(name: string): VarFlowEntry {
    let entry = map.get(name);
    if (!entry) {
      entry = {
        name,
        cssDefinitions: [],
        vBindUsages: [],
        templateUsages: false,
        scriptManipulations: [],
      };
      map.set(name, entry);
    }
    return entry;
  }

  // CSS custom property definitions from style blocks
  for (let i = 0; i < (analysis.styles?.length ?? 0); i++) {
    const style = analysis.styles![i]!;
    for (const prop of style.css?.customProperties ?? []) {
      getOrCreate(prop.name).cssDefinitions.push({ styleIndex: i });
    }
    // v-bind() usages bridge CSS → script
    for (const vBind of style.vBinds ?? []) {
      getOrCreate(`--v-bind(${vBind.expression})`).vBindUsages.push({
        expression: vBind.expression,
        styleIndex: i,
      });
    }
  }

  // Template cssVarNames (inline style --var usage)
  for (const name of analysis.template?.cssVarNames ?? []) {
    getOrCreate(name).templateUsages = true;
  }

  // Script DOM manipulations
  for (const m of analysis.cssVarManipulations ?? []) {
    getOrCreate(m.varName).scriptManipulations.push({
      kind: m.kind,
      valueExpr: m.valueExpr,
    });
  }

  // Sort by name
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
});

function flowColor(type: "css" | "vbind" | "template" | "script"): string {
  switch (type) {
    case "css": return "#22c55e";
    case "vbind": return "#3b82f6";
    case "template": return "#a855f7";
    case "script": return "#f97316";
  }
}
</script>

<template>
  <div class="flow-panel">
    <div v-if="!flowEntries.length" class="empty-state">
      No CSS variables detected
    </div>

    <div v-else class="flow-list">
      <div v-for="entry in flowEntries" :key="entry.name" class="flow-card">
        <div class="var-name">{{ entry.name }}</div>
        <div class="flow-items">
          <div
            v-for="(def, i) in entry.cssDefinitions"
            :key="'css-' + i"
            class="flow-item"
            :style="{ borderLeftColor: flowColor('css') }"
          >
            <span class="flow-badge" :style="{ background: flowColor('css') }">CSS</span>
            Defined in &lt;style{{ def.styleIndex > 0 ? ` #${def.styleIndex}` : '' }}&gt;
          </div>

          <div
            v-for="(vb, i) in entry.vBindUsages"
            :key="'vbind-' + i"
            class="flow-item"
            :style="{ borderLeftColor: flowColor('vbind') }"
          >
            <span class="flow-badge" :style="{ background: flowColor('vbind') }">v-bind</span>
            <code>v-bind({{ vb.expression }})</code> in &lt;style{{ vb.styleIndex > 0 ? ` #${vb.styleIndex}` : '' }}&gt;
          </div>

          <div
            v-if="entry.templateUsages"
            class="flow-item"
            :style="{ borderLeftColor: flowColor('template') }"
          >
            <span class="flow-badge" :style="{ background: flowColor('template') }">Template</span>
            Used in inline style
          </div>

          <div
            v-for="(m, i) in entry.scriptManipulations"
            :key="'script-' + i"
            class="flow-item"
            :style="{ borderLeftColor: flowColor('script') }"
          >
            <span class="flow-badge" :style="{ background: flowColor('script') }">Script</span>
            <code>{{ m.kind }}</code>
            <span v-if="m.valueExpr"> = {{ m.valueExpr }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.flow-panel {
  height: 100%;
  overflow-y: auto;
  padding: 12px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
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

.flow-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.flow-card {
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 10px;
  background: var(--bg-secondary);
}

.var-name {
  font-weight: 600;
  font-size: 14px;
  margin-bottom: 8px;
  color: var(--accent-color, #4299e1);
}

.flow-items {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.flow-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-left: 3px solid;
  border-radius: 0 3px 3px 0;
  font-size: 12px;
  color: var(--text-secondary);
}

.flow-badge {
  display: inline-block;
  padding: 1px 6px;
  font-size: 9px;
  font-weight: 700;
  color: #fff;
  border-radius: 3px;
  text-transform: uppercase;
  flex-shrink: 0;
}

.flow-item code {
  font-size: 11px;
  background: var(--bg-tertiary);
  padding: 1px 4px;
  border-radius: 2px;
}
</style>
