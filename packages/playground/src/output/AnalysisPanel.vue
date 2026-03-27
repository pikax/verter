<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import {
  type FileAnalysis,
  type AnalysisBinding,
  type AnalysisTemplateBindingOccurrence,
  AnalysisFlags,
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

const bindingUsageGroups = computed<Map<string, { kind: string; count: number }[]>>(() => {
  const tpl = analysis.value?.template;
  if (!tpl?.bindingOccurrences?.length) return new Map();
  const map = new Map<string, Map<string, number>>();
  for (const occ of tpl.bindingOccurrences) {
    let kindMap = map.get(occ.name);
    if (!kindMap) {
      kindMap = new Map();
      map.set(occ.name, kindMap);
    }
    kindMap.set(occ.usageKind, (kindMap.get(occ.usageKind) ?? 0) + 1);
  }
  const result = new Map<string, { kind: string; count: number }[]>();
  for (const [name, kindMap] of map) {
    result.set(
      name,
      [...kindMap.entries()].map(([kind, count]) => ({ kind, count })),
    );
  }
  return result;
});

const totalBindingOccurrences = computed(() => {
  return analysis.value?.template?.bindingOccurrences?.length ?? 0;
});

function formatInitializer(init: AnalysisBinding["initializer"]): string {
  if (!init) return "";
  if (typeof init === "string") return init === "Other" ? "..." : init;
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

// SSR readiness scoring (matches MCP compute_ssr_readiness logic)
const CLIENT_ONLY_HOOKS = new Set([
  "onMounted",
  "onUpdated",
  "onBeforeMount",
  "onBeforeUnmount",
  "onActivated",
  "onDeactivated",
]);

const ssrReadiness = computed(() => {
  if (!analysis.value) return null;
  let score = 100;
  const issues: Array<{ severity: string; type: string; detail: string }> = [];

  for (const call of analysis.value.vueApiCalls ?? []) {
    if (CLIENT_ONLY_HOOKS.has(call.api)) {
      score -= 15;
      issues.push({
        severity: "error",
        type: "client-only-lifecycle",
        detail: `\`${call.api}\` never fires during SSR`,
      });
    }
  }
  for (const q of analysis.value.domQueryCalls ?? []) {
    score -= 20;
    issues.push({
      severity: "error",
      type: "dom-query",
      detail: `\`${q.kind}\` has no DOM on server`,
    });
  }
  for (const m of analysis.value.cssVarManipulations ?? []) {
    score -= 10;
    issues.push({
      severity: "warning",
      type: "css-var-manipulation",
      detail: `\`${m.kind}\` requires DOM access`,
    });
  }
  const hasAsyncSetup = (analysis.value.scriptFlags & AnalysisFlags.ASYNC_SETUP) !== 0;
  const hasServerPrefetch = (analysis.value.vueApiCalls ?? []).some(
    (c) => c.api === "onServerPrefetch",
  );
  if (hasAsyncSetup && !hasServerPrefetch) {
    score -= 5;
    issues.push({
      severity: "info",
      type: "missing-server-prefetch",
      detail: "Async setup without `onServerPrefetch`",
    });
  }
  for (const call of analysis.value.vueApiCalls ?? []) {
    if (call.api === "useTemplateRef") {
      score -= 5;
      issues.push({
        severity: "warning",
        type: "template-ref",
        detail: "Template refs are `null` during SSR",
      });
    }
  }
  if (hasServerPrefetch) score += 5;
  score = Math.max(0, Math.min(100, score));
  return { score, issues };
});

function ssrScoreClass(score: number): string {
  if (score >= 80) return "badge-vue";
  if (score >= 50) return "badge-warning";
  return "badge-reactive";
}
</script>

<template>
  <div class="analysis-panel">
    <div v-if="!analysis" class="empty-state">Analysis not available</div>
    <div v-else class="analysis-content">
      <!-- Timing -->
      <section class="analysis-section">
        <div class="timing-row">
          <span class="timing-label">Total:</span>
          <span class="timing-value timing-total">
            {{
              store.compileTiming.verterNewJs !== null
                ? store.compileTiming.verterNewJs.toFixed(2) + "ms"
                : "—"
            }}
          </span>
        </div>
        <div class="timing-breakdown">
          <span class="timing-step">
            <span class="timing-label">Parse</span>
            <span class="timing-value">{{
              store.compileTiming.parseDurationMs !== null
                ? store.compileTiming.parseDurationMs.toFixed(2) + "ms"
                : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">Script</span>
            <span class="timing-value">{{
              store.compileTiming.scriptMs !== null
                ? store.compileTiming.scriptMs.toFixed(2) + "ms"
                : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">Template</span>
            <span class="timing-value">{{
              store.compileTiming.templateMs !== null
                ? store.compileTiming.templateMs.toFixed(2) + "ms"
                : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">Style</span>
            <span class="timing-value">{{
              store.compileTiming.styleMs !== null
                ? store.compileTiming.styleMs.toFixed(2) + "ms"
                : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">TSX</span>
            <span class="timing-value">{{
              store.compileTiming.tsxMs !== null ? store.compileTiming.tsxMs.toFixed(2) + "ms" : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">TSC</span>
            <span class="timing-value">{{
              store.compileTiming.tscMs !== null ? store.compileTiming.tscMs.toFixed(2) + "ms" : "—"
            }}</span>
          </span>
          <span class="timing-step">
            <span class="timing-label">Lint</span>
            <span class="timing-value">{{
              store.compileTiming.lintMs !== null
                ? store.compileTiming.lintMs.toFixed(2) + "ms"
                : "—"
            }}</span>
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
      <details v-if="analysis.imports?.length > 0" class="analysis-section" open>
        <summary class="section-title">Imports ({{ analysis.imports.length }})</summary>
        <div class="import-list">
          <div v-for="(imp, i) in analysis.imports" :key="i" class="import-item">
            <div class="import-source">
              <code>{{ imp.source }}</code>
              <span v-if="imp.isTypeOnly" class="badge badge-type">type</span>
            </div>
            <div v-if="imp.bindings?.length > 0" class="import-bindings">
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
      <details v-if="analysis.bindings?.length > 0" class="analysis-section" open>
        <summary class="section-title">Bindings ({{ analysis.bindings.length }})</summary>
        <div class="binding-list">
          <div v-for="(b, i) in analysis.bindings" :key="i" class="binding-item">
            <code class="binding-name">{{ b.name }}</code>
            <span class="badge badge-kind">{{ b.kind }}</span>
            <span
              v-if="b.reactivityKind && b.reactivityKind !== 'none'"
              class="badge badge-reactive"
            >
              {{ b.reactivityKind }}
            </span>
            <span v-else-if="b.isReactive" class="badge badge-reactive">reactive</span>
            <span v-if="b.typeAnnotation" class="badge badge-type">{{ b.typeAnnotation }}</span>
            <span v-if="b.initializer" class="binding-init">
              = <code>{{ formatInitializer(b.initializer) }}</code>
            </span>
          </div>
        </div>
      </details>

      <!-- Macros -->
      <details v-if="analysis.macros?.length > 0" class="analysis-section" open>
        <summary class="section-title">Macros ({{ analysis.macros.length }})</summary>
        <div class="macro-list">
          <div v-for="(m, i) in analysis.macros" :key="i" class="macro-item">
            <code class="macro-kind">{{ m.kind }}</code>
            <span v-if="m.isTypeBased" class="badge badge-type">type-based</span>
            <span v-if="m.bindingName" class="macro-binding">
              &rarr; <code>{{ m.bindingName }}</code>
            </span>
            <span v-if="m.typeReferences?.length > 0" class="macro-refs">
              refs: <code v-for="(r, j) in m.typeReferences" :key="j">{{ r }}</code>
            </span>
          </div>
        </div>
      </details>

      <!-- Style Analysis -->
      <details v-if="analysis.styles?.length > 0" class="analysis-section" open>
        <summary class="section-title">Styles ({{ analysis.styles.length }})</summary>
        <div v-for="(style, i) in analysis.styles" :key="i" class="style-block">
          <div class="style-header">
            <span class="badge badge-kind">{{ style.lang }}</span>
            <span v-if="style.scoped" class="badge badge-reactive">scoped</span>
            <span v-if="style.isModule" class="badge badge-vue">module</span>
            <span v-if="style.moduleName" class="badge badge-kind">{{ style.moduleName }}</span>
          </div>
          <div v-if="style.css" class="style-details">
            <div v-if="style.css.classes?.length > 0" class="style-sub">
              <span class="sub-label">Classes:</span>
              <code v-for="(c, j) in style.css.classes" :key="j" class="style-tag"
                >.{{ c.name }}</code
              >
            </div>
            <div v-if="style.css.ids?.length > 0" class="style-sub">
              <span class="sub-label">IDs:</span>
              <code v-for="(id, j) in style.css.ids" :key="j" class="style-tag"
                >#{{ id.name }}</code
              >
            </div>
            <div v-if="style.css.customProperties?.length > 0" class="style-sub">
              <span class="sub-label">Custom Props:</span>
              <code v-for="(cp, j) in style.css.customProperties" :key="j" class="style-tag">{{
                cp.name
              }}</code>
            </div>
            <div v-if="style.css.atRules?.length > 0" class="style-sub">
              <span class="sub-label">At-rules:</span>
              <span v-for="(ar, j) in style.css.atRules" :key="j" class="style-tag">
                @{{ ar.kind.toLowerCase() }} <code>{{ ar.name }}</code>
              </span>
            </div>
            <div class="style-sub">
              <span class="sub-label">Rules:</span>
              <span>{{ style.css.ruleCount }}</span>
              <span class="sub-label" style="margin-left: 12px">Selectors:</span>
              <span>{{ style.css.selectors.length }}</span>
            </div>
          </div>
          <div v-if="style.vBinds?.length > 0" class="style-sub">
            <span class="sub-label">v-bind:</span>
            <code v-for="(vb, j) in style.vBinds" :key="j" class="style-tag">{{
              vb.expression
            }}</code>
          </div>
          <div v-if="style.specialPseudos?.length > 0" class="style-sub">
            <span class="sub-label">Pseudos:</span>
            <span v-for="(sp, j) in style.specialPseudos" :key="j" class="badge badge-kind">
              :{{ sp.kind.toLowerCase() }}
            </span>
          </div>
        </div>
      </details>

      <!-- Template Components -->
      <details v-if="analysis.template?.components?.length" class="analysis-section">
        <summary class="section-title">
          Components ({{ analysis.template.components.length }})
        </summary>
        <div class="import-list">
          <div v-for="(comp, i) in analysis.template.components" :key="i" class="import-item">
            <code class="binding-name">{{ comp.name }}</code>
            <span v-if="comp.isDynamic" class="badge badge-reactive">dynamic</span>
            <span v-if="comp.importSource" class="import-source">
              from <code>{{ comp.importSource }}</code>
            </span>
            <div v-if="comp.props?.length" class="import-bindings">
              <span v-for="(p, j) in comp.props" :key="j" class="binding-tag">
                <code>{{ p.isBound ? ":" : "" }}{{ p.name }}</code>
                <span
                  :class="[
                    'badge',
                    p.constness === 'Const'
                      ? 'badge-vue'
                      : p.constness === 'Dynamic'
                        ? 'badge-reactive'
                        : 'badge-kind',
                  ]"
                >
                  {{ p.constness.toLowerCase() }}
                </span>
              </span>
            </div>
            <div v-if="comp.slotsUsed?.length" class="import-bindings">
              <span class="sub-label">slots:</span>
              <code v-for="(s, j) in comp.slotsUsed" :key="j" class="style-tag">{{ s }}</code>
            </div>
            <div v-if="comp.vModels?.length" class="import-bindings">
              <span class="sub-label">v-model:</span>
              <code v-for="(m, j) in comp.vModels" :key="j" class="style-tag">{{
                m.bindingName
              }}</code>
            </div>
          </div>
        </div>
      </details>

      <!-- Binding Usage Map -->
      <details v-if="totalBindingOccurrences > 0" class="analysis-section">
        <summary class="section-title">Binding Usage ({{ totalBindingOccurrences }})</summary>
        <div class="import-list">
          <div v-for="[name, kinds] in bindingUsageGroups" :key="name" class="binding-item">
            <code class="binding-name">{{ name }}</code>
            <span class="badge badge-kind">{{ kinds.reduce((s, k) => s + k.count, 0) }}x</span>
            <span v-for="k in kinds" :key="k.kind" class="badge badge-type" style="font-size: 9px">
              {{ k.kind }}: {{ k.count }}
            </span>
          </div>
          <div v-if="analysis.template?.unresolvedBindings?.length" class="unresolved-section">
            <span class="sub-label warning-label">Unresolved:</span>
            <span
              v-for="(u, i) in analysis.template.unresolvedBindings"
              :key="i"
              class="badge badge-warning"
            >
              {{ u.name }}
            </span>
          </div>
        </div>
      </details>

      <!-- Event Handlers -->
      <details v-if="analysis.template?.eventHandlers?.length" class="analysis-section">
        <summary class="section-title">
          Events ({{ analysis.template.eventHandlers.length }})
        </summary>
        <div class="import-list">
          <div v-for="(ev, i) in analysis.template.eventHandlers" :key="i" class="binding-item">
            <code class="binding-name">@{{ ev.eventName }}</code>
            <span class="macro-binding">&rarr;</span>
            <code v-if="ev.handlerBinding">{{ ev.handlerBinding }}</code>
            <code v-else class="binding-init">(inline)</code>
            <span class="badge badge-kind">{{ ev.targetTag }}</span>
            <span v-if="ev.isInline" class="badge badge-type">inline</span>
          </div>
        </div>
      </details>

      <!-- Slots & Refs -->
      <details
        v-if="
          (analysis.template?.definedSlots?.length ?? 0) +
            (analysis.template?.templateRefs?.length ?? 0) >
          0
        "
        class="analysis-section"
      >
        <summary class="section-title">Slots & Refs</summary>
        <div class="import-list">
          <div v-if="analysis.template?.definedSlots?.length">
            <div class="sub-label" style="padding: 4px 0">Defined Slots</div>
            <div
              v-for="(slot, i) in analysis.template.definedSlots"
              :key="'s' + i"
              class="binding-item"
            >
              <code class="binding-name">#{{ slot.name }}</code>
              <span v-if="slot.hasBindings && slot.bindingNames?.length" class="import-bindings">
                <span class="sub-label">scoped:</span>
                <code v-for="(bn, j) in slot.bindingNames" :key="j" class="style-tag">{{
                  bn
                }}</code>
              </span>
              <span v-else-if="!slot.hasBindings" class="badge badge-kind">no bindings</span>
            </div>
          </div>
          <div v-if="analysis.template?.templateRefs?.length">
            <div class="sub-label" style="padding: 4px 0">Template Refs</div>
            <div
              v-for="(ref, i) in analysis.template.templateRefs"
              :key="'r' + i"
              class="binding-item"
            >
              <code class="binding-name">{{ ref.name }}</code>
              <span class="badge badge-kind">{{ ref.targetTag }}</span>
              <span v-if="ref.isDynamic" class="badge badge-reactive">dynamic</span>
            </div>
          </div>
        </div>
      </details>

      <!-- Directives Summary -->
      <details
        v-if="analysis.template && analysis.template.maxNestingDepth > 0"
        class="analysis-section"
      >
        <summary class="section-title">Template Summary</summary>
        <div class="import-list">
          <div class="binding-item">
            <span class="sub-label">Max nesting depth:</span>
            <span class="badge badge-kind">{{ analysis.template.maxNestingDepth }}</span>
          </div>
          <div v-if="analysis.template.elements?.length" class="binding-item">
            <span class="sub-label">Elements:</span>
            <span class="badge badge-kind">{{ analysis.template.elements.length }}</span>
            <span class="sub-label" style="margin-left: 8px">Components:</span>
            <span class="badge badge-vue">{{
              analysis.template.elements.filter((e) => e.isComponent).length
            }}</span>
          </div>
          <div v-if="analysis.template.vIfVForConflicts?.length" class="binding-item">
            <span class="badge badge-warning"
              >v-if + v-for conflicts: {{ analysis.template.vIfVForConflicts.length }}</span
            >
          </div>
          <div v-if="analysis.template.ifChains?.length" class="binding-item">
            <span class="sub-label">If chains:</span>
            <span class="badge badge-kind">{{ analysis.template.ifChains.length }}</span>
            <span class="sub-label" style="margin-left: 8px">Longest:</span>
            <span class="badge badge-kind"
              >{{
                Math.max(...analysis.template.ifChains.map((c) => c.conditions.length))
              }}
              branches</span
            >
          </div>
          <div v-if="analysis.template.cssVarNames?.length" class="binding-item">
            <span class="sub-label">CSS vars (template):</span>
            <code v-for="(v, i) in analysis.template.cssVarNames" :key="i" class="style-tag">{{
              v
            }}</code>
          </div>
        </div>
      </details>

      <!-- Prop & Emit Definitions -->
      <details
        v-if="
          (analysis.template?.propDefinitions?.length ?? 0) +
            (analysis.template?.emitDefinitions?.length ?? 0) >
          0
        "
        class="analysis-section"
      >
        <summary class="section-title">Props & Emits</summary>
        <div class="import-list">
          <div v-if="analysis.template?.propDefinitions?.length">
            <div class="sub-label" style="padding: 4px 0">Props</div>
            <div
              v-for="(p, i) in analysis.template.propDefinitions"
              :key="'p' + i"
              class="binding-item"
            >
              <code class="binding-name">{{ p.name }}</code>
              <span v-if="p.typeAnnotation" class="badge badge-type">{{ p.typeAnnotation }}</span>
              <span v-if="p.isRequired" class="badge badge-reactive">required</span>
              <span v-if="p.hasDefault" class="badge badge-vue">default</span>
              <span v-if="p.isBoolean" class="badge badge-kind">boolean</span>
              <span v-if="!p.usedInTemplate && !p.usedInScript" class="badge badge-warning"
                >unused</span
              >
            </div>
          </div>
          <div v-if="analysis.template?.emitDefinitions?.length">
            <div class="sub-label" style="padding: 4px 0">Emits</div>
            <div
              v-for="(e, i) in analysis.template.emitDefinitions"
              :key="'e' + i"
              class="binding-item"
            >
              <code class="binding-name">{{ e.eventName }}</code>
              <span v-if="e.isDeclared" class="badge badge-vue">declared</span>
              <span v-if="e.hasValidator" class="badge badge-reactive">validator</span>
              <span v-if="e.emitLocations?.length" class="badge badge-kind"
                >{{ e.emitLocations.length }} emit sites</span
              >
            </div>
          </div>
        </div>
      </details>

      <!-- Vue API Calls -->
      <details v-if="analysis.vueApiCalls?.length" class="analysis-section">
        <summary class="section-title">Vue API Calls ({{ analysis.vueApiCalls.length }})</summary>
        <div class="import-list">
          <div v-for="(call, i) in analysis.vueApiCalls" :key="i" class="binding-item">
            <code class="binding-name">{{ call.api }}</code>
            <span v-if="call.argValue" class="badge badge-kind">"{{ call.argValue }}"</span>
            <span v-if="call.isAsyncCallback" class="badge badge-warning">async</span>
          </div>
        </div>
      </details>

      <!-- DOM Queries -->
      <details v-if="analysis.domQueryCalls?.length" class="analysis-section">
        <summary class="section-title">DOM Queries ({{ analysis.domQueryCalls.length }})</summary>
        <div class="import-list">
          <div v-for="(q, i) in analysis.domQueryCalls" :key="i" class="binding-item">
            <code class="binding-name">{{ q.kind }}</code>
            <code class="style-tag">"{{ q.selectorText }}"</code>
          </div>
        </div>
      </details>

      <!-- CSS Variable Manipulations -->
      <details v-if="analysis.cssVarManipulations?.length" class="analysis-section">
        <summary class="section-title">
          CSS Var Manipulations ({{ analysis.cssVarManipulations.length }})
        </summary>
        <div class="import-list">
          <div v-for="(m, i) in analysis.cssVarManipulations" :key="i" class="binding-item">
            <code class="binding-name">{{ m.kind }}</code>
            <code class="style-tag">{{ m.varName }}</code>
            <span v-if="m.valueExpr" class="binding-init">= {{ m.valueExpr }}</span>
          </div>
        </div>
      </details>

      <!-- SSR Readiness -->
      <details v-if="ssrReadiness" class="analysis-section">
        <summary class="section-title">
          SSR Readiness
          <span :class="['badge', ssrScoreClass(ssrReadiness.score)]" style="margin-left: 6px">
            {{ ssrReadiness.score }}/100
          </span>
        </summary>
        <div class="import-list">
          <div v-if="ssrReadiness.issues.length === 0" class="binding-item">
            <span class="badge badge-vue">No issues detected</span>
          </div>
          <div v-for="(issue, i) in ssrReadiness.issues" :key="i" class="binding-item">
            <span
              :class="[
                'badge',
                issue.severity === 'error'
                  ? 'badge-warning'
                  : issue.severity === 'warning'
                    ? 'badge-reactive'
                    : 'badge-kind',
              ]"
            >
              {{ issue.severity }}
            </span>
            <span class="binding-init">{{ issue.detail }}</span>
          </div>
        </div>
      </details>

      <!-- Store Usages -->
      <details v-if="analysis.storeUsages?.length" class="analysis-section">
        <summary class="section-title">Store Usages ({{ analysis.storeUsages.length }})</summary>
        <div class="import-list">
          <div v-for="(s, i) in analysis.storeUsages" :key="i" class="binding-item">
            <code class="binding-name">{{ s.bindingName }}</code>
            <span class="macro-binding">&rarr;</span>
            <code>{{ s.callee }}</code>
            <span class="badge badge-kind">{{ s.storeApi }}</span>
            <span v-if="s.importSource" class="binding-init">from {{ s.importSource }}</span>
          </div>
        </div>
      </details>

      <!-- Store Definitions -->
      <details v-if="analysis.storeDefinitions?.length" class="analysis-section">
        <summary class="section-title">
          Store Definitions ({{ analysis.storeDefinitions.length }})
        </summary>
        <div class="import-list">
          <div v-for="(d, i) in analysis.storeDefinitions" :key="i" class="binding-item">
            <code class="binding-name">{{ d.exportName }}</code>
            <span v-if="d.storeId" class="badge badge-vue">"{{ d.storeId }}"</span>
            <span class="badge badge-kind">{{ d.storeApi }}</span>
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
  border-radius: 4px 4px 0 0;
}

.timing-breakdown {
  display: flex;
  flex-wrap: wrap;
  gap: 2px 12px;
  padding: 6px 10px 8px;
  background: var(--bg-secondary);
  border-radius: 0 0 4px 4px;
  border-top: 1px solid var(--border-color);
}

.timing-step {
  display: flex;
  align-items: center;
  gap: 4px;
}

.timing-label {
  color: var(--text-secondary);
  font-size: 11px;
}

.timing-value {
  font-weight: 600;
  font-size: 11px;
  color: var(--accent-color, #4299e1);
}

.timing-total {
  font-size: 13px;
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

.unresolved-section {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  padding: 4px 0;
  margin-top: 4px;
  border-top: 1px solid var(--border-color);
}

.warning-label {
  color: #e5a100;
}

.badge-warning {
  background: rgba(229, 161, 0, 0.15);
  color: #e5a100;
  padding: 1px 5px;
  font-size: 10px;
  border-radius: 3px;
  font-weight: 500;
}

code {
  font-family: inherit;
}
</style>
