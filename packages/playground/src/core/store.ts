import { reactive, ref, computed, watch, type Ref } from "vue";
import {
  File,
  type OutputMode,
  type StoreState,
  type CompilerOptions,
  type CompileTiming,
  type TypeCheckerMode,
  type TsDiagnosticEntry,
} from "./types";
import { compileFile, initCompilers, switchWasmVersion } from "./compiler";
import { getDefaultImportMap, extractVueVersion, type ImportMap } from "./importMap";
import { serializeToHash, deserializeFromHash, type SerializedState } from "./urlState";
import type { VersionEntry } from "./versions";
import * as projectStorage from "./projectStorage";

const defaultAppCode = `<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)
const message = ref('Hello from Verter!')

function increment() {
  count.value++
}
</script>

<template>
  <div class="app">
    <h1>{{ message }}</h1>
    <button @click="increment">Count: {{ count }}</button>
  </div>
</template>

<style scoped>
.app {
  font-family: sans-serif;
  text-align: center;
  padding: 2rem;
}
button {
  padding: 0.5rem 1rem;
  font-size: 1rem;
  cursor: pointer;
}
</style>
`;

export const IMPORT_MAP_FILENAME = "import-map.json";

export interface Store extends StoreState {
  activeFile: File | undefined;
  importMap: ImportMap;
  verterVersion: string;
  versionLoading: boolean;
  tsDiagnostics: TsDiagnosticEntry[];
  init(): Promise<void>;
  setActiveFile(filename: string): void;
  addFile(filename: string): void;
  deleteFile(filename: string): void;
  updateCode(code: string): void;
  updateImportMap(json: string): void;
  setOutputMode(mode: OutputMode): void;
  toggleDarkMode(): void;
  toggleAutoSave(): void;
  toggleProduction(): void;
  toggleSSR(): void;
  setTypeChecker(mode: TypeCheckerMode): void;
  recompile(): Promise<void>;
  switchVerterVersion(entry: VersionEntry): Promise<void>;
  setVueVersion(version: string): void;
  vueVersion: string;
  // Project management
  saveProject(name?: string): void;
  loadProject(name: string, state: SerializedState): Promise<void>;
  deleteProject(name: string): void;
  // Editable output
  toggleEditableOutput(): void;
  updateTsxOverride(code: string): void;
  clearTsxOverride(): void;
}

export function useStore(): Store {
  const files: Ref<Record<string, File>> = ref({});
  const activeFilename = ref("App.vue");
  const mainFile = ref("App.vue");
  const errors: Ref<string[]> = ref([]);
  const outputMode: Ref<OutputMode> = ref("preview");
  const loading = ref(true);
  const darkMode = ref(
    typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const autoSave = ref(true);
  const compilerOptions = reactive<CompilerOptions>({
    isProduction: false,
    ssr: false,
  });
  const compileTiming = reactive<CompileTiming>({
    verterNew: null,
    verterNewJs: null,
    parseDurationMs: null,
  });
  const typeChecker = ref<TypeCheckerMode>("tsc");

  const verterVersion = ref("local");
  const versionLoading = ref(false);
  const tsDiagnostics: Ref<TsDiagnosticEntry[]> = ref([]);

  const currentProjectName = ref<string | null>(null);
  const editableOutput = ref(false);
  const tsxUserEdited = ref(false);
  const tsxOverrideCode = ref<string | null>(null);

  const importMap = reactive(getDefaultImportMap());
  const vueVersion = computed(() => extractVueVersion(importMap) ?? "3.5.26");

  const activeFile = computed(() => {
    if (activeFilename.value === IMPORT_MAP_FILENAME) {
      return new File(IMPORT_MAP_FILENAME, JSON.stringify(importMap, null, 2));
    }
    return files.value[activeFilename.value];
  });

  async function init() {
    loading.value = true;
    await initCompilers();

    // Restore state from URL hash if present
    const savedState = deserializeFromHash();
    if (savedState) {
      for (const [filename, code] of Object.entries(savedState.files)) {
        files.value[filename] = new File(filename, code);
      }
      if (
        savedState.activeFile &&
        (files.value[savedState.activeFile] || savedState.activeFile === IMPORT_MAP_FILENAME)
      ) {
        activeFilename.value = savedState.activeFile;
      }
      if (savedState.outputMode) {
        outputMode.value = savedState.outputMode;
      }
      if (savedState.compilerOptions) {
        Object.assign(compilerOptions, savedState.compilerOptions);
      }
      // Re-initialize import map with correct vue version, then merge custom imports
      if (savedState.vueVersion) {
        const defaults = getDefaultImportMap(savedState.vueVersion);
        Object.assign(importMap, defaults);
      }
      if (savedState.importMap?.imports) {
        Object.assign(importMap.imports, savedState.importMap.imports);
      }
      if (savedState.importMap?.scopes) {
        importMap.scopes = savedState.importMap.scopes;
      }

      // Switch verter version if specified and different from current
      if (savedState.verterVersion && savedState.verterVersion !== verterVersion.value) {
        verterVersion.value = savedState.verterVersion;
        // Version switch will happen after init compilers are ready
      }
      if (savedState.typeChecker) {
        typeChecker.value = savedState.typeChecker;
      }
    }

    if (!files.value[mainFile.value]) {
      files.value[mainFile.value] = new File(mainFile.value, defaultAppCode);
    }

    // Compile all files on init and capture timing from the last one compiled
    let lastTiming: CompileTiming = {
      verterNew: null,
      verterNewJs: null,
      parseDurationMs: null,
    };
    for (const file of Object.values(files.value)) {
      lastTiming = await compileFile(file, compilerOptions);
    }
    Object.assign(compileTiming, lastTiming);
    loading.value = false;

    // Watch for file code changes and auto-compile when autoSave is enabled
    watch(
      () => activeFile.value?.code,
      async () => {
        if (activeFilename.value === IMPORT_MAP_FILENAME) return;
        if (autoSave.value && activeFile.value) {
          const timing = await compileFile(activeFile.value, compilerOptions);
          Object.assign(compileTiming, timing);
          errors.value = activeFile.value.compiled.errors;
          clearTsxOverride();
        }
      },
    );

    // Watch for active file changes - always compile when switching files
    watch(
      () => activeFilename.value,
      async () => {
        if (activeFilename.value === IMPORT_MAP_FILENAME) return;
        const file = activeFile.value;
        if (file) {
          const timing = await compileFile(file, compilerOptions);
          Object.assign(compileTiming, timing);
          errors.value = file.compiled.errors;
          clearTsxOverride();
        }
      },
    );

    // Auto-save state to URL hash (debounced)
    let saveTimeout: ReturnType<typeof setTimeout> | null = null;
    watch(
      () => ({
        files: Object.fromEntries(Object.entries(files.value).map(([k, f]) => [k, f.code])),
        activeFile: activeFilename.value,
        outputMode: outputMode.value,
        compilerOptions: { ...compilerOptions },
        importMap: {
          imports: { ...importMap.imports },
          scopes: importMap.scopes ? { ...importMap.scopes } : undefined,
        },
        vueVersion: extractVueVersion(importMap),
        verterVersion: verterVersion.value,
        typeChecker: typeChecker.value,
      }),
      (state) => {
        if (saveTimeout) clearTimeout(saveTimeout);
        saveTimeout = setTimeout(() => {
          serializeToHash(state);
          if (currentProjectName.value) {
            projectStorage.saveProject(currentProjectName.value, state);
          }
        }, 500);
      },
      { deep: true },
    );
  }

  function setActiveFile(filename: string) {
    if (filename === IMPORT_MAP_FILENAME || files.value[filename]) {
      activeFilename.value = filename;
    }
  }

  function addFile(filename: string) {
    if (!files.value[filename]) {
      const ext = filename.split(".").pop();
      let defaultCode = "";
      if (ext === "vue") {
        defaultCode = `<script setup lang="ts">\n\n</script>\n\n<template>\n  <div></div>\n</template>\n`;
      }
      files.value[filename] = new File(filename, defaultCode);
      activeFilename.value = filename;
    }
  }

  function deleteFile(filename: string) {
    if (filename === mainFile.value || filename === IMPORT_MAP_FILENAME) return;
    if (files.value[filename]) {
      delete files.value[filename];
      if (activeFilename.value === filename) {
        activeFilename.value = mainFile.value;
      }
    }
  }

  function updateCode(code: string) {
    const file = activeFile.value;
    if (file && activeFilename.value !== IMPORT_MAP_FILENAME) {
      file.code = code;
    }
  }

  function updateImportMap(json: string) {
    try {
      const parsed = JSON.parse(json);
      if (parsed && typeof parsed.imports === "object") {
        importMap.imports = parsed.imports;
        importMap.scopes = parsed.scopes;
        errors.value = [];
      }
    } catch {
      // Don't update import map on invalid JSON - user is still typing
    }
  }

  function setOutputMode(mode: OutputMode) {
    outputMode.value = mode;
  }

  function toggleDarkMode() {
    darkMode.value = !darkMode.value;
    document.documentElement.classList.toggle("dark", darkMode.value);
  }

  function toggleAutoSave() {
    autoSave.value = !autoSave.value;
  }

  function toggleProduction() {
    compilerOptions.isProduction = !compilerOptions.isProduction;
    recompile();
  }

  function toggleSSR() {
    compilerOptions.ssr = !compilerOptions.ssr;
    recompile();
  }

  function setTypeChecker(mode: TypeCheckerMode) {
    typeChecker.value = mode;
  }

  function setVueVersion(version: string) {
    const defaults = getDefaultImportMap(version);
    Object.assign(importMap, defaults);
  }

  async function recompile() {
    if (activeFilename.value === IMPORT_MAP_FILENAME) return;
    const file = activeFile.value;
    if (file) {
      const timing = await compileFile(file, compilerOptions);
      Object.assign(compileTiming, timing);
      errors.value = file.compiled.errors;
    }
  }

  function getCurrentState(): SerializedState {
    return {
      files: Object.fromEntries(Object.entries(files.value).map(([k, f]) => [k, f.code])),
      activeFile: activeFilename.value,
      outputMode: outputMode.value,
      compilerOptions: { ...compilerOptions },
      importMap: {
        imports: { ...importMap.imports },
        scopes: importMap.scopes ? { ...importMap.scopes } : undefined,
      },
      vueVersion: extractVueVersion(importMap) ?? undefined,
      verterVersion: verterVersion.value,
      typeChecker: typeChecker.value,
    };
  }

  function saveCurrentProject(name?: string) {
    const projectName = name ?? currentProjectName.value;
    if (!projectName) return;
    currentProjectName.value = projectName;
    projectStorage.saveProject(projectName, getCurrentState());
  }

  async function loadProject(name: string, state: SerializedState) {
    // Reset files
    files.value = {};
    for (const [filename, code] of Object.entries(state.files)) {
      files.value[filename] = new File(filename, code);
    }
    if (!files.value[mainFile.value]) {
      files.value[mainFile.value] = new File(mainFile.value, defaultAppCode);
    }

    // Restore state
    if (state.activeFile && (files.value[state.activeFile] || state.activeFile === IMPORT_MAP_FILENAME)) {
      activeFilename.value = state.activeFile;
    } else {
      activeFilename.value = mainFile.value;
    }
    if (state.outputMode) outputMode.value = state.outputMode;
    if (state.compilerOptions) Object.assign(compilerOptions, state.compilerOptions);
    if (state.vueVersion) {
      const defaults = getDefaultImportMap(state.vueVersion);
      Object.assign(importMap, defaults);
    }
    if (state.importMap?.imports) Object.assign(importMap.imports, state.importMap.imports);
    if (state.importMap?.scopes) importMap.scopes = state.importMap.scopes;
    if (state.typeChecker) typeChecker.value = state.typeChecker;

    currentProjectName.value = name;
    clearTsxOverride();

    // Recompile all files
    let lastTiming: CompileTiming = { verterNew: null, verterNewJs: null, parseDurationMs: null };
    for (const file of Object.values(files.value)) {
      lastTiming = await compileFile(file, compilerOptions);
    }
    Object.assign(compileTiming, lastTiming);
  }

  function deleteCurrentProject(name: string) {
    projectStorage.deleteProject(name);
    if (currentProjectName.value === name) {
      currentProjectName.value = null;
    }
  }

  function toggleEditableOutput() {
    editableOutput.value = !editableOutput.value;
    if (!editableOutput.value) {
      clearTsxOverride();
    }
  }

  function updateTsxOverride(code: string) {
    tsxOverrideCode.value = code;
    tsxUserEdited.value = true;
  }

  function clearTsxOverride() {
    tsxOverrideCode.value = null;
    tsxUserEdited.value = false;
  }

  async function switchVersion(entry: VersionEntry) {
    versionLoading.value = true;
    try {
      await switchWasmVersion(entry);
      verterVersion.value = entry.id;
      await recompile();
    } catch (e) {
      errors.value = [e instanceof Error ? e.message : String(e)];
    } finally {
      versionLoading.value = false;
    }
  }

  return reactive({
    files,
    activeFilename,
    mainFile,
    errors,
    outputMode,
    loading,
    darkMode,
    autoSave,
    compilerOptions,
    compileTiming,
    activeFile,
    importMap,
    verterVersion,
    versionLoading,
    init,
    setActiveFile,
    addFile,
    deleteFile,
    updateCode,
    updateImportMap,
    setOutputMode,
    toggleDarkMode,
    toggleAutoSave,
    toggleProduction,
    toggleSSR,
    typeChecker,
    setTypeChecker,
    recompile,
    tsDiagnostics,
    switchVerterVersion: switchVersion,
    setVueVersion,
    vueVersion,
    // Project management
    currentProjectName,
    saveProject: saveCurrentProject,
    loadProject,
    deleteProject: deleteCurrentProject,
    // Editable output
    editableOutput,
    tsxUserEdited,
    tsxOverrideCode,
    toggleEditableOutput,
    updateTsxOverride,
    clearTsxOverride,
  }) as Store;
}
