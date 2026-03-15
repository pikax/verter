<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";

const props = defineProps<{
  store: Store;
}>();

interface CssPropEntry {
  name: string;
  /** Style blocks that define this property */
  definitions: Array<{ styleIndex: number }>;
  /** Used in template inline style */
  templateUsage: boolean;
  /** Script DOM manipulations (setProperty / getPropertyValue / removeProperty) */
  scriptManipulations: Array<{ kind: string; valueExpr?: string | null }>;
}

interface VBindEntry {
  expression: string;
  /** Actual generated CSS var name (e.g. "--a4f2eed6-color"), if available */
  generatedVarName: string | null;
  /** Which style blocks use this v-bind() */
  styleIndices: number[];
}

const cssPropEntries = computed<CssPropEntry[]>(() => {
  const analysis = props.store.activeFile?.compiled.analysis;
  if (!analysis) return [];

  const map = new Map<string, CssPropEntry>();

  function getOrCreate(name: string): CssPropEntry {
    let entry = map.get(name);
    if (!entry) {
      entry = { name, definitions: [], templateUsage: false, scriptManipulations: [] };
      map.set(name, entry);
    }
    return entry;
  }

  // CSS custom property definitions from style blocks
  for (let i = 0; i < (analysis.styles?.length ?? 0); i++) {
    const style = analysis.styles![i]!;
    for (const prop of style.css?.customProperties ?? []) {
      getOrCreate(prop.name).definitions.push({ styleIndex: i });
    }
  }

  // Template inline style --var usages
  for (const name of analysis.template?.cssVarNames ?? []) {
    getOrCreate(name).templateUsage = true;
  }

  // Script DOM manipulations
  for (const m of analysis.cssVarManipulations ?? []) {
    getOrCreate(m.varName).scriptManipulations.push({
      kind: m.kind,
      valueExpr: m.valueExpr,
    });
  }

  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
});

const vBindEntries = computed<VBindEntry[]>(() => {
  const analysis = props.store.activeFile?.compiled.analysis;
  if (!analysis) return [];

  const entries: VBindEntry[] = [];

  for (let i = 0; i < (analysis.styles?.length ?? 0); i++) {
    const style = analysis.styles![i]!;
    for (const vBind of style.vBinds ?? []) {
      // Check if we already have an entry for this expression
      const existing = entries.find((e) => e.expression === vBind.expression);
      if (existing) {
        if (!existing.styleIndices.includes(i)) {
          existing.styleIndices.push(i);
        }
        // Prefer non-null generatedVarName
        if (!existing.generatedVarName && vBind.generatedVarName) {
          existing.generatedVarName = vBind.generatedVarName;
        }
      } else {
        entries.push({
          expression: vBind.expression,
          generatedVarName: vBind.generatedVarName ?? null,
          styleIndices: [i],
        });
      }
    }
  }

  return entries.sort((a, b) => a.expression.localeCompare(b.expression));
});

const isEmpty = computed(() => cssPropEntries.value.length === 0 && vBindEntries.value.length === 0);
</script>

<template>
  <div class="flow-panel">
    <div v-if="isEmpty" class="empty-state">
      No CSS variables detected
    </div>

    <template v-else>
      <!-- CSS Custom Properties section -->
      <div v-if="cssPropEntries.length > 0" class="section">
        <div class="section-header">CSS Custom Properties</div>
        <div class="entry-list">
          <div v-for="entry in cssPropEntries" :key="entry.name" class="flow-card">
            <div class="var-name">{{ entry.name }}</div>
            <div class="flow-items">
              <div
                v-for="(def, i) in entry.definitions"
                :key="'def-' + i"
                class="flow-item"
                style="border-left-color: #22c55e"
              >
                <span class="flow-badge" style="background: #22c55e">CSS</span>
                Defined in &lt;style{{ def.styleIndex > 0 ? ` #${def.styleIndex}` : '' }}&gt;
              </div>

              <div
                v-if="entry.templateUsage"
                class="flow-item"
                style="border-left-color: #a855f7"
              >
                <span class="flow-badge" style="background: #a855f7">Template</span>
                Used in inline style
              </div>

              <div
                v-for="(m, i) in entry.scriptManipulations"
                :key="'script-' + i"
                class="flow-item"
                style="border-left-color: #f97316"
              >
                <span class="flow-badge" style="background: #f97316">Script</span>
                <code>{{ m.kind }}</code>
                <span v-if="m.valueExpr"> = {{ m.valueExpr }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- v-bind() Bindings section -->
      <div v-if="vBindEntries.length > 0" class="section">
        <div class="section-header">v-bind() Bindings</div>
        <div class="entry-list">
          <div v-for="entry in vBindEntries" :key="entry.expression" class="flow-card">
            <div class="var-name">
              <code class="expression">v-bind({{ entry.expression }})</code>
              <span v-if="entry.generatedVarName" class="generated-name">→ {{ entry.generatedVarName }}</span>
            </div>
            <div class="flow-items">
              <div
                v-for="(idx, i) in entry.styleIndices"
                :key="'vbind-style-' + i"
                class="flow-item"
                style="border-left-color: #3b82f6"
              >
                <span class="flow-badge" style="background: #3b82f6">v-bind</span>
                Bound in &lt;style{{ idx > 0 ? ` #${idx}` : '' }}&gt;
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
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
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.section-header {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.6px;
  color: var(--text-secondary);
  margin-bottom: 8px;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--border-color);
}

.entry-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.flow-card {
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 10px;
  background: var(--bg-secondary);
}

.var-name {
  font-weight: 600;
  font-size: 13px;
  margin-bottom: 6px;
  color: var(--accent-color, #4299e1);
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.expression {
  font-size: 13px;
  background: transparent;
  color: var(--accent-color, #4299e1);
  padding: 0;
}

.generated-name {
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 400;
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
