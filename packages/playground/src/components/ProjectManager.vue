<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import type { Store } from "../core/store";
import { listProjects, type StoredProject } from "../core/projectStorage";
import { presets } from "../core/presets";

const props = defineProps<{
  store: Store;
}>();

const open = ref(false);
const showSaveAs = ref(false);
const saveAsName = ref("");
const dropdownEl = ref<HTMLElement>();

const projects = ref<StoredProject[]>([]);

function refreshProjects() {
  projects.value = listProjects();
}

const displayName = computed(() => props.store.currentProjectName ?? "Untitled");

function toggle() {
  open.value = !open.value;
  if (open.value) {
    refreshProjects();
    showSaveAs.value = false;
  }
}

function close() {
  open.value = false;
  showSaveAs.value = false;
}

function handleSave() {
  if (props.store.currentProjectName) {
    props.store.saveProject();
    close();
  } else {
    showSaveAs.value = true;
    saveAsName.value = "";
  }
}

function handleSaveAs() {
  const name = saveAsName.value.trim();
  if (!name) return;
  props.store.saveProject(name);
  refreshProjects();
  showSaveAs.value = false;
  close();
}

function handleNew() {
  // Reset to untitled state — load the default Counter preset
  const counter = presets[0];
  props.store.loadProject("", counter.state).then(() => {
    // Clear the project name so it shows as "Untitled"
    // @ts-expect-error — direct mutation for reset
    props.store.currentProjectName = null;
  });
  close();
}

function handleLoadPreset(preset: (typeof presets)[number]) {
  props.store.loadProject(preset.name, preset.state);
  close();
}

function handleLoadProject(project: StoredProject) {
  props.store.loadProject(project.name, project.state);
  close();
}

function handleDeleteProject(project: StoredProject, event: Event) {
  event.stopPropagation();
  props.store.deleteProject(project.name);
  refreshProjects();
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
  <div class="project-manager" ref="dropdownEl">
    <button class="project-btn" @click="toggle" :title="'Project: ' + displayName">
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      </svg>
      <span class="project-name">{{ displayName }}</span>
      <svg
        width="10"
        height="10"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </button>

    <div v-if="open" class="dropdown">
      <!-- Actions -->
      <div class="section actions-section">
        <button class="action-btn" @click="handleSave">Save</button>
        <button
          class="action-btn"
          @click="
            showSaveAs = true;
            saveAsName = '';
          "
        >
          Save As
        </button>
        <button class="action-btn" @click="handleNew">New</button>
      </div>

      <!-- Save As input -->
      <div v-if="showSaveAs" class="section save-as-section">
        <input
          v-model="saveAsName"
          class="save-as-input"
          placeholder="Project name..."
          @keydown.enter="handleSaveAs"
          @keydown.escape.stop="showSaveAs = false"
          autofocus
        />
        <button class="save-as-confirm" @click="handleSaveAs" :disabled="!saveAsName.trim()">
          OK
        </button>
      </div>

      <!-- Presets -->
      <div class="section">
        <div class="section-label">Presets</div>
        <button
          v-for="preset in presets"
          :key="preset.name"
          class="item-btn"
          @click="handleLoadPreset(preset)"
          :title="preset.description"
        >
          <span class="item-name">{{ preset.name }}</span>
          <span class="item-desc">{{ preset.description }}</span>
        </button>
      </div>

      <!-- My Projects -->
      <div v-if="projects.length > 0" class="section">
        <div class="section-label">My Projects</div>
        <button
          v-for="project in projects"
          :key="project.name"
          class="item-btn"
          @click="handleLoadProject(project)"
        >
          <span class="item-name">{{ project.name }}</span>
          <button
            class="delete-btn"
            @click="handleDeleteProject(project, $event)"
            title="Delete project"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.project-manager {
  position: relative;
}

.project-btn {
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

.project-btn:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

.project-name {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  width: 280px;
  max-height: 400px;
  overflow-y: auto;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  z-index: 100;
}

.section {
  padding: 6px;
  border-bottom: 1px solid var(--border-color);
}

.section:last-child {
  border-bottom: none;
}

.section-label {
  padding: 4px 8px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  color: var(--text-muted, var(--text-secondary));
  letter-spacing: 0.5px;
}

.actions-section {
  display: flex;
  gap: 4px;
}

.action-btn {
  flex: 1;
  padding: 6px 8px;
  font-size: 12px;
  font-weight: 500;
  border-radius: 4px;
  background: var(--bg-tertiary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  cursor: pointer;
}

.action-btn:hover {
  background: var(--accent-color);
  color: white;
  border-color: var(--accent-color);
}

.save-as-section {
  display: flex;
  gap: 4px;
}

.save-as-input {
  flex: 1;
  padding: 6px 8px;
  font-size: 12px;
  border-radius: 4px;
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-primary);
  outline: none;
}

.save-as-input:focus {
  border-color: var(--accent-color);
}

.save-as-confirm {
  padding: 6px 12px;
  font-size: 12px;
  font-weight: 500;
  border-radius: 4px;
  background: var(--accent-color);
  color: white;
  border: none;
  cursor: pointer;
}

.save-as-confirm:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.item-btn {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 6px 8px;
  font-size: 12px;
  border-radius: 4px;
  background: transparent;
  color: var(--text-primary);
  border: none;
  cursor: pointer;
  text-align: left;
  gap: 8px;
}

.item-btn:hover {
  background: var(--bg-tertiary);
}

.item-name {
  font-weight: 500;
  white-space: nowrap;
}

.item-desc {
  flex: 1;
  color: var(--text-muted, var(--text-secondary));
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.delete-btn {
  padding: 2px;
  border-radius: 3px;
  background: transparent;
  color: var(--text-secondary);
  border: none;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.delete-btn:hover {
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
}
</style>
