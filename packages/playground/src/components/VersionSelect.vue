<script setup lang="ts">
import { ref, onMounted } from "vue";
import type { Store } from "../core/store";
import type { VersionEntry } from "../core/versions";
import { fetchVersions } from "../core/versions";

const props = defineProps<{
  store: Store;
}>();

const versions = ref<VersionEntry[]>([]);
const loading = ref(false);
const open = ref(false);

onMounted(async () => {
  loading.value = true;
  try {
    versions.value = await fetchVersions();
  } finally {
    loading.value = false;
  }
});

async function selectVersion(entry: VersionEntry) {
  open.value = false;
  if (entry.id === props.store.verterVersion) return;
  await props.store.switchVerterVersion(entry);
}

function currentLabel(): string {
  const current = versions.value.find((v) => v.id === props.store.verterVersion);
  return current?.label ?? "This Build";
}

function closeDropdown(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (!target.closest(".version-select")) {
    open.value = false;
  }
}

onMounted(() => {
  document.addEventListener("click", closeDropdown);
});
</script>

<template>
  <div class="version-select">
    <button class="version-btn" @click.stop="open = !open" :disabled="props.store.versionLoading">
      <span v-if="props.store.versionLoading" class="spinner"></span>
      <span v-else>{{ currentLabel() }}</span>
      <span class="caret">&#9662;</span>
    </button>
    <div v-if="open" class="dropdown">
      <div v-if="loading" class="dropdown-item loading-item">Loading versions...</div>
      <template v-else>
        <div class="dropdown-section">Current</div>
        <template v-for="entry in versions" :key="entry.id">
          <div
            v-if="entry.type === 'local'"
            class="dropdown-item"
            :class="{ active: entry.id === props.store.verterVersion }"
            @click="selectVersion(entry)"
          >
            {{ entry.label }}
          </div>
        </template>

        <template v-if="versions.some((v) => v.type === 'release')">
          <div class="dropdown-section">Releases</div>
          <template v-for="entry in versions" :key="entry.id">
            <div
              v-if="entry.type === 'release'"
              class="dropdown-item"
              :class="{ active: entry.id === props.store.verterVersion }"
              @click="selectVersion(entry)"
            >
              {{ entry.label }}
            </div>
          </template>
        </template>

        <template v-if="versions.some((v) => v.type === 'commit')">
          <div class="dropdown-section">Nightly Commits</div>
          <template v-for="entry in versions" :key="entry.id">
            <div
              v-if="entry.type === 'commit'"
              class="dropdown-item commit-item"
              :class="{ active: entry.id === props.store.verterVersion }"
              @click="selectVersion(entry)"
            >
              <code>{{ entry.sha }}</code>
              <span class="commit-msg">{{ entry.label.split(" - ").slice(1).join(" - ") }}</span>
            </div>
          </template>
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
.version-select {
  position: relative;
}

.version-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  cursor: pointer;
  white-space: nowrap;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.version-btn:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

.version-btn:disabled {
  opacity: 0.6;
  cursor: wait;
}

.caret {
  font-size: 10px;
  margin-left: 2px;
}

.spinner {
  display: inline-block;
  width: 12px;
  height: 12px;
  border: 2px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  margin-top: 4px;
  min-width: 280px;
  max-height: 400px;
  overflow-y: auto;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 100;
}

.dropdown-section {
  padding: 6px 12px 2px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  color: var(--text-secondary);
  letter-spacing: 0.5px;
}

.dropdown-item {
  padding: 8px 12px;
  font-size: 12px;
  cursor: pointer;
  color: var(--text-primary);
}

.dropdown-item:hover {
  background: var(--bg-tertiary);
}

.dropdown-item.active {
  background: var(--accent-color);
  color: white;
}

.loading-item {
  color: var(--text-secondary);
  cursor: default;
}

.commit-item {
  display: flex;
  align-items: center;
  gap: 8px;
}

.commit-item code {
  font-size: 11px;
  font-family: monospace;
  background: var(--bg-tertiary);
  padding: 1px 4px;
  border-radius: 3px;
  flex-shrink: 0;
}

.commit-item.active code {
  background: rgba(255, 255, 255, 0.2);
}

.commit-msg {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
