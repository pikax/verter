<script setup lang="ts">
import type { Store } from "../core/store";

const props = defineProps<{
  store: Store;
}>();
</script>

<template>
  <div class="preview-unsupported">
    <div class="badge">experimental</div>
    <h2>Live preview is not yet supported for {{ props.store.effectiveLanguage }}</h2>
    <p>
      {{ props.store.effectiveLanguage }} ships first-class for selection, presets,
      compilation, IDE TSX, public API, analysis, and lint — but the in-browser
      runtime preview is not available yet.
    </p>
    <p class="hint">Switch to the <strong>Files</strong> or <strong>Analysis</strong> tab to inspect the output.</p>
  </div>
</template>

<style scoped>
.preview-unsupported {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 2rem;
  text-align: center;
  color: var(--text-secondary);
}

.badge {
  padding: 3px 10px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  border-radius: 10px;
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
}

.preview-unsupported h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
  text-transform: capitalize;
}

.preview-unsupported p {
  max-width: 420px;
  font-size: 13px;
  line-height: 1.5;
}

.hint {
  font-size: 12px;
  opacity: 0.85;
}
</style>
