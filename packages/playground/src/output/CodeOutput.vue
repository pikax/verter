<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, shallowRef } from "vue";
import * as monaco from "monaco-editor-core";
import type { Store } from "../core/store";
import type { OutputMode } from "../core/types";
import { TypeScriptService, type RawTsDiagnostic } from "../editor/tsService";

const props = defineProps<{
  store: Store;
  mode: OutputMode;
  editable?: boolean;
}>();

const editorContainer = ref<HTMLElement>();
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>();

// Lazy TS service for editable types/tsc modes
let tsServiceInstance: TypeScriptService | null = null;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

const code = computed(() => {
  const file = props.store.activeFile;
  if (!file) return "";

  // When user has edited TSX in types mode, show the override
  if (props.mode === "types" && props.store.tsxOverrideCode !== null) {
    return props.store.tsxOverrideCode;
  }

  switch (props.mode) {
    case "js":
      return file.compiled.js;
    case "ssr":
      return file.compiled.ssrCode;
    case "css":
      return file.compiled.css;
    case "types":
      return file.compiled.types;
    case "tsc":
      return file.compiled.tscCode;
    default:
      return "";
  }
});

const language = computed(() => {
  switch (props.mode) {
    case "js":
    case "ssr":
      return "javascript";
    case "css":
      return "css";
    case "types":
    case "tsc":
      return "typescript";
    default:
      return "plaintext";
  }
});

const isEditable = computed(() => {
  return !!props.editable && (props.mode === "types" || props.mode === "tsc");
});

onMounted(() => {
  if (!editorContainer.value) return;

  editor.value = monaco.editor.create(editorContainer.value, {
    value: code.value || "// No output",
    language: language.value,
    theme: props.store.darkMode ? "vs-dark" : "vs",
    readOnly: !isEditable.value,
    minimap: { enabled: false },
    fontSize: 13,
    lineNumbers: "on",
    renderLineHighlight: isEditable.value ? "line" : "none",
    scrollBeyondLastLine: false,
    automaticLayout: true,
    folding: true,
    wordWrap: "on",
    domReadOnly: !isEditable.value,
    contextmenu: isEditable.value,
  });

  if (isEditable.value) {
    editor.value.onDidChangeModelContent(() => {
      const currentCode = editor.value!.getValue();

      if (props.mode === "types") {
        props.store.updateTsxOverride(currentCode);
      }

      // Debounce TS diagnostics
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => runDiagnostics(currentCode), 300);
    });
  }
});

async function ensureTsService(): Promise<TypeScriptService> {
  if (!tsServiceInstance) {
    tsServiceInstance = new TypeScriptService();
    await tsServiceInstance.init();
  }
  return tsServiceInstance;
}

async function runDiagnostics(tsxCode: string) {
  if (!editor.value) return;

  try {
    const svc = await ensureTsService();
    // Standalone scratch check: raw TSX-space diagnostics BY DESIGN (this
    // panel edits generated TSX directly — the only unmapped path).
    const diagnostics = await svc.checkStandalone(tsxCode);
    applyMarkers(diagnostics);
  } catch {
    // Silently ignore worker errors
  }
}

function applyMarkers(diagnostics: RawTsDiagnostic[]) {
  if (!editor.value) return;
  const model = editor.value.getModel();
  if (!model) return;

  const markers: monaco.editor.IMarkerData[] = diagnostics.map((d) => {
    const startPos = model.getPositionAt(d.start);
    const endPos = model.getPositionAt(d.end);
    return {
      severity:
        d.severity === "error"
          ? monaco.MarkerSeverity.Error
          : d.severity === "warning"
            ? monaco.MarkerSeverity.Warning
            : monaco.MarkerSeverity.Info,
      message: `TS${d.code}: ${d.message}`,
      startLineNumber: startPos.lineNumber,
      startColumn: startPos.column,
      endLineNumber: endPos.lineNumber,
      endColumn: endPos.column,
    };
  });

  monaco.editor.setModelMarkers(model, "ts-direct", markers);
}

// Watch for compiled code changes (recompilation resets the editor)
watch(code, (newCode) => {
  if (!editor.value) return;

  // When editable and user has edited, skip overwriting with compiled output
  // (the store.tsxOverrideCode drives the computed, so this won't fire on user edits
  //  — it only fires when the compiled output changes after a recompile)
  if (isEditable.value && props.store.tsxUserEdited) {
    // Recompile happened — clear override and reset
    // clearTsxOverride is called by the store watcher already
  }

  editor.value.setValue(newCode || "// No output");

  // Clear markers on recompile
  const model = editor.value.getModel();
  if (model) {
    monaco.editor.setModelMarkers(model, "ts-direct", []);
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

// Watch for editable state changes
watch(isEditable, (nowEditable) => {
  if (!editor.value) return;
  editor.value.updateOptions({
    readOnly: !nowEditable,
    domReadOnly: !nowEditable,
    renderLineHighlight: nowEditable ? "line" : "none",
    contextmenu: nowEditable,
  });

  if (!nowEditable) {
    // Clear markers when switching to read-only
    const model = editor.value.getModel();
    if (model) {
      monaco.editor.setModelMarkers(model, "ts-direct", []);
    }
  }
});

onUnmounted(() => {
  if (debounceTimer) clearTimeout(debounceTimer);
  tsServiceInstance?.dispose();
  tsServiceInstance = null;
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
