<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, shallowRef } from "vue";
import * as monaco from "monaco-editor-core";
import type { Store } from "../core/store";

const props = defineProps<{ store: Store }>();

interface VirtualNode {
  kind: string;
  index?: number;
  label: string;
}

const virtualFiles = computed<VirtualNode[]>(() => {
  const file = props.store.activeFile;
  if (!file) return [];
  // Access the compiled data to know what virtual nodes exist
  const nodes: VirtualNode[] = [];
  if (file.compiled.js) {
    nodes.push({ kind: "script", label: "script" });
  }
  if (file.compiled.templateCode) {
    nodes.push({ kind: "template", label: "template" });
  }
  if (file.compiled.css) {
    nodes.push({ kind: "style", index: 0, label: "style[0]" });
  }
  if (file.compiled.types) {
    nodes.push({ kind: "tsx", label: "IDE (TSX)" });
  }
  if (file.compiled.tscCode) {
    nodes.push({ kind: "tsc", label: "API (.d.ts)" });
  }
  if (file.compiled.ssrCode) {
    nodes.push({ kind: "ssr", label: "SSR" });
  }
  return nodes;
});

const selectedNode = ref<string>("script");

watch(virtualFiles, (files) => {
  if (files.length > 0 && !files.find((f) => f.kind === selectedNode.value)) {
    selectedNode.value = files[0].kind;
  }
});

const selectedCode = computed(() => {
  const file = props.store.activeFile;
  if (!file) return "";
  switch (selectedNode.value) {
    case "script":
      return file.compiled.js;
    case "template":
      return file.compiled.templateCode;
    case "style":
      return file.compiled.css;
    case "tsx":
      return file.compiled.types;
    case "tsc":
      return file.compiled.tscCode;
    case "ssr":
      return file.compiled.ssrCode;
    default:
      return "";
  }
});

const selectedLanguage = computed(() => {
  switch (selectedNode.value) {
    case "script":
    case "template":
    case "ssr":
      return "javascript";
    case "style":
      return "css";
    case "tsx":
    case "tsc":
      return "typescript";
    default:
      return "plaintext";
  }
});

const editorContainer = ref<HTMLElement>();
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>();

onMounted(() => {
  if (!editorContainer.value) return;

  editor.value = monaco.editor.create(editorContainer.value, {
    value: selectedCode.value || "// No output",
    language: selectedLanguage.value,
    theme: props.store.darkMode ? "vs-dark" : "vs",
    readOnly: true,
    minimap: { enabled: false },
    fontSize: 13,
    lineNumbers: "on",
    renderLineHighlight: "none",
    scrollBeyondLastLine: false,
    automaticLayout: true,
    folding: true,
    wordWrap: "on",
    domReadOnly: true,
    contextmenu: false,
  });
});

watch(selectedCode, (newCode) => {
  if (editor.value) {
    editor.value.setValue(newCode || "// No output");
  }
});

watch(selectedLanguage, (newLang) => {
  if (editor.value) {
    const model = editor.value.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, newLang);
    }
  }
});

watch(
  () => props.store.darkMode,
  (dark) => {
    if (editor.value) {
      monaco.editor.setTheme(dark ? "vs-dark" : "vs");
    }
  },
);

onUnmounted(() => {
  editor.value?.dispose();
});
</script>

<template>
  <div class="vfiles-panel">
    <div class="vfiles-sidebar">
      <button
        v-for="node in virtualFiles"
        :key="node.kind + (node.index ?? '')"
        class="vfile-btn"
        :class="{ active: selectedNode === node.kind }"
        @click="selectedNode = node.kind"
      >
        {{ node.label }}
      </button>
      <div v-if="virtualFiles.length === 0" class="empty-state">No virtual files</div>
    </div>
    <div class="vfiles-code">
      <div ref="editorContainer" class="editor-container" />
    </div>
  </div>
</template>

<style scoped>
.vfiles-panel {
  height: 100%;
  display: flex;
}
.vfiles-sidebar {
  width: 120px;
  min-width: 120px;
  border-right: 1px solid var(--border-color);
  padding: 4px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.vfile-btn {
  padding: 6px 8px;
  font-size: 12px;
  text-align: left;
  border-radius: 4px;
  color: var(--text-secondary);
  background: transparent;
}
.vfile-btn.active {
  background: var(--tab-active-bg);
  color: var(--text-primary);
}
.vfile-btn:hover {
  background: var(--bg-tertiary);
}
.vfiles-code {
  flex: 1;
  overflow: hidden;
}
.editor-container {
  height: 100%;
  width: 100%;
}
.empty-state {
  color: var(--text-secondary);
  text-align: center;
  padding: 16px 4px;
  font-size: 12px;
}
</style>
