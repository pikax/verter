<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import type { Store } from "../core/store";
import { languageOptions, type LanguageOption } from "../core/frameworks";

const props = defineProps<{
  store: Store;
}>();

const open = ref(false);
const dropdownEl = ref<HTMLElement>();

// The selectable options: an explicit Auto state plus every registered
// framework (manifest order). The list IS the manifest — see languageOptions().
const options = computed<LanguageOption[]>(() => languageOptions());

const currentLabel = computed(() => {
  if (props.store.languagePin === null) return `Auto (${props.store.effectiveLanguage})`;
  return props.store.languagePin;
});

function toggle() {
  open.value = !open.value;
}

function close() {
  open.value = false;
}

async function pick(option: LanguageOption) {
  if (option.id === null) {
    props.store.unpinLanguage();
  } else {
    await props.store.selectFramework(option.id);
  }
  close();
}

function isActive(option: LanguageOption): boolean {
  return option.id === props.store.languagePin;
}

function onClickOutside(e: MouseEvent) {
  if (dropdownEl.value && !dropdownEl.value.contains(e.target as Node)) {
    close();
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") close();
}

onMounted(() => {
  document.addEventListener("click", onClickOutside, true);
  document.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  document.removeEventListener("click", onClickOutside, true);
  document.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="language-select" ref="dropdownEl">
    <button class="language-btn" @click="toggle" :title="'Language: ' + currentLabel">
      <span class="language-name">{{ currentLabel }}</span>
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </button>

    <!-- Persistent experimental badge whenever the effective language is experimental. -->
    <span v-if="store.isExperimentalLanguage" class="experimental-badge" title="Experimental — not production ready">
      experimental
    </span>

    <div v-if="open" class="dropdown">
      <button
        v-for="option in options"
        :key="option.id ?? '__auto__'"
        class="option-btn"
        :class="{ active: isActive(option) }"
        @click="pick(option)"
      >
        <span class="option-name">{{ option.label }}</span>
        <span v-if="option.experimental" class="option-experimental">experimental</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.language-select {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
}

.language-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  cursor: pointer;
  transition: all 0.15s ease;
}

.language-btn:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

.language-name {
  text-transform: capitalize;
}

.experimental-badge {
  padding: 2px 8px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  border-radius: 10px;
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
  white-space: nowrap;
}

.dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  min-width: 180px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 100;
  padding: 6px;
}

.option-btn {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 6px 8px;
  font-size: 13px;
  border-radius: 4px;
  background: transparent;
  color: var(--text-primary);
  border: none;
  cursor: pointer;
  text-align: left;
  gap: 8px;
}

.option-btn:hover {
  background: var(--bg-tertiary);
}

.option-btn.active {
  background: var(--accent-color);
  color: white;
}

.option-name {
  text-transform: capitalize;
}

.option-experimental {
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: #f59e0b;
}

.option-btn.active .option-experimental {
  color: white;
}
</style>
