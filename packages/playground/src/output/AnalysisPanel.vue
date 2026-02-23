<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import {
  type FileAnalysis,
  type AnalysisBinding,
  AnalysisFlagLabels,
} from "../core/types";

const props = defineProps<{
  store: Store;
}>();

const analysis = computed<FileAnalysis | null>(() => {
  return props.store.activeFile?.compiled.analysis ?? null;
});

const activeFlags = computed<string[]>(() => {
  if (!analysis.value) return [];
  const bits = analysis.value.scriptFlags;
  const labels: string[] = [];
  for (const [bit, label] of Object.entries(AnalysisFlagLabels)) {
    if (bits & Number(bit)) {
      labels.push(label);
    }
  }
  return labels;
});

function formatInitializer(init: AnalysisBinding["initializer"]): string {
  if (!init) return "";
  if (init === "Other") return "...";
  if ("FunctionCall" in init) {
    const fc = init.FunctionCall;
    let s = `${fc.callee}()`;
    if (fc.vueApi) s += ` [${fc.vueApi}]`;
    return s;
  }
  if ("Literal" in init) return init.Literal.kind;
  if ("Reference" in init) return init.Reference.name;
  return "";
}
</script>

<template>
  <div class="analysis-panel">
    <div v-if="!analysis" class="empty-state">
      Analysis not available
    </div>
    <div v-else class="analysis-content">
      <!-- Timing -->
      <section class="analysis-section">
        <div class="timing-row">
          <span class="timing-label">Parse:</span>
          <span class="timing-value">
            {{ store.compileTiming.parseDurationMs !== null
              ? store.compileTiming.parseDurationMs.toFixed(2) + 'ms'
              : '—' }}
          </span>
          <span class="timing-sep">|</span>
          <span class="timing-label">Total JS:</span>
          <span class="timing-value">
            {{ store.compileTiming.verterNewJs !== null
              ? store.compileTiming.verterNewJs.toFixed(2) + 'ms'
              : '—' }}
          </span>
        </div>
      </section>

      <!-- Flags -->
      <details v-if="activeFlags.length > 0" class="analysis-section" open>
        <summary class="section-title">Flags ({{ activeFlags.length }})</summary>
        <div class="flag-chips">
          <span v-for="flag in activeFlags" :key="flag" class="flag-chip">{{ flag }}</span>
        </div>
      </details>

      <!-- Imports -->
      <details v-if="analysis.imports.length > 0" class="analysis-section" open>
        <summary class="section-title">Imports ({{ analysis.imports.length }})</summary>
        <div class="import-list">
          <div v-for="(imp, i) in analysis.imports" :key="i" class="import-item">
            <div class="import-source">
              <code>{{ imp.source }}</code>
              <span v-if="imp.isTypeOnly" class="badge badge-type">type</span>
            </div>
            <div v-if="imp.bindings.length > 0" class="import-bindings">
              <span v-for="(b, j) in imp.bindings" :key="j" class="binding-tag">
                <code>{{ b.name }}</code>
                <span v-if="b.isTypeOnly" class="badge badge-type">type</span>
                <span v-if="b.vueApi" class="badge badge-vue">{{ b.vueApi }}</span>
              </span>
            </div>
          </div>
        </div>
      </details>

      <!-- Bindings -->
      <details v-if="analysis.bindings.length > 0" class="analysis-section" open>
        <summary class="section-title">Bindings ({{ analysis.bindings.length }})</summary>
        <div class="binding-list">
          <div v-for="(b, i) in analysis.bindings" :key="i" class="binding-item">
            <code class="binding-name">{{ b.name }}</code>
            <span class="badge badge-kind">{{ b.kind }}</span>
            <span v-if="b.isReactive" class="badge badge-reactive">reactive</span>
            <span v-if="b.initializer" class="binding-init">
              = <code>{{ formatInitializer(b.initializer) }}</code>
            </span>
          </div>
        </div>
      </details>

      <!-- Macros -->
      <details v-if="analysis.macros.length > 0" class="analysis-section" open>
        <summary class="section-title">Macros ({{ analysis.macros.length }})</summary>
        <div class="macro-list">
          <div v-for="(m, i) in analysis.macros" :key="i" class="macro-item">
            <code class="macro-kind">{{ m.kind }}</code>
            <span v-if="m.isTypeBased" class="badge badge-type">type-based</span>
            <span v-if="m.bindingName" class="macro-binding">
              &rarr; <code>{{ m.bindingName }}</code>
            </span>
            <span v-if="m.typeReferences.length > 0" class="macro-refs">
              refs: <code v-for="(r, j) in m.typeReferences" :key="j">{{ r }}</code>
            </span>
          </div>
        </div>
      </details>

      <!-- Style Analysis -->
      <details v-if="analysis.styles.length > 0" class="analysis-section" open>
        <summary class="section-title">Styles ({{ analysis.styles.length }})</summary>
        <div v-for="(style, i) in analysis.styles" :key="i" class="style-block">
          <div class="style-header">
            <span class="badge badge-kind">{{ style.lang }}</span>
            <span v-if="style.scoped" class="badge badge-reactive">scoped</span>
            <span v-if="style.isModule" class="badge badge-vue">module</span>
            <span v-if="style.moduleName" class="badge badge-kind">{{ style.moduleName }}</span>
          </div>
          <div v-if="style.css" class="style-details">
            <div v-if="style.css.classes.length > 0" class="style-sub">
              <span class="sub-label">Classes:</span>
              <code v-for="(c, j) in style.css.classes" :key="j" class="style-tag">.{{ c.name }}</code>
            </div>
            <div v-if="style.css.ids.length > 0" class="style-sub">
              <span class="sub-label">IDs:</span>
              <code v-for="(id, j) in style.css.ids" :key="j" class="style-tag">#{{ id.name }}</code>
            </div>
            <div v-if="style.css.customProperties.length > 0" class="style-sub">
              <span class="sub-label">Custom Props:</span>
              <code v-for="(cp, j) in style.css.customProperties" :key="j" class="style-tag">{{ cp.name }}</code>
            </div>
            <div v-if="style.css.atRules.length > 0" class="style-sub">
              <span class="sub-label">At-rules:</span>
              <span v-for="(ar, j) in style.css.atRules" :key="j" class="style-tag">
                @{{ ar.kind.toLowerCase() }} <code>{{ ar.name }}</code>
              </span>
            </div>
            <div class="style-sub">
              <span class="sub-label">Rules:</span>
              <span>{{ style.css.ruleCount }}</span>
              <span class="sub-label" style="margin-left: 12px;">Selectors:</span>
              <span>{{ style.css.selectors.length }}</span>
            </div>
          </div>
          <div v-if="style.vBinds.length > 0" class="style-sub">
            <span class="sub-label">v-bind:</span>
            <code v-for="(vb, j) in style.vBinds" :key="j" class="style-tag">{{ vb.expression }}</code>
          </div>
          <div v-if="style.specialPseudos.length > 0" class="style-sub">
            <span class="sub-label">Pseudos:</span>
            <span v-for="(sp, j) in style.specialPseudos" :key="j" class="badge badge-kind">
              :{{ sp.kind.toLowerCase() }}
            </span>
          </div>
        </div>
      </details>
    </div>
  </div>
</template>

<style scoped>
.analysis-panel {
  height: 100%;
  overflow-y: auto;
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

.analysis-content {
  padding: 12px;
}

.analysis-section {
  margin-bottom: 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
}

.analysis-section[open] {
  padding-bottom: 8px;
}

.section-title {
  padding: 6px 10px;
  font-weight: 600;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}

.section-title:hover {
  color: var(--text-primary);
}

.timing-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  background: var(--bg-secondary);
  border-radius: 4px;
  margin-bottom: 4px;
}

.timing-label {
  color: var(--text-secondary);
  font-size: 12px;
}

.timing-value {
  font-weight: 600;
  color: var(--accent-color, #4299e1);
}

.timing-sep {
  color: var(--text-secondary);
  opacity: 0.4;
}

.flag-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 4px 10px;
}

.flag-chip {
  padding: 2px 8px;
  font-size: 11px;
  background: var(--bg-tertiary);
  border-radius: 10px;
  color: var(--text-secondary);
}

.import-list,
.binding-list,
.macro-list {
  padding: 4px 10px;
}

.import-item,
.binding-item,
.macro-item {
  padding: 3px 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}

.import-source {
  display: flex;
  align-items: center;
  gap: 4px;
}

.import-bindings {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-left: 12px;
}

.binding-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.badge {
  padding: 1px 5px;
  font-size: 10px;
  border-radius: 3px;
  font-weight: 500;
}

.badge-type {
  background: rgba(168, 85, 247, 0.15);
  color: #a855f7;
}

.badge-vue {
  background: rgba(66, 184, 131, 0.15);
  color: #42b883;
}

.badge-kind {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.badge-reactive {
  background: rgba(66, 153, 225, 0.15);
  color: var(--accent-color, #4299e1);
}

.binding-name,
.macro-kind {
  font-weight: 600;
}

.binding-init {
  color: var(--text-secondary);
  font-size: 12px;
}

.macro-binding {
  color: var(--text-secondary);
}

.macro-refs {
  color: var(--text-secondary);
  font-size: 12px;
}

.macro-refs code {
  margin-left: 2px;
}

.style-block {
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-color);
}

.style-block:last-child {
  border-bottom: none;
}

.style-header {
  display: flex;
  gap: 4px;
  margin-bottom: 4px;
}

.style-details {
  margin-left: 8px;
}

.style-sub {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  padding: 2px 0;
}

.sub-label {
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 500;
}

.style-tag {
  font-size: 12px;
  padding: 1px 4px;
  background: var(--bg-tertiary);
  border-radius: 2px;
}

code {
  font-family: inherit;
}
</style>
