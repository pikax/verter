<script setup lang="ts">
import { onMounted, provide, ref } from "vue";
import type { ProvenanceChain } from "@verter/types/audit.generated";
import { useStore } from "./core/store";
import Header from "./components/Header.vue";
import SplitPane from "./components/SplitPane.vue";
import Message from "./components/Message.vue";
import AuditTree from "./components/AuditTree.vue";
import FileSelector from "./editor/FileSelector.vue";
import Editor from "./editor/Editor.vue";
import Output from "./output/Output.vue";

const store = useStore();

provide("store", store);

// "Why?" tab for the playground. The chain is
// populated by the audit pipeline once the playground wires a
// `getComponentMetaWithAudit` call against the WASM session; until
// then the tab renders the component's empty state with the
// enable-footprint hint.
const provenanceChain = ref<ProvenanceChain | null>(null);
const showWhyTab = ref(false);

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
          <div class="output-panel">
            <nav class="output-tabs">
              <button
                type="button"
                class="output-tab"
                :class="{ active: !showWhyTab }"
                @click="showWhyTab = false"
              >
                Output
              </button>
              <button
                type="button"
                class="output-tab"
                :class="{ active: showWhyTab }"
                @click="showWhyTab = true"
                title="Provenance — see which files loaded + derivation chain"
              >
                Why?
              </button>
            </nav>
            <div v-show="!showWhyTab" class="output-tab-body">
              <Output :store="store" />
            </div>
            <div v-show="showWhyTab" class="output-tab-body">
              <AuditTree :chain="provenanceChain" />
            </div>
          </div>
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

.output-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.output-tabs {
  display: flex;
  border-bottom: 1px solid var(--border-color, #ccc);
  flex-shrink: 0;
}

.output-tab {
  padding: 0.4rem 0.9rem;
  border: none;
  background: none;
  cursor: pointer;
  font: inherit;
  color: var(--text-secondary, #888);
  border-bottom: 2px solid transparent;
}

.output-tab.active {
  color: var(--text, #222);
  border-bottom-color: var(--accent-color, #4285f4);
}

.output-tab-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
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
