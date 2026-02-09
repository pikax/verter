<script setup lang="ts">
import { ref } from 'vue'
import { IMPORT_MAP_FILENAME, type Store } from '../core/store'

const props = defineProps<{
  store: Store
}>()

const showNewFileInput = ref(false)
const newFilename = ref('')

function handleAddFile() {
  if (showNewFileInput.value && newFilename.value.trim()) {
    let filename = newFilename.value.trim()
    if (!filename.includes('.')) {
      filename += '.vue'
    }
    props.store.addFile(filename)
    newFilename.value = ''
    showNewFileInput.value = false
  } else {
    showNewFileInput.value = true
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    handleAddFile()
  } else if (e.key === 'Escape') {
    showNewFileInput.value = false
    newFilename.value = ''
  }
}
</script>

<template>
  <div class="file-selector">
    <div class="tabs">
      <button
        v-for="(file, filename) in store.files"
        :key="filename"
        class="tab"
        :class="{ active: filename === store.activeFilename }"
        @click="store.setActiveFile(filename as string)"
      >
        <span class="filename">{{ filename }}</span>
        <span
          v-if="filename !== store.mainFile"
          class="close"
          @click.stop="store.deleteFile(filename as string)"
        >
          &times;
        </span>
      </button>
    </div>
    <div class="actions">
      <button
        class="tab import-map-tab"
        :class="{ active: store.activeFilename === IMPORT_MAP_FILENAME }"
        @click="store.setActiveFile(IMPORT_MAP_FILENAME)"
        title="Edit Import Map"
      >
        Import Map
      </button>
      <input
        v-if="showNewFileInput"
        v-model="newFilename"
        class="new-file-input"
        placeholder="filename.vue"
        @keydown="handleKeydown"
        @blur="showNewFileInput = false"
        autofocus
      />
      <button class="add-btn" @click="handleAddFile" title="Add file">+</button>
    </div>
  </div>
</template>

<style scoped>
.file-selector {
  display: flex;
  align-items: center;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  height: 36px;
  padding: 0 8px;
}

.tabs {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  flex: 1;
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: var(--tab-inactive-bg);
  border-radius: 4px 4px 0 0;
  font-size: 13px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.tab.active {
  background: var(--tab-active-bg);
  color: var(--text-primary);
}

.tab:hover {
  color: var(--text-primary);
}

.close {
  font-size: 16px;
  line-height: 1;
  opacity: 0.5;
}

.close:hover {
  opacity: 1;
  color: var(--error-color);
}

.actions {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: 8px;
}

.new-file-input {
  width: 120px;
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
}

.import-map-tab {
  font-size: 12px;
  font-style: italic;
  border: 1px dashed var(--border-color);
  background: transparent;
}

.import-map-tab.active {
  border-style: solid;
}

.add-btn {
  width: 24px;
  height: 24px;
  border-radius: 4px;
  background: var(--accent-color);
  color: white;
  font-size: 18px;
  font-weight: bold;
  display: flex;
  align-items: center;
  justify-content: center;
}

.add-btn:hover {
  background: var(--accent-hover);
}
</style>
