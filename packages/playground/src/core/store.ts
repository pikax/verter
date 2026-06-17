import { reactive, ref, computed, watch, type Ref } from "vue";
import {
  File,
  type OutputMode,
  type StoreState,
  type CompilerOptions,
  type CompileTiming,
  type TypeCheckerMode,
  type TypeCheckerStatus,
  type TsDiagnosticEntry,
} from "./types";
import { compileFile, relintFile, initCompilers, switchWasmVersion } from "./compiler";
import { getDefaultImportMap, extractVueVersion, type ImportMap } from "./importMap";
import { serializeToHash, deserializeFromHash, type SerializedState } from "./urlState";
import type { VersionEntry } from "./versions";
import * as projectStorage from "./projectStorage";
import {
  type LanguagePin,
  detectFrameworkId,
  frameworkById,
  frameworkCarrierFilename,
  frameworkCarrierExtension,
  isCarrierFilename,
  isExperimentalFramework,
} from "./frameworks";
import { presets } from "./presets";

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

const defaultSvelteAppCode = `<script lang="ts">
  let count = $state(0)
</script>

<button onclick={() => count++}>
  Clicked {count} {count === 1 ? 'time' : 'times'}
</button>
`;

export const IMPORT_MAP_FILENAME = "import-map.json";

/**
 * The default carrier code for a framework (the seed for a fresh main file when
 * a language is selected). Vue keeps its existing default; other frameworks use
 * a minimal carrier. Descriptor-driven via the framework id.
 */
function defaultCarrierCodeFor(frameworkId: string): string {
  if (frameworkId === "svelte") return defaultSvelteAppCode;
  return defaultAppCode;
}

/**
 * The minimal child-carrier seed for a NEW carrier file added via the file
 * tabs (distinct from the full app seed). Vue seeds an empty `<script setup>` +
 * `<template>`; Svelte seeds an empty rune `<script>` + markup. Keyed by the
 * framework id (descriptor-resolved by the caller), never by a literal
 * extension switch. A non-carrier file (`.ts`/`.js`/etc.) seeds empty.
 */
function defaultChildCarrierCodeFor(frameworkId: string | null): string {
  if (frameworkId === "svelte") return `<script lang="ts">\n\n</script>\n\n<div></div>\n`;
  if (frameworkId === "vue") {
    return `<script setup lang="ts">\n\n</script>\n\n<template>\n  <div></div>\n</template>\n`;
  }
  return "";
}

/** The first preset registered for a framework, if any. */
function defaultPresetFor(frameworkId: string): SerializedState | undefined {
  return presets.find((p) => p.language === frameworkId)?.state;
}

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
  toggleStrictSlots(): void;
  setTypeChecker(mode: TypeCheckerMode): void;
  setTypeCheckerStatus(status: TypeCheckerStatus): void;
  disabledRules: Set<string>;
  toggleLintRule(name: string): void;
  relint(): void;
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
  // Click-to-highlight from output panels
  revealSpan: { start: number; end: number } | null;
  requestRevealSpan(start: number, end: number): void;
  // Framework language selection (descriptor-driven)
  languagePin: LanguagePin;
  effectiveLanguage: string;
  isExperimentalLanguage: boolean;
  /**
   * The default carrier extension (incl. leading dot, e.g. `.vue` / `.svelte`)
   * appended to a bare new-file name, derived from the effective framework.
   */
  newFileExtension: string;
  selectFramework(frameworkId: string): Promise<void>;
  unpinLanguage(): void;
}

function normalizeOutputMode(mode: string | undefined): OutputMode {
  switch (mode) {
    case "js":
    case "css":
    case "tsc":
    case "types":
      return "files";
    case "componentMeta":
      return "analysis";
    case "preview":
    case "ssr":
    case "analysis":
    case "lint":
    case "outline":
    case "files":
    case "cssMatch":
    case "map":
    case "diagnostics":
    case "templateAst":
    case "cssVarFlow":
    case "depGraph":
      return mode;
    default:
      return "preview";
  }
}

export function useStore(): Store {
  const files: Ref<Record<string, File>> = ref({});
  const activeFilename = ref("App.vue");
  const mainFile = ref("App.vue");
  // The pinned framework language id, or null for Auto (auto-detect).
  const languagePin = ref<LanguagePin>(null);
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
    strictSlots: false,
  });
  const compileTiming = reactive<CompileTiming>({
    verterNewJs: null,
    parseDurationMs: null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
  });
  const typeChecker = ref<TypeCheckerMode>("tsc");
  const typeCheckerStatus = ref<TypeCheckerStatus>("active");

  const verterVersion = ref("local");
  const versionLoading = ref(false);
  const tsDiagnostics: Ref<TsDiagnosticEntry[]> = ref([]);

  const disabledRules = reactive(new Set<string>());

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

  // The effective framework language: the explicit pin if set, otherwise
  // auto-detected (longest-suffix). Detection prefers the main file when it
  // actually exists, then the active file, then ANY existing carrier/adapter
  // file in the project (so an unpinned restore whose files are .svelte resolves
  // to svelte even if mainFile still names the default App.vue). Defaults to
  // "vue" when nothing resolves.
  const effectiveLanguage = computed<string>(() => {
    if (languagePin.value) return languagePin.value;
    if (files.value[mainFile.value]) {
      const fromMain = detectFrameworkId(mainFile.value);
      if (fromMain) return fromMain;
    }
    if (files.value[activeFilename.value]) {
      const fromActive = detectFrameworkId(activeFilename.value);
      if (fromActive) return fromActive;
    }
    for (const name of Object.keys(files.value)) {
      const id = detectFrameworkId(name);
      if (id) return id;
    }
    // Bare detect of mainFile (handles fresh, files-empty init before seeding).
    return detectFrameworkId(mainFile.value) ?? "vue";
  });

  const isExperimentalLanguage = computed<boolean>(() =>
    isExperimentalFramework(effectiveLanguage.value),
  );

  // The carrier extension appended to a bare new-file name (descriptor-driven):
  // the effective framework's primary carrier extension (`.vue`, `.svelte`, …).
  const newFileExtension = computed<string>(() => {
    const framework = frameworkById(effectiveLanguage.value);
    return framework ? frameworkCarrierExtension(framework) : "";
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
        outputMode.value = normalizeOutputMode(savedState.outputMode);
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
      // Restore the framework pin only if it names a registered framework;
      // an invalid / stale id is ignored (falls back to Auto).
      if (savedState.language && frameworkById(savedState.language)) {
        languagePin.value = savedState.language;
      }
    }

    // Derive the main carrier file from the effective framework so a Svelte pin
    // (or an auto-detected Svelte carrier in the restored files) seeds the right
    // main file and default code.
    reconcileMainFileForLanguage();

    if (!files.value[mainFile.value]) {
      files.value[mainFile.value] = new File(
        mainFile.value,
        defaultCarrierCodeFor(effectiveLanguage.value),
      );
    }

    // Compile all files on init and capture timing from the last one compiled
    let lastTiming: CompileTiming = {
      verterNewJs: null,
      parseDurationMs: null,
      scriptMs: null,
      templateMs: null,
      styleMs: null,
      tsxMs: null,
      tscMs: null,
      lintMs: null,
    };
    for (const file of Object.values(files.value)) {
      lastTiming = await compileFile(file, compilerOptions, disabledRules, files.value);
    }
    Object.assign(compileTiming, lastTiming);
    loading.value = false;

    // Watch for file code changes and auto-compile when autoSave is enabled
    watch(
      () => activeFile.value?.code,
      async () => {
        if (activeFilename.value === IMPORT_MAP_FILENAME) return;
        if (autoSave.value && activeFile.value) {
          const timing = await compileFile(
            activeFile.value,
            compilerOptions,
            undefined,
            files.value,
          );
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
          const timing = await compileFile(file, compilerOptions, disabledRules, files.value);
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
        language: languagePin.value ?? undefined,
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

  /**
   * Point `mainFile` at a carrier file matching the effective framework. If no
   * file with the framework's carrier extension exists, the main file stays as
   * the framework's default carrier name (created with default code by the
   * caller).
   */
  function reconcileMainFileForLanguage() {
    const framework = frameworkById(effectiveLanguage.value);
    if (!framework) return;
    const carrierExts = framework.carrierExtensions;
    // Already pointing at a carrier of the right framework — keep it.
    if (carrierExts.some((ext) => mainFile.value.endsWith(ext))) return;
    // Find an existing carrier file for this framework.
    const existing = Object.keys(files.value).find((name) =>
      carrierExts.some((ext) => name.endsWith(ext)),
    );
    mainFile.value = existing ?? frameworkCarrierFilename(framework);
  }

  async function selectFramework(frameworkId: string) {
    const framework = frameworkById(frameworkId);
    if (!framework) return;
    languagePin.value = frameworkId;
    // Load the framework's default preset when available, else seed a minimal
    // carrier file. Either way the main file swaps to that framework's carrier.
    const preset = defaultPresetFor(frameworkId);
    if (preset) {
      await loadProject("", preset);
      currentProjectName.value = null;
      // loadProject reconciled files; re-pin (loadProject may have set it from state).
      languagePin.value = frameworkId;
    } else {
      const carrier = frameworkCarrierFilename(framework);
      files.value = {
        [carrier]: new File(carrier, defaultCarrierCodeFor(frameworkId)),
      };
      mainFile.value = carrier;
      activeFilename.value = carrier;
      await recompile();
    }
  }

  function unpinLanguage() {
    languagePin.value = null;
  }

  function setActiveFile(filename: string) {
    if (filename === IMPORT_MAP_FILENAME || files.value[filename]) {
      activeFilename.value = filename;
    }
  }

  function addFile(filename: string) {
    if (!files.value[filename]) {
      // Seed the carrier body from the framework that OWNS the filename's
      // extension (descriptor-resolved). A non-carrier file seeds empty.
      const carrierFramework = isCarrierFilename(filename) ? detectFrameworkId(filename) : null;
      const defaultCode = defaultChildCarrierCodeFor(carrierFramework);
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
    outputMode.value = normalizeOutputMode(mode);
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

  function toggleStrictSlots() {
    compilerOptions.strictSlots = !compilerOptions.strictSlots;
    recompile();
  }

  function setTypeChecker(mode: TypeCheckerMode) {
    typeChecker.value = mode;
  }

  function setTypeCheckerStatus(status: TypeCheckerStatus) {
    typeCheckerStatus.value = status;
  }

  function setVueVersion(version: string) {
    const defaults = getDefaultImportMap(version);
    Object.assign(importMap, defaults);
  }

  async function recompile() {
    if (activeFilename.value === IMPORT_MAP_FILENAME) return;
    const file = activeFile.value;
    if (file) {
      const timing = await compileFile(file, compilerOptions, disabledRules, files.value);
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
      language: languagePin.value ?? undefined,
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

    // Restore the framework pin (only if registered), then point the main file
    // at the matching carrier before seeding any default file.
    languagePin.value = state.language && frameworkById(state.language) ? state.language : null;
    reconcileMainFileForLanguage();

    if (!files.value[mainFile.value]) {
      files.value[mainFile.value] = new File(
        mainFile.value,
        defaultCarrierCodeFor(effectiveLanguage.value),
      );
    }

    // Restore state
    if (
      state.activeFile &&
      (files.value[state.activeFile] || state.activeFile === IMPORT_MAP_FILENAME)
    ) {
      activeFilename.value = state.activeFile;
    } else {
      activeFilename.value = mainFile.value;
    }
    outputMode.value = normalizeOutputMode(state.outputMode);
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
    let lastTiming: CompileTiming = {
      verterNewJs: null,
      parseDurationMs: null,
      scriptMs: null,
      templateMs: null,
      styleMs: null,
      tsxMs: null,
      tscMs: null,
      lintMs: null,
    };
    for (const file of Object.values(files.value)) {
      lastTiming = await compileFile(file, compilerOptions, disabledRules, files.value);
    }
    Object.assign(compileTiming, lastTiming);
  }

  function deleteCurrentProject(name: string) {
    projectStorage.deleteProject(name);
    if (currentProjectName.value === name) {
      currentProjectName.value = null;
    }
  }

  function toggleLintRule(name: string) {
    if (disabledRules.has(name)) {
      disabledRules.delete(name);
    } else {
      disabledRules.add(name);
    }
  }

  function relint() {
    if (activeFilename.value === IMPORT_MAP_FILENAME) return;
    const file = activeFile.value;
    if (!file) return;
    const lintMs = relintFile(file, disabledRules);
    if (lintMs != null) {
      compileTiming.lintMs = lintMs;
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

  // Click-to-highlight from output panels
  const revealSpan: Ref<{ start: number; end: number } | null> = ref(null);
  function requestRevealSpan(start: number, end: number) {
    revealSpan.value = { start, end };
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
    toggleStrictSlots,
    typeChecker,
    typeCheckerStatus,
    setTypeChecker,
    setTypeCheckerStatus,
    disabledRules,
    toggleLintRule,
    relint,
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
    revealSpan,
    requestRevealSpan,
    // Framework language selection
    languagePin,
    effectiveLanguage,
    isExperimentalLanguage,
    newFileExtension,
    selectFramework,
    unpinLanguage,
  }) as Store;
}
