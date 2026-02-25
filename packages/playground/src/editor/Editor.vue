<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, shallowRef } from "vue";
import * as monaco from "monaco-editor-core";
import { IMPORT_MAP_FILENAME, type Store } from "../core/store";
import type { HostDiagnostic, LintDiagnostic } from "../core/types";
import { registerLspProviders } from "./lspProviders";
import { computeAutoCloseTagText } from "./templateIde";
import { TypeScriptService, type MappedDiagnostic } from "./tsService";
import { getTypeDiagnosticsSourceMap } from "./diagnosticSourceMap";

const props = defineProps<{
  store: Store;
}>();

const editorContainer = ref<HTMLElement>();
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>();
const pendingCode = ref<string | null>(null);
let lspDisposables: monaco.IDisposable[] = [];
const tsService = new TypeScriptService();
let tsDiagnostics: MappedDiagnostic[] = [];

function getLanguage(filename: string): string {
  if (filename.endsWith(".vue")) return "vue";
  if (filename.endsWith(".ts")) return "typescript";
  if (filename.endsWith(".js")) return "javascript";
  if (filename.endsWith(".css")) return "css";
  if (filename.endsWith(".json")) return "json";
  return "plaintext";
}

function saveAndCompile() {
  const value = editor.value?.getValue();
  if (value !== undefined) {
    props.store.updateCode(value);
    props.store.recompile();
    pendingCode.value = null;
  }
}

function lintSeverityToMarkerSeverity(
  severity: LintDiagnostic["severity"],
): monaco.MarkerSeverity {
  switch (severity) {
    case "error":
      return monaco.MarkerSeverity.Error;
    case "warning":
      return monaco.MarkerSeverity.Warning;
    case "info":
      return monaco.MarkerSeverity.Hint;
  }
}

function hostSeverityToMarkerSeverity(
  severity: HostDiagnostic["severity"],
): monaco.MarkerSeverity {
  switch (severity) {
    case "error":
      return monaco.MarkerSeverity.Error;
    case "warning":
      return monaco.MarkerSeverity.Warning;
    case "info":
      return monaco.MarkerSeverity.Info;
  }
}

function tsSeverityToMarkerSeverity(
  severity: MappedDiagnostic["severity"],
): monaco.MarkerSeverity {
  switch (severity) {
    case "error":
      return monaco.MarkerSeverity.Error;
    case "warning":
      return monaco.MarkerSeverity.Warning;
    case "info":
      return monaco.MarkerSeverity.Info;
  }
}

function collectStyleVBindIdentifiers(): Set<string> {
  const file = props.store.activeFile;
  const styles = file?.compiled.analysis?.styles ?? [];
  const names = new Set<string>();

  for (const style of styles) {
    for (const vBind of style.vBinds) {
      const matches = vBind.expression.match(/[A-Za-z_$][\w$]*/g) ?? [];
      for (const name of matches) {
        names.add(name);
      }
    }
  }

  return names;
}

function extractUnusedBindingName(diagnostic: MappedDiagnostic): string | null {
  // TS6133: "'x' is declared but its value is never read."
  // TS6196: "'x' is declared but never used."
  if (diagnostic.code !== 6133 && diagnostic.code !== 6196) return null;
  const match = diagnostic.message.match(/'([^']+)'/);
  return match?.[1] ?? null;
}

function updateMarkers() {
  const model = editor.value?.getModel();
  if (!model) return;

  const file = props.store.activeFile;
  if (!file) {
    monaco.editor.setModelMarkers(model, "verter", []);
    monaco.editor.setModelMarkers(model, "typescript", []);
    return;
  }

  // Verter markers (lint + compiler)
  const verterMarkers: monaco.editor.IMarkerData[] = [];

  for (const d of file.compiled.lintDiagnostics) {
    const startPos = model.getPositionAt(d.spanStart);
    const endPos = model.getPositionAt(d.spanEnd);
    verterMarkers.push({
      severity: lintSeverityToMarkerSeverity(d.severity),
      message: `[${d.rule}] ${d.message}`,
      startLineNumber: startPos.lineNumber,
      startColumn: startPos.column,
      endLineNumber: endPos.lineNumber,
      endColumn: endPos.column,
      source: "verter-lint",
    });
  }

  for (const d of file.compiled.compilerDiagnostics) {
    if (d.spanStart == null || d.spanEnd == null) continue;
    const startPos = model.getPositionAt(d.spanStart);
    const endPos = model.getPositionAt(d.spanEnd);
    verterMarkers.push({
      severity: hostSeverityToMarkerSeverity(d.severity),
      message: d.message,
      startLineNumber: startPos.lineNumber,
      startColumn: startPos.column,
      endLineNumber: endPos.lineNumber,
      endColumn: endPos.column,
      source: "verter",
    });
  }

  monaco.editor.setModelMarkers(model, "verter", verterMarkers);

  // TypeScript markers
  const tsMarkers: monaco.editor.IMarkerData[] = [];
  for (const d of tsDiagnostics) {
    const startPos = model.getPositionAt(d.start);
    const endPos = model.getPositionAt(d.end);
    tsMarkers.push({
      severity: tsSeverityToMarkerSeverity(d.severity),
      message: `TS${d.code}: ${d.message}`,
      startLineNumber: startPos.lineNumber,
      startColumn: startPos.column,
      endLineNumber: endPos.lineNumber,
      endColumn: endPos.column,
      source: "typescript",
    });
  }
  monaco.editor.setModelMarkers(model, "typescript", tsMarkers);
}

async function syncTypeScript() {
  const file = props.store.activeFile;
  if (!file) return;

  const tsxCode = file.compiled.types;
  if (!tsxCode) {
    tsDiagnostics = [];
    updateMarkers();
    return;
  }

  try {
    const diagnosticsSourceMap = getTypeDiagnosticsSourceMap(file.compiled);
    const diagnostics = await tsService.syncTsx(
      file.filename,
      tsxCode,
      file.code,
      diagnosticsSourceMap,
    );
    const styleVBindIdentifiers = collectStyleVBindIdentifiers();
    tsDiagnostics = diagnostics.filter((diag) => {
      const unusedName = extractUnusedBindingName(diag);
      if (!unusedName) return true;
      return !styleVBindIdentifiers.has(unusedName);
    });
  } catch {
    tsDiagnostics = [];
  }
  updateMarkers();
}

onMounted(() => {
  if (!editorContainer.value) return;

  editor.value = monaco.editor.create(editorContainer.value, {
    value: props.store.activeFile?.code ?? "",
    language: getLanguage(props.store.activeFilename),
    theme: props.store.darkMode ? "vs-dark" : "vs",
    minimap: { enabled: false },
    fontSize: 14,
    lineNumbers: "on",
    renderLineHighlight: "line",
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    wordWrap: "on",
  });

  // Initialize TypeScript service in background (non-blocking)
  tsService.init().then(() => {
    // Re-sync on init if we already have compiled output
    syncTypeScript();
  });

  // Register LSP providers with TS bridge
  lspDisposables = registerLspProviders(props.store, tsService);

  // Add Ctrl+S / Cmd+S keybinding
  editor.value.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
    saveAndCompile();
  });

  // Auto-close HTML/Vue tags inside <template> blocks when typing `>`.
  editor.value.onDidType((text) => {
    if (text !== ">") return;
    if (!props.store.activeFilename.endsWith(".vue")) return;

    const monacoEditor = editor.value;
    if (!monacoEditor) return;

    const model = monacoEditor.getModel();
    const position = monacoEditor.getPosition();
    if (!model || !position) return;

    const offset = model.getOffsetAt(position);
    const closeTagText = computeAutoCloseTagText(model.getValue(), offset);
    if (!closeTagText) return;

    monacoEditor.executeEdits("template-auto-close", [
      {
        range: new monaco.Range(
          position.lineNumber,
          position.column,
          position.lineNumber,
          position.column,
        ),
        text: closeTagText,
      },
    ]);
    monacoEditor.setPosition(position);
  });

  editor.value.onDidChangeModelContent(() => {
    const value = editor.value?.getValue();
    if (value !== undefined) {
      if (props.store.activeFilename === IMPORT_MAP_FILENAME) {
        props.store.updateImportMap(value);
      } else if (props.store.autoSave) {
        props.store.updateCode(value);
      } else {
        // Store pending changes but don't compile
        pendingCode.value = value;
        // Still update the file code for display, but compilation won't auto-trigger
        props.store.updateCode(value);
      }
    }
  });

  // Watch diagnostics and update markers (Verter lint + compiler)
  watch(
    () => [
      props.store.activeFile?.compiled.lintDiagnostics,
      props.store.activeFile?.compiled.compilerDiagnostics,
    ],
    () => updateMarkers(),
    { deep: true },
  );

  // Watch TSX output changes to trigger TS re-sync
  watch(
    () => [props.store.activeFile?.compiled.types, props.store.activeFile?.compiled.typesSourceMap],
    () => syncTypeScript(),
  );

  watch(
    () => props.store.activeFilename,
    (filename) => {
      const file = props.store.activeFile;
      if (file && editor.value) {
        const model = monaco.editor.createModel(file.code, getLanguage(filename));
        editor.value.setModel(model);
        pendingCode.value = null;
        tsDiagnostics = [];
        updateMarkers();
        syncTypeScript();
      }
    },
  );

  watch(
    () => props.store.darkMode,
    (dark) => {
      monaco.editor.setTheme(dark ? "vs-dark" : "vs");
    },
  );

  // Initial markers
  updateMarkers();
});

onUnmounted(() => {
  lspDisposables.forEach((d) => d.dispose());
  lspDisposables = [];
  tsService.dispose();
  editor.value?.dispose();
});
</script>

<template>
  <div class="editor-wrapper">
    <div ref="editorContainer" class="editor-container" />
  </div>
</template>

<style scoped>
.editor-wrapper {
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
}

.editor-container {
  flex: 1;
  min-height: 0;
}
</style>
