<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, shallowRef } from "vue";
import * as monaco from "monaco-editor-core";
import { IMPORT_MAP_FILENAME, type Store } from "../core/store";
import { extractVueVersion } from "../core/importMap";
import type { HostDiagnostic, LintDiagnostic } from "../core/types";
import { registerLspProviders } from "./lspProviders";
import { computeAutoCloseTagText } from "./templateIde";
import { TypeScriptService, type MappedDiagnostic } from "./tsService";
import { TsgoService } from "./tsgoService";
import { getTypeDiagnosticsSourceMap } from "./diagnosticSourceMap";
import {
  computeBindingDecorations,
  computeCssClassDecorations,
  getDecorationStyles,
} from "./decorations";
import type { TypeScriptServiceBridge } from "./lspProviders";
import { prefixWith } from "@verter/types/string";

const typeHelpersSource = prefixWith("");

const props = defineProps<{
  store: Store;
}>();

const editorContainer = ref<HTMLElement>();
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>();
const pendingCode = ref<string | null>(null);
let suppressExternalSync = false;
let lspDisposables: monaco.IDisposable[] = [];
let tsService: TypeScriptServiceBridge & { init(): Promise<void>; dispose(): void; syncTsx: TypeScriptService["syncTsx"] } = new TypeScriptService();
let tsgoService: TsgoService | null = null;
let tsDiagnostics: MappedDiagnostic[] = [];
let decorationIds: string[] = [];
let decorationStyleEl: HTMLStyleElement | null = null;

// Debounce + cancel-on-new-edit for TypeScript sync
let tsSyncVersion = 0;
let tsSyncTimer: ReturnType<typeof setTimeout> | null = null;
const TS_SYNC_DEBOUNCE_MS = 300;

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
    // TS6133/6196 = unused variable/parameter → show as hint with dimmed text
    const isUnused = d.code === 6133 || d.code === 6196;
    tsMarkers.push({
      severity: isUnused ? monaco.MarkerSeverity.Hint : tsSeverityToMarkerSeverity(d.severity),
      message: `TS${d.code}: ${d.message}`,
      startLineNumber: startPos.lineNumber,
      startColumn: startPos.column,
      endLineNumber: endPos.lineNumber,
      endColumn: endPos.column,
      source: "typescript",
      tags: isUnused ? [1] : undefined, // MarkerTag.Unnecessary = 1 → dims text
    });
  }
  monaco.editor.setModelMarkers(model, "typescript", tsMarkers);
}

/** Debounced TypeScript sync with cancel-on-new-edit. */
function scheduleTsSync() {
  if (tsSyncTimer) clearTimeout(tsSyncTimer);
  tsSyncTimer = setTimeout(() => {
    tsSyncTimer = null;
    syncTypeScript();
  }, TS_SYNC_DEBOUNCE_MS);
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

  // Sync ALL files' .d.ts to the worker for cross-file import resolution
  const dtsFiles: Array<{ filename: string; dtsCode: string }> = [];
  for (const [filename, f] of Object.entries(props.store.files)) {
    if (f.compiled.tscCode) {
      dtsFiles.push({ filename, dtsCode: f.compiled.tscCode });
    }
  }
  if (dtsFiles.length > 0 && tsService instanceof TypeScriptService) {
    await tsService.syncDtsFiles(dtsFiles);
  }

  // Capture version to detect stale results
  const version = ++tsSyncVersion;

  try {
    const diagnosticsSourceMap = getTypeDiagnosticsSourceMap(file.compiled);
    const diagnostics = await tsService.syncTsx(
      file.filename,
      tsxCode,
      file.code,
      diagnosticsSourceMap,
      file.compiled.destructuredBlock,
    );

    // Discard results if a newer sync was requested while we were waiting
    if (version !== tsSyncVersion) return;

    const styleVBindIdentifiers = collectStyleVBindIdentifiers();
    tsDiagnostics = diagnostics.filter((diag) => {
      const unusedName = extractUnusedBindingName(diag);
      if (!unusedName) return true;
      // Suppress unused warnings for compiler-generated variables and style v-bind identifiers
      if (unusedName.startsWith("___VERTER___")) return false;
      return !styleVBindIdentifiers.has(unusedName);
    });
  } catch {
    if (version !== tsSyncVersion) return;
    tsDiagnostics = [];
  }
  // Expose TS diagnostics to store for the Diagnostics panel
  props.store.tsDiagnostics = tsDiagnostics.map((d) => ({
    message: d.message,
    start: d.start,
    end: d.end,
    severity: d.severity,
    code: d.code,
  }));
  updateMarkers();
}

function updateDecorations() {
  const monacoEditor = editor.value;
  const model = monacoEditor?.getModel();
  if (!monacoEditor || !model) return;

  const file = props.store.activeFile;
  const analysis = file?.compiled.analysis;
  if (!analysis || !file) {
    decorationIds = monacoEditor.deltaDecorations(decorationIds, []);
    return;
  }

  const newDecorations: monaco.editor.IModelDeltaDecoration[] = [];

  // Binding reactivity decorations
  const bindingDecs = computeBindingDecorations(file.code, analysis);
  for (const dec of bindingDecs) {
    const startPos = model.getPositionAt(dec.start);
    const endPos = model.getPositionAt(dec.end);
    newDecorations.push({
      range: new monaco.Range(
        startPos.lineNumber,
        startPos.column,
        endPos.lineNumber,
        endPos.column,
      ),
      options: {
        inlineClassName: dec.className,
        hoverMessage: { value: dec.hoverMessage },
      },
    });
  }

  // CSS class usage decorations
  const cssDecs = computeCssClassDecorations(analysis);
  for (const dec of cssDecs) {
    const startPos = model.getPositionAt(dec.start);
    const endPos = model.getPositionAt(dec.end);
    newDecorations.push({
      range: new monaco.Range(
        startPos.lineNumber,
        startPos.column,
        endPos.lineNumber,
        endPos.column,
      ),
      options: {
        inlineClassName: dec.className,
      },
    });
  }

  decorationIds = monacoEditor.deltaDecorations(decorationIds, newDecorations);
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
  const vueVersion = extractVueVersion(props.store.importMap) ?? "3.5";
  tsService.init({ vueVersion, verterTypesContent: typeHelpersSource }).then(() => {
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
      suppressExternalSync = true;
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
      suppressExternalSync = false;
    }
  });

  // Inject decoration styles
  decorationStyleEl = document.createElement("style");
  decorationStyleEl.textContent = getDecorationStyles();
  document.head.appendChild(decorationStyleEl);

  // Sync external code changes (e.g. applyFix from LintPanel) back to the editor
  watch(
    () => props.store.activeFile?.code,
    (newCode) => {
      if (suppressExternalSync || !editor.value || newCode === undefined) return;
      const model = editor.value.getModel();
      if (model && model.getValue() !== newCode) {
        model.setValue(newCode);
      }
    },
  );

  // Watch diagnostics and update markers (Verter lint + compiler)
  watch(
    () => [
      props.store.activeFile?.compiled.lintDiagnostics,
      props.store.activeFile?.compiled.compilerDiagnostics,
    ],
    () => updateMarkers(),
    { deep: true },
  );

  // Watch analysis changes to update inline decorations
  watch(
    () => props.store.activeFile?.compiled.analysis,
    () => updateDecorations(),
    { deep: true },
  );

  // Watch TSX output changes to trigger debounced TS re-sync
  watch(
    () => [props.store.activeFile?.compiled.types, props.store.activeFile?.compiled.typesSourceMap],
    () => scheduleTsSync(),
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

  // Watch file list changes to clean up removed files from TS worker
  let prevFileKeys = new Set(Object.keys(props.store.files));
  watch(
    () => Object.keys(props.store.files),
    (newKeys) => {
      const newKeySet = new Set(newKeys);
      if (tsService instanceof TypeScriptService) {
        for (const key of prevFileKeys) {
          if (!newKeySet.has(key)) {
            tsService.closeFile(key);
          }
        }
      }
      prevFileKeys = newKeySet;
      // Re-sync so newly added files are available for cross-file resolution
      scheduleTsSync();
    },
  );

  watch(
    () => props.store.darkMode,
    (dark) => {
      monaco.editor.setTheme(dark ? "vs-dark" : "vs");
    },
  );

  // Watch type checker toggle to switch between tsc and tsgo
  watch(
    () => props.store.typeChecker,
    async (mode) => {
      console.log(`[type-checker] Switching to ${mode}`);
      props.store.setTypeCheckerStatus("initializing");
      // Dispose current providers
      lspDisposables.forEach((d) => d.dispose());
      lspDisposables = [];

      if (mode === "tsgo") {
        tsService.dispose();
        if (!tsgoService) {
          tsgoService = new TsgoService();
        }
        tsService = tsgoService as unknown as typeof tsService;
        await tsgoService.init();
        if (tsgoService.isAvailable) {
          console.log("[type-checker] tsgo initialized successfully");
          props.store.setTypeCheckerStatus("active");
        } else {
          console.warn("[type-checker] tsgo unavailable (SharedArrayBuffer missing or init failed)");
          props.store.setTypeCheckerStatus("unavailable");
        }
      } else {
        if (tsgoService) {
          tsgoService.dispose();
          tsgoService = null;
        }
        const newTsService = new TypeScriptService();
        tsService = newTsService;
        const curVueVersion = extractVueVersion(props.store.importMap) ?? "3.5";
        await newTsService.init({ vueVersion: curVueVersion, verterTypesContent: typeHelpersSource });
        console.log("[type-checker] tsc initialized successfully");
        props.store.setTypeCheckerStatus("active");
      }

      // Re-register LSP providers with new service
      lspDisposables = registerLspProviders(props.store, tsService);

      // Re-sync current file
      tsDiagnostics = [];
      updateMarkers();
      syncTypeScript();
    },
  );

  // Watch Vue version changes in import map to reload types
  watch(
    () => extractVueVersion(props.store.importMap),
    async (newVersion) => {
      if (!newVersion) return;
      if (tsService instanceof TypeScriptService) {
        await tsService.updateVueTypes(newVersion);
        syncTypeScript();
      }
    },
  );

  // Initial markers
  updateMarkers();
});

onUnmounted(() => {
  lspDisposables.forEach((d) => d.dispose());
  lspDisposables = [];
  tsService.dispose();
  tsgoService?.dispose();
  editor.value?.dispose();
  if (decorationStyleEl) {
    decorationStyleEl.remove();
    decorationStyleEl = null;
  }
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
