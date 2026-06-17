<script setup lang="ts">
import type { Store } from "../core/store";
import ProjectManager from "./ProjectManager.vue";
import LanguageSelect from "./LanguageSelect.vue";
import VersionSelect from "./VersionSelect.vue";
import VueVersionSelect from "./VueVersionSelect.vue";

const props = defineProps<{
  store: Store;
}>();

function verterTimingTitle(): string {
  const { verterNewJs } = props.store.compileTiming;
  let title = "Verter: Vue SFC compilation";
  if (verterNewJs !== null) {
    title += ` (${verterNewJs.toFixed(1)}ms)`;
  }
  return title;
}
</script>

<template>
  <header class="header">
    <div class="logo">
      <img src="/logo.svg" alt="Verter" class="logo-img" />
      <span class="logo-text">Verter Playground</span>
    </div>
    <ProjectManager :store="store" />
    <LanguageSelect :store="store" />
    <VersionSelect :store="store" />
    <VueVersionSelect :store="store" />
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
        :class="{ active: store.compilerOptions.strictSlots }"
        @click="store.toggleStrictSlots"
        title="Toggle strict slot type checking"
      >
        Slots {{ store.compilerOptions.strictSlots ? "STRICT" : "OFF" }}
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
        class="type-checker-toggle"
        :title="`Type checker: ${store.typeChecker} (${store.typeCheckerStatus})`"
      >
        <button
          class="tc-btn"
          :class="{ active: store.typeChecker === 'tsc' }"
          @click="store.setTypeChecker('tsc')"
        >
          tsc
        </button>
        <button
          class="tc-btn"
          :class="{ active: store.typeChecker === 'tsgo' }"
          @click="store.setTypeChecker('tsgo')"
        >
          tsgo
        </button>
        <span
          v-if="store.typeCheckerStatus !== 'active'"
          class="tc-status"
          :class="'tc-status-' + store.typeCheckerStatus"
          :title="
            store.typeCheckerStatus === 'unavailable'
              ? 'Type checker unavailable (SharedArrayBuffer requires COOP/COEP headers)'
              : 'Initializing...'
          "
        >
          {{ store.typeCheckerStatus === "unavailable" ? "!" : "..." }}
        </span>
      </div>
      <div class="timing" v-if="store.compileTiming.verterNewJs !== null">
        <span class="timing-item" :title="verterTimingTitle()">
          V: {{ store.compileTiming.verterNewJs.toFixed(1) }}ms
        </span>
      </div>
      <a
        class="icon-link"
        href="https://verterjs.dev"
        target="_blank"
        rel="noopener"
        title="Documentation"
      >
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
          <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
        </svg>
      </a>
      <a
        class="icon-link"
        href="https://github.com/pikax/verter"
        target="_blank"
        rel="noopener"
        title="GitHub"
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0 0 24 12c0-6.63-5.37-12-12-12z"
          />
        </svg>
      </a>
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

.icon-link {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  text-decoration: none;
}

.icon-link:hover {
  background: var(--border-color);
  color: var(--text-primary);
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

.type-checker-toggle {
  display: flex;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  overflow: hidden;
}

.tc-btn {
  padding: 4px 10px;
  font-size: 11px;
  font-weight: 600;
  font-family: monospace;
  border: none;
  cursor: pointer;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  transition: all 0.15s ease;
}

.tc-btn:not(:last-child) {
  border-right: 1px solid var(--border-color);
}

.tc-btn:hover {
  color: var(--text-primary);
}

.tc-btn.active {
  background: var(--accent-color);
  color: white;
}

.tc-status {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 6px;
  font-size: 10px;
  font-weight: 700;
  border-left: 1px solid var(--border-color);
}

.tc-status-unavailable {
  color: #ef4444;
  background: rgba(239, 68, 68, 0.1);
}

.tc-status-initializing {
  color: #f59e0b;
  background: rgba(245, 158, 11, 0.1);
}
</style>
