<script setup lang="ts">
import { ref, onMounted } from "vue";
import type { Store } from "../core/store";

const props = defineProps<{
  store: Store;
}>();

interface VueVersion {
  version: string;
  label: string;
}

const versions = ref<VueVersion[]>([]);
const loading = ref(false);
const open = ref(false);

onMounted(async () => {
  loading.value = true;
  try {
    const resp = await fetch(
      "https://data.jsdelivr.com/v1/packages/npm/vue/resolved?specifier=>=3.0.0",
    );
    if (resp.ok) {
      const data = await resp.json();
      // data is { version: "3.5.26" } for a single version
      // For multiple versions, use the tags endpoint
    }

    // Fetch available tags/versions
    const tagsResp = await fetch(
      "https://data.jsdelivr.com/v1/packages/npm/vue",
    );
    if (tagsResp.ok) {
      const data = await tagsResp.json();
      const allVersions: string[] = data.versions ?? [];
      // Filter to Vue 3.x releases: stable + beta (no alpha/rc)
      const vue3 = allVersions
        .filter((v: string) => v.startsWith("3.") && (!v.includes("-") || v.includes("-beta")))
        .reverse(); // newest first

      // Take the last 30 for a reasonable list
      versions.value = vue3.slice(0, 30).map((v: string) => ({
        version: v,
        label: v.includes("-") ? `v${v}` : `v${v}`,
      }));
    }
  } catch {
    // Fallback to a few known versions
    versions.value = [
      { version: "3.6.0-beta.7", label: "v3.6.0-beta.7" },
      { version: "3.5.26", label: "v3.5.26" },
      { version: "3.5.13", label: "v3.5.13" },
      { version: "3.4.38", label: "v3.4.38" },
    ];
  } finally {
    loading.value = false;
  }
});

function selectVersion(entry: VueVersion) {
  open.value = false;
  if (entry.version === props.store.vueVersion) return;
  props.store.setVueVersion(entry.version);
}

function closeDropdown(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (!target.closest(".vue-version-select")) {
    open.value = false;
  }
}

onMounted(() => {
  document.addEventListener("click", closeDropdown);
});
</script>

<template>
  <div class="vue-version-select">
    <button class="vue-version-btn" @click.stop="open = !open" title="Vue version">
      <svg width="14" height="14" viewBox="0 0 256 221" fill="none" xmlns="http://www.w3.org/2000/svg">
        <path d="M204.8 0H256L128 220.8L0 0H97.92L128 51.2L157.44 0H204.8Z" fill="#41B883"/>
        <path d="M0 0L128 220.8L256 0H204.8L128 141.44L50.56 0H0Z" fill="#41B883"/>
        <path d="M50.56 0L128 141.44L204.8 0H157.44L128 51.2L97.92 0H50.56Z" fill="#35495E"/>
      </svg>
      <span>{{ store.vueVersion }}</span>
      <span class="caret">&#9662;</span>
    </button>
    <div v-if="open" class="dropdown">
      <div v-if="loading" class="dropdown-item loading-item">Loading versions...</div>
      <template v-else>
        <div
          v-for="entry in versions"
          :key="entry.version"
          class="dropdown-item"
          :class="{ active: entry.version === store.vueVersion }"
          @click="selectVersion(entry)"
        >
          {{ entry.label }}
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.vue-version-select {
  position: relative;
}

.vue-version-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  cursor: pointer;
  white-space: nowrap;
}

.vue-version-btn:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

.caret {
  font-size: 10px;
  margin-left: 2px;
}

.dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  margin-top: 4px;
  min-width: 160px;
  max-height: 300px;
  overflow-y: auto;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 100;
}

.dropdown-item {
  padding: 8px 12px;
  font-size: 12px;
  font-family: monospace;
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
  font-family: inherit;
}
</style>
