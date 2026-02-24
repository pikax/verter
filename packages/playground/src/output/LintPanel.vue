<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import type { LintDiagnostic } from "../core/types";

const props = defineProps<{
  store: Store;
}>();

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
</script>

<template>
  <div class="lint-panel">
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
            <span v-if="d.spanStart != null" class="lint-span">
              {{ d.spanStart }}:{{ d.spanEnd }}
            </span>
          </div>
        </div>
      </details>
    </div>
  </div>
</template>

<style scoped>
.lint-panel {
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

.lint-span {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-secondary);
  opacity: 0.6;
}

code {
  font-family: inherit;
}
</style>
