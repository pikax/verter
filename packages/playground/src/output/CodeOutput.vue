<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, shallowRef } from "vue";
import * as monaco from "monaco-editor-core";
import type { Store } from "../core/store";
import type { OutputMode } from "../core/types";

const props = defineProps<{
  store: Store;
  mode: OutputMode;
}>();

const editorContainer = ref<HTMLElement>();
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>();

const code = computed(() => {
  const file = props.store.activeFile;
  if (!file) return "";

  switch (props.mode) {
    case "ts":
      return file.compiled.ts;
    case "js":
      return file.compiled.js;
    case "css":
      return file.compiled.css;
    default:
      return "";
  }
});

const language = computed(() => {
  switch (props.mode) {
    case "ts":
      return "typescript";
    case "js":
      return "javascript";
    case "css":
      return "css";
    default:
      return "plaintext";
  }
});

onMounted(() => {
  if (!editorContainer.value) return;

  editor.value = monaco.editor.create(editorContainer.value, {
    value: code.value || "// No output",
    language: language.value,
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

// Watch for code changes
watch(code, (newCode) => {
  if (editor.value) {
    editor.value.setValue(newCode || "// No output");
  }
});

// Watch for language changes
watch(language, (newLang) => {
  if (editor.value) {
    const model = editor.value.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, newLang);
    }
  }
});

// Watch for dark mode changes
watch(
  () => props.store.darkMode,
  (dark) => {
    monaco.editor.setTheme(dark ? "vs-dark" : "vs");
  },
);

onUnmounted(() => {
  editor.value?.dispose();
});
</script>

<template>
  <div class="code-output">
    <div ref="editorContainer" class="editor-container" />
  </div>
</template>

<style scoped>
.code-output {
  height: 100%;
  width: 100%;
  overflow: hidden;
  background: var(--bg-secondary);
}

.editor-container {
  height: 100%;
  width: 100%;
}
</style>
