<script setup lang="ts">
import type { Store } from "../core/store";
import VersionSelect from "./VersionSelect.vue";

const props = defineProps<{
  store: Store;
}>();

function verterTimingTitle(): string {
  const { verter, verterNative } = props.store.compileTiming;
  let title = "Verter: Vue SFC → TypeScript";
  if (verterNative !== null) {
    title += ` (native: ${verterNative.toFixed(1)}ms)`;
  }
  return title;
}
</script>

<template>
  <header class="header">
    <div class="logo">
      <img src="/verter-logo.svg" alt="Verter" class="logo-img" />
      <span class="logo-text">Verter Playground</span>
    </div>
    <VersionSelect :store="store" />
    <div class="actions">
      <button
        class="toggle-btn"
        :class="{ active: store.compilerOptions.isProduction }"
        @click="store.toggleProduction"
        title="Toggle Production mode"
      >
        {{ store.compilerOptions.isProduction ? "PROD" : "DEV" }}
      </button>
      <button
        class="toggle-btn"
        :class="{ active: store.compilerOptions.ssr }"
        @click="store.toggleSSR"
        title="Toggle SSR mode"
      >
        SSR {{ store.compilerOptions.ssr ? "ON" : "OFF" }}
      </button>
      <button
        class="toggle-btn"
        :class="{ active: store.autoSave }"
        @click="store.toggleAutoSave"
        :title="store.autoSave ? 'Auto-save enabled' : 'Manual save (Ctrl+S)'"
      >
        Auto {{ store.autoSave ? "ON" : "OFF" }}
      </button>
      <div
        class="timing"
        v-if="store.compileTiming.verter !== null || store.compileTiming.oxc !== null"
      >
        <span
          v-if="store.compileTiming.verter !== null"
          class="timing-item"
          :title="verterTimingTitle()"
        >
          V: {{ store.compileTiming.verter.toFixed(1) }}ms
        </span>
        <span
          v-if="store.compileTiming.oxc !== null"
          class="timing-item"
          title="OXC: TypeScript → JavaScript"
        >
          O: {{ store.compileTiming.oxc.toFixed(1) }}ms
        </span>
      </div>
      <button
        class="theme-toggle"
        @click="store.toggleDarkMode"
        :title="store.darkMode ? 'Light mode' : 'Dark mode'"
      >
        <span v-if="store.darkMode">☀️</span>
        <span v-else>🌙</span>
      </button>
    </div>
  </header>
</template>

<style scoped>
.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  height: 48px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.logo {
  display: flex;
  align-items: center;
  gap: 10px;
}

.logo-img {
  height: 28px;
  width: 28px;
}

.logo-text {
  font-weight: 600;
  font-size: 16px;
  color: var(--text-primary);
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.toggle-btn {
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  transition: all 0.15s ease;
}

.toggle-btn:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

.toggle-btn.active {
  background: var(--accent-color);
  color: white;
  border-color: var(--accent-color);
}

.timing {
  display: flex;
  gap: 8px;
  padding: 4px 10px;
  background: var(--bg-tertiary);
  border-radius: 6px;
  border: 1px solid var(--border-color);
}

.timing-item {
  font-size: 11px;
  font-family: monospace;
  color: var(--text-secondary);
}

.theme-toggle {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  font-size: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-tertiary);
}

.theme-toggle:hover {
  background: var(--border-color);
}
</style>
