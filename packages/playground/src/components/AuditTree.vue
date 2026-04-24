<script setup lang="ts">
/**
 * AuditTree — renders a `ProvenanceChain` JSON as an interactive
 * tree. Plan §3 Commit 8.
 *
 * This component is display-only: it never walks the audit graph
 * itself (plan §2.8 — the walker is in Rust, TS consumers render
 * JSON). Given a `chain: ProvenanceChain`, it displays the steps
 * BFS-ordered with collapsible sections per depth-level and
 * surface the `terminated` + `shared_load_terminals` trailers.
 */
import { computed, ref } from "vue";
import type { ProvenanceChain, ProvenanceStep } from "@verter/types/audit.generated";

const props = defineProps<{
  /** Provenance chain returned by `whyLoaded` / `whyInstantiated`. */
  chain: ProvenanceChain | null;
  /** Optional display title. Defaults to "Provenance". */
  title?: string;
}>();

const collapsed = ref<Record<number, boolean>>({});
function toggle(depth: number): void {
  collapsed.value[depth] = !collapsed.value[depth];
}

const groupedSteps = computed<Array<{ depth: number; steps: ProvenanceStep[] }>>(() => {
  if (!props.chain) return [];
  const byDepth = new Map<number, ProvenanceStep[]>();
  for (const step of props.chain.steps) {
    const bucket = byDepth.get(step.depth) ?? [];
    bucket.push(step);
    byDepth.set(step.depth, bucket);
  }
  return Array.from(byDepth.entries())
    .sort(([a], [b]) => a - b)
    .map(([depth, steps]) => ({ depth, steps }));
});

const terminationLabel = computed<string>(() => {
  const t = props.chain?.terminated;
  if (!t) return "";
  if (t === "Complete") return "Complete";
  if (t === "NotFound") return "Not found";
  if (typeof t === "object") {
    if ("DepthExceeded" in t) return `Depth exceeded (cap=${t.DepthExceeded.cap})`;
    if ("Cycle" in t) return `Cycle (at edge #${t.Cycle.at_edge.toString()})`;
  }
  return JSON.stringify(t);
});
</script>

<template>
  <section class="audit-tree">
    <header class="audit-tree-header">
      <h3>{{ props.title ?? "Provenance" }}</h3>
    </header>

    <div v-if="!props.chain" class="audit-tree-empty">
      No audit bundle attached. Enable <code>audit_enabled</code> +
      <code>footprint_capture</code> on the host, then run a component-meta query.
    </div>

    <div v-else-if="props.chain.root === null" class="audit-tree-empty">
      Chain has no root. Termination: {{ terminationLabel }}.
    </div>

    <div v-else>
      <div class="audit-tree-root">
        <strong>root:</strong> NodeId({{ props.chain.root!.toString() }})
      </div>

      <div
        v-for="group in groupedSteps"
        :key="group.depth"
        class="audit-tree-group"
      >
        <button
          type="button"
          class="audit-tree-group-toggle"
          @click="toggle(group.depth)"
        >
          <span class="chevron">{{ collapsed[group.depth] ? "▸" : "▾" }}</span>
          depth {{ group.depth }} &middot; {{ group.steps.length }} step(s)
        </button>

        <ul v-show="!collapsed[group.depth]" class="audit-tree-step-list">
          <li
            v-for="step in group.steps"
            :key="step.edge_id.toString()"
            class="audit-tree-step"
          >
            <div class="audit-tree-step-line">
              <span class="audit-tree-edge">
                edge #{{ step.edge_id.toString() }}
              </span>
              <code class="audit-tree-node-label">{{ step.node_label }}</code>
              <span class="audit-tree-kind">({{ step.edge.kind }})</span>
            </div>
          </li>
        </ul>
      </div>

      <footer class="audit-tree-termination">
        terminated: <strong>{{ terminationLabel }}</strong>
      </footer>

      <div
        v-if="props.chain.shared_load_terminals.length > 0"
        class="audit-tree-shared-loads"
      >
        <h4>Shared-load terminals</h4>
        <ul>
          <li
            v-for="t in props.chain.shared_load_terminals"
            :key="`${t.canonical_id}-${t.winner_request_id.toString()}`"
          >
            <code>{{ t.canonical_id }}</code>
            — winner request id
            <code>{{ t.winner_request_id.toString() }}</code>
            (<span v-if="t.winner_audited">audited</span
            ><span v-else>not audited</span>)
          </li>
        </ul>
      </div>
    </div>
  </section>
</template>

<style scoped>
.audit-tree {
  font-family: var(--monospace, monospace);
  font-size: 0.85em;
  padding: 0.5rem;
  border-left: 2px solid var(--border, #ccc);
}

.audit-tree-header h3 {
  margin: 0 0 0.5rem 0;
  font-size: 1em;
  font-weight: 600;
}

.audit-tree-empty {
  color: var(--fg-muted, #888);
  font-style: italic;
}

.audit-tree-root {
  margin-bottom: 0.5rem;
  color: var(--accent, #4285f4);
}

.audit-tree-group {
  margin-bottom: 0.5rem;
}

.audit-tree-group-toggle {
  background: none;
  border: none;
  padding: 0;
  font: inherit;
  color: inherit;
  cursor: pointer;
  text-align: left;
  width: 100%;
  font-weight: 600;
}

.audit-tree-group-toggle .chevron {
  display: inline-block;
  width: 1em;
}

.audit-tree-step-list {
  list-style: none;
  padding-left: 1.5em;
  margin: 0.25rem 0;
}

.audit-tree-step-line {
  display: flex;
  gap: 0.5em;
  align-items: center;
  padding: 0.1rem 0;
}

.audit-tree-edge {
  color: var(--fg-muted, #888);
}

.audit-tree-node-label {
  color: var(--fg, #333);
}

.audit-tree-kind {
  color: var(--accent-dim, #1a73e8);
}

.audit-tree-termination {
  margin-top: 0.75rem;
  padding-top: 0.5rem;
  border-top: 1px dashed var(--border, #ccc);
  color: var(--fg-muted, #888);
}

.audit-tree-shared-loads {
  margin-top: 0.5rem;
}

.audit-tree-shared-loads h4 {
  margin: 0 0 0.25rem 0;
  font-size: 0.95em;
}

.audit-tree-shared-loads ul {
  list-style: disc;
  padding-left: 1.5em;
  margin: 0;
}
</style>
