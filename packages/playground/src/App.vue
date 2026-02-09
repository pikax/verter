<script setup lang="ts">
import { onMounted, provide } from "vue";
import { useStore } from "./core/store";
import Header from "./components/Header.vue";
import SplitPane from "./components/SplitPane.vue";
import Message from "./components/Message.vue";
import FileSelector from "./editor/FileSelector.vue";
import Editor from "./editor/Editor.vue";
import Output from "./output/Output.vue";

const store = useStore();

provide("store", store);

onMounted(async () => {
  if (store.darkMode) {
    document.documentElement.classList.add("dark");
  }
  await store.init();
});
</script>

<template>
  <div class="playground">
    <Header :store="store" />
    <div class="main-content">
      <div v-if="store.loading" class="loading">
        <div class="loading-spinner" />
        <span>Initializing compiler...</span>
      </div>
      <SplitPane v-else :initial-split="50">
        <template #first>
          <div class="editor-panel">
            <FileSelector :store="store" />
            <Editor :store="store" />
            <Message :errors="store.errors" />
          </div>
        </template>
        <template #second>
          <Output :store="store" />
        </template>
      </SplitPane>
    </div>
  </div>
</template>

<style scoped>
.playground {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.main-content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.editor-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.loading {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 16px;
  color: var(--text-secondary);
}

.loading-spinner {
  width: 40px;
  height: 40px;
  border: 3px solid var(--border-color);
  border-top-color: var(--accent-color);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
