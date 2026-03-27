<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import type { HostDiagnostic, LintDiagnostic, TsDiagnosticEntry } from "../core/types";

const props = defineProps<{
  store: Store;
}>();

interface DiagnosticItem {
  source: "verter" | "lint" | "typescript";
  severity: "error" | "warning" | "info";
  message: string;
  code: string;
  spanStart: number | null;
  spanEnd: number | null;
}

function hostToDiagnosticItem(d: HostDiagnostic): DiagnosticItem {
  return {
    source: "verter",
    severity: d.severity,
    message: d.message,
    code: d.code,
    spanStart: d.spanStart ?? null,
    spanEnd: d.spanEnd ?? null,
  };
}

function lintToDiagnosticItem(d: LintDiagnostic): DiagnosticItem {
  return {
    source: "lint",
    severity: d.severity,
    message: `[${d.rule}] ${d.message}`,
    code: d.rule,
    spanStart: d.spanStart,
    spanEnd: d.spanEnd,
  };
}

function tsToDiagnosticItem(d: TsDiagnosticEntry): DiagnosticItem {
  return {
    source: "typescript",
    severity: d.severity,
    message: d.message,
    code: `TS${d.code}`,
    spanStart: d.start,
    spanEnd: d.end,
  };
}

const allDiagnostics = computed<DiagnosticItem[]>(() => {
  const file = props.store.activeFile;
  if (!file) return [];

  const items: DiagnosticItem[] = [];

  for (const d of file.compiled.compilerDiagnostics) {
    items.push(hostToDiagnosticItem(d));
  }
  for (const d of file.compiled.lintDiagnostics) {
    items.push(lintToDiagnosticItem(d));
  }
  for (const d of props.store.tsDiagnostics) {
    items.push(tsToDiagnosticItem(d));
  }

  return items;
});

const errorCount = computed(
  () => allDiagnostics.value.filter((d) => d.severity === "error").length,
);
const warningCount = computed(
  () => allDiagnostics.value.filter((d) => d.severity === "warning").length,
);
const infoCount = computed(() => allDiagnostics.value.filter((d) => d.severity === "info").length);

const groupedBySource = computed(() => {
  const groups: Record<string, DiagnosticItem[]> = {};
  for (const d of allDiagnostics.value) {
    if (!groups[d.source]) groups[d.source] = [];
    groups[d.source].push(d);
  }
  return groups;
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

function severityClass(severity: string): string {
  return `severity-${severity}`;
}

function sourceLabel(source: string): string {
  switch (source) {
    case "verter":
      return "Verter Compiler";
    case "lint":
      return "Verter Lint";
    case "typescript":
      return "TypeScript";
    default:
      return source;
  }
}

function formatLocation(d: DiagnosticItem): string {
  if (d.spanStart == null) return "";
  const file = props.store.activeFile;
  if (!file) return `${d.spanStart}`;
  // Simple line:col from offset
  const code = file.code;
  let line = 1;
  let col = 1;
  for (let i = 0; i < d.spanStart && i < code.length; i++) {
    if (code[i] === "\n") {
      line++;
      col = 1;
    } else {
      col++;
    }
  }
  return `${line}:${col}`;
}
</script>

<template>
  <div class="diagnostics-panel">
    <div class="diag-toolbar">
      <span class="diag-summary">
        <span v-if="errorCount > 0" class="count-error"
          >{{ errorCount }} error{{ errorCount !== 1 ? "s" : "" }}</span
        >
        <span v-if="warningCount > 0" class="count-warning"
          >{{ warningCount }} warning{{ warningCount !== 1 ? "s" : "" }}</span
        >
        <span v-if="infoCount > 0" class="count-info">{{ infoCount }} info</span>
        <span v-if="allDiagnostics.length === 0" class="count-ok">No issues</span>
      </span>
    </div>
    <div class="diag-body">
      <template v-if="allDiagnostics.length === 0">
        <div class="empty-state">No diagnostics to display.</div>
      </template>
      <template v-else>
        <div v-for="(items, source) in groupedBySource" :key="source" class="diag-section">
          <div class="section-header">
            {{ sourceLabel(source as string) }}
            <span class="section-count">{{ items.length }}</span>
          </div>
          <div
            v-for="(d, i) in items"
            :key="i"
            class="diag-item"
            :class="severityClass(d.severity)"
          >
            <span class="diag-icon" :class="severityClass(d.severity)">{{
              severityIcon(d.severity)
            }}</span>
            <span class="diag-code">{{ d.code }}</span>
            <span class="diag-message">{{ d.message }}</span>
            <span v-if="d.spanStart != null" class="diag-location">{{ formatLocation(d) }}</span>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.diagnostics-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.diag-toolbar {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
  gap: 8px;
}

.diag-summary {
  display: flex;
  gap: 12px;
  font-size: 12px;
  font-weight: 500;
}

.count-error {
  color: #ef4444;
}
.count-warning {
  color: #f59e0b;
}
.count-info {
  color: #3b82f6;
}
.count-ok {
  color: #22c55e;
}

.diag-body {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

.empty-state {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}

.diag-section {
  margin-bottom: 4px;
}

.section-header {
  padding: 6px 12px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  display: flex;
  align-items: center;
  gap: 6px;
}

.section-count {
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  background: var(--border-color);
  border-radius: 10px;
  color: var(--text-primary);
}

.diag-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 12px;
  font-size: 12px;
  line-height: 1.5;
  border-bottom: 1px solid var(--border-color);
}

.diag-item:hover {
  background: var(--bg-tertiary);
}

.diag-icon {
  flex-shrink: 0;
  width: 14px;
  text-align: center;
  font-size: 11px;
  margin-top: 2px;
}

.diag-icon.severity-error {
  color: #ef4444;
}
.diag-icon.severity-warning {
  color: #f59e0b;
}
.diag-icon.severity-info {
  color: #3b82f6;
}

.diag-code {
  flex-shrink: 0;
  font-family: monospace;
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  padding: 1px 4px;
  border-radius: 3px;
  margin-top: 1px;
}

.diag-message {
  flex: 1;
  color: var(--text-primary);
  word-break: break-word;
}

.diag-location {
  flex-shrink: 0;
  font-family: monospace;
  font-size: 11px;
  color: var(--text-secondary);
  margin-top: 1px;
}
</style>
