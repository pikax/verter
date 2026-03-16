<script setup lang="ts">
import { computed, ref } from "vue";
import type { Store } from "../core/store";
import type { LintDiagnostic } from "../core/types";
import {
  getCodeActions,
  getLintRuleMetadata,
  type HostCodeAction,
  type HostLintRuleMetadata,
} from "../core/compiler";

const props = defineProps<{
  store: Store;
}>();

/** Toggle between issues view and rule browser */
const activeView = ref<"issues" | "rules">("issues");

// ── Issues view ──

const diagnostics = computed<LintDiagnostic[]>(() => {
  return props.store.activeFile?.compiled.lintDiagnostics ?? [];
});

const errorCount = computed(() => diagnostics.value.filter((d) => d.severity === "error").length);
const warningCount = computed(() => diagnostics.value.filter((d) => d.severity === "warning").length);

const grouped = computed(() => {
  const map = new Map<string, LintDiagnostic[]>();
  for (const d of diagnostics.value) {
    const existing = map.get(d.category);
    if (existing) {
      existing.push(d);
    } else {
      map.set(d.category, [d]);
    }
  }
  return map;
});

function severityIcon(severity: string): string {
  switch (severity) {
    case "error":
      return "\u2716";
    case "warning":
      return "\u26A0";
    case "info":
      return "\u2139";
    default:
      return "\u2022";
  }
}

function getFixForDiagnostic(d: LintDiagnostic): HostCodeAction | null {
  const file = props.store.activeFile;
  if (!file || d.spanStart == null) return null;
  const actions = getCodeActions(file.filename, d.spanStart);
  return actions.find((a) => a.diagnosticRule === d.rule) ?? null;
}

function applyFix(action: HostCodeAction): void {
  const file = props.store.activeFile;
  if (!file) return;
  // Apply edits in reverse order to preserve offsets
  let code = file.code;
  const sorted = [...action.edits].sort((a, b) => b.spanStart - a.spanStart);
  for (const edit of sorted) {
    code = code.slice(0, edit.spanStart) + edit.newText + code.slice(edit.spanEnd);
  }
  props.store.updateCode(code);
}

// ── Rule browser ──

const allRules = computed<HostLintRuleMetadata[]>(() => getLintRuleMetadata());

const rulesByCategory = computed(() => {
  const map = new Map<string, HostLintRuleMetadata[]>();
  for (const rule of allRules.value) {
    const existing = map.get(rule.category);
    if (existing) {
      existing.push(rule);
    } else {
      map.set(rule.category, [rule]);
    }
  }
  return map;
});

/** Set of rules that fired in the current file */
const firedRules = computed(() => {
  const set = new Set<string>();
  for (const d of diagnostics.value) {
    set.add(d.rule);
  }
  return set;
});

function isRuleEnabled(name: string): boolean {
  return !props.store.disabledRules.has(name);
}

function toggleRule(name: string) {
  props.store.toggleLintRule(name);
  props.store.relint();
}

function severityBadgeClass(severity: string): string {
  switch (severity) {
    case "Error":
      return "badge-error";
    case "Warning":
      return "badge-warning";
    default:
      return "badge-info";
  }
}
</script>

<template>
  <div class="lint-panel">
    <div class="lint-toolbar">
      <button
        class="view-btn"
        :class="{ active: activeView === 'issues' }"
        @click="activeView = 'issues'"
      >
        Issues
        <span v-if="diagnostics.length > 0" class="issue-count">{{ diagnostics.length }}</span>
      </button>
      <button
        class="view-btn"
        :class="{ active: activeView === 'rules' }"
        @click="activeView = 'rules'"
      >
        Rules
        <span v-if="allRules.length > 0" class="rule-count">{{ allRules.length }}</span>
      </button>
    </div>

    <!-- Issues view -->
    <div v-if="activeView === 'issues'" class="lint-body">
      <div v-if="diagnostics.length === 0" class="empty-state">
        No lint issues found
      </div>
      <div v-else class="lint-content">
        <div class="lint-summary">
          <span v-if="errorCount > 0" class="summary-count summary-error">
            {{ errorCount }} error{{ errorCount !== 1 ? "s" : "" }}
          </span>
          <span v-if="warningCount > 0" class="summary-count summary-warning">
            {{ warningCount }} warning{{ warningCount !== 1 ? "s" : "" }}
          </span>
        </div>
        <details
          v-for="[category, items] in grouped"
          :key="category"
          class="lint-section"
          open
        >
          <summary class="section-title">
            {{ category }} ({{ items.length }})
          </summary>
          <div class="lint-list">
            <div
              v-for="(d, i) in items"
              :key="i"
              class="lint-item"
              :class="'lint-' + d.severity"
            >
              <span class="lint-icon">{{ severityIcon(d.severity) }}</span>
              <span class="lint-message">{{ d.message }}</span>
              <code class="lint-rule">{{ d.rule }}</code>
              <button
                v-if="getFixForDiagnostic(d)"
                class="lint-fix-btn"
                @click="applyFix(getFixForDiagnostic(d)!)"
              >
                Fix
              </button>
              <span v-if="d.spanStart != null" class="lint-span">
                {{ d.spanStart }}:{{ d.spanEnd }}
              </span>
            </div>
          </div>
        </details>
      </div>
    </div>

    <!-- Rule browser view -->
    <div v-else class="lint-body">
      <div v-if="allRules.length === 0" class="empty-state">
        No rule metadata available in this WASM version.
      </div>
      <div v-else class="lint-content">
        <details
          v-for="[category, rules] in rulesByCategory"
          :key="category"
          class="lint-section"
          open
        >
          <summary class="section-title">
            {{ category }} ({{ rules.length }})
          </summary>
          <div class="lint-list">
            <label
              v-for="rule in rules"
              :key="rule.name"
              class="rule-item"
              :class="{ 'rule-fired': firedRules.has(rule.name), 'rule-disabled': !isRuleEnabled(rule.name) }"
            >
              <input
                type="checkbox"
                class="rule-toggle"
                :checked="isRuleEnabled(rule.name)"
                @change="toggleRule(rule.name)"
              />
              <code class="rule-name">{{ rule.name }}</code>
              <span
                class="severity-badge"
                :class="severityBadgeClass(rule.defaultSeverity)"
              >
                {{ rule.defaultSeverity.toLowerCase() }}
              </span>
              <span v-if="firedRules.has(rule.name)" class="fired-indicator">active</span>
            </label>
          </div>
        </details>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lint-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
  font-family: ui-monospace, "Cascadia Code", "Source Code Pro", Menlo, Consolas, monospace;
}

.lint-toolbar {
  display: flex;
  gap: 2px;
  padding: 6px 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.view-btn {
  padding: 3px 10px;
  font-size: 11px;
  font-weight: 600;
  border-radius: 3px;
  cursor: pointer;
  color: var(--text-secondary);
  background: transparent;
}

.view-btn.active {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.issue-count,
.rule-count {
  margin-left: 4px;
  padding: 0 5px;
  font-size: 10px;
  border-radius: 10px;
  background: var(--bg-tertiary);
}

.lint-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.lint-content {
  padding: 12px;
}

.lint-summary {
  display: flex;
  gap: 8px;
  padding: 8px 10px;
  background: var(--bg-secondary);
  border-radius: 4px;
  margin-bottom: 8px;
}

.summary-count {
  font-weight: 600;
  font-size: 12px;
}

.summary-error {
  color: #ef4444;
}

.summary-warning {
  color: #f59e0b;
}

.lint-section {
  margin-bottom: 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
}

.lint-section[open] {
  padding-bottom: 4px;
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

.lint-list {
  padding: 0 10px;
}

.lint-item {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 4px 0;
  border-bottom: 1px solid var(--border-color);
}

.lint-item:last-child {
  border-bottom: none;
}

.lint-icon {
  flex-shrink: 0;
  width: 16px;
  text-align: center;
}

.lint-error .lint-icon {
  color: #ef4444;
}

.lint-warning .lint-icon {
  color: #f59e0b;
}

.lint-info .lint-icon {
  color: #3b82f6;
}

.lint-message {
  flex: 1;
  min-width: 0;
}

.lint-rule {
  flex-shrink: 0;
  font-size: 11px;
  padding: 1px 5px;
  background: var(--bg-tertiary);
  border-radius: 3px;
  color: var(--text-secondary);
}

.lint-fix-btn {
  flex-shrink: 0;
  padding: 1px 8px;
  font-size: 10px;
  font-weight: 600;
  background: rgba(34, 197, 94, 0.15);
  color: #22c55e;
  border: 1px solid rgba(34, 197, 94, 0.3);
  border-radius: 3px;
  cursor: pointer;
}

.lint-fix-btn:hover {
  background: rgba(34, 197, 94, 0.25);
}

.lint-span {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-secondary);
  opacity: 0.6;
}

code {
  font-family: inherit;
}

/* Rule browser styles */
.rule-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
  border-bottom: 1px solid var(--border-color);
  cursor: pointer;
}

.rule-item:last-child {
  border-bottom: none;
}

.rule-item:hover {
  background: var(--bg-secondary);
}

.rule-item.rule-fired {
  background: rgba(59, 130, 246, 0.05);
}

.rule-item.rule-disabled {
  opacity: 0.45;
}

.rule-toggle {
  flex-shrink: 0;
  width: 13px;
  height: 13px;
  cursor: pointer;
  accent-color: #3b82f6;
}

.rule-name {
  flex: 1;
  font-size: 12px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.severity-badge {
  flex-shrink: 0;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  border-radius: 3px;
}

.badge-error {
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
}

.badge-warning {
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
}

.badge-info {
  background: rgba(59, 130, 246, 0.15);
  color: #3b82f6;
}

.fired-indicator {
  flex-shrink: 0;
  padding: 1px 6px;
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  border-radius: 3px;
  background: rgba(59, 130, 246, 0.15);
  color: #3b82f6;
}
</style>
