import type { File, CompilerOptions, CompileTiming, FileAnalysis } from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";

interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  ssr?: boolean;
  hmrStrategy?: "none" | "vite" | "webpack";
  forceJs?: boolean;
  sourceMap?: boolean;
  enableTypes?: boolean;
}

interface HostVirtualNodeKind {
  kind: "main" | "script" | "template" | "style" | "custom" | "tsx";
  index?: number;
}

interface HostDiagnostic {
  severity: "error" | "warning" | "info";
  code: string;
  message: string;
  spanStart?: number;
  spanEnd?: number;
}

interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[];
  hasErrors: boolean;
}

interface HostUpdateResult {
  diagnostics: HostDiagnosticsSnapshot;
  parseDurationMs?: number;
}

interface HostVirtualFileResponse {
  code: string;
  sourceMap?: string;
  diagnostics: HostDiagnosticsSnapshot;
}

interface HostBinding {
  upsert(request: {
    inputId: string;
    source: string;
    fileKind: "vue";
    aliases?: string[];
    compileProfile?: HostCompileProfile;
  }): HostUpdateResult;
  getVirtualFile(query: {
    rawId?: string;
    canonicalId?: string;
    nodeKind?: HostVirtualNodeKind;
    compileProfile?: HostCompileProfile;
  }): HostVirtualFileResponse;
  listVirtualFiles(canonicalId: string): HostVirtualNodeKind[];
  getAnalysis?(canonicalOrAlias: string): FileAnalysis | null;
}

/** Convert structured host diagnostics to display strings. */
export function formatDiagnostics(diagnostics: HostDiagnostic[] | undefined): string[] {
  if (!diagnostics || diagnostics.length === 0) return [];
  return diagnostics.map((d) => {
    const loc = d.spanStart != null ? ` (${d.spanStart}:${d.spanEnd ?? d.spanStart})` : "";
    return `[${d.severity}] ${d.message}${loc}`;
  });
}

let wasmHost: HostBinding | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

function toHostProfile(file: File, options?: CompilerOptions): HostCompileProfile {
  return {
    filename: file.filename,
    isProduction: options?.isProduction ?? false,
    ssr: options?.ssr ?? false,
    hmrStrategy: "none",
    forceJs: true,
    sourceMap: true,
    enableTypes: true,
  };
}

function configureHost(wasmModule: WasmModule): void {
  const hostCtor = wasmModule.VerterHost;
  if (!hostCtor) {
    wasmHost = null;
    return;
  }
  try {
    wasmHost = new hostCtor({
      devMode: true,
      compileErrorPolicy: "devServeLastKnownGood",
      maxProfilesPerFile: 8,
    }) as HostBinding;
  } catch {
    wasmHost = null;
  }
}

function collectUniqueHostDiagnostics(
  snapshots: Array<HostDiagnosticsSnapshot | undefined>,
): HostDiagnostic[] {
  const seen = new Set<string>();
  const diagnostics: HostDiagnostic[] = [];
  for (const snapshot of snapshots) {
    const list = snapshot?.diagnostics ?? [];
    for (const d of list) {
      const key = `${d.severity}|${d.code}|${d.message}|${d.spanStart ?? -1}|${d.spanEnd ?? -1}`;
      if (!seen.has(key)) {
        seen.add(key);
        diagnostics.push(d);
      }
    }
  }
  return diagnostics;
}

export async function initCompilers(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    const wasmModule = await loadLocalWasm();
    configureHost(wasmModule);
    initialized = true;
  })();

  return initPromise;
}

/**
 * Switch the WASM compiler to a different version.
 * Loads the appropriate WASM module based on the version entry type.
 */
export async function switchWasmVersion(entry: VersionEntry): Promise<void> {
  let wasmModule: WasmModule;

  if (entry.type === "local") {
    wasmModule = await loadLocalWasm();
  } else if (entry.type === "commit" && entry.sha) {
    wasmModule = await loadCommitWasm(entry.sha);
  } else if (entry.type === "release" && entry.version) {
    wasmModule = await loadReleaseWasm(entry.version);
  } else {
    throw new Error(`Unknown version type: ${entry.type}`);
  }

  configureHost(wasmModule);
}

/**
 * Post-process the Verter WASM output to match the Vue SFC playground format.
 *
 * Transforms:
 *   export default _defineComponent({...}); function render(...) {...}
 * Into:
 *   const __sfc__ = _defineComponent({...}); function render(...) {...}
 *   __sfc__.render = render; export default __sfc__;
 *
 * This ensures the render function is properly attached to the component object.
 */
export function mergeRenderIntoComponent(code: string): string {
  let merged = code;

  // Detect if compiler already used "const __sfc__" (scoped styles emit this)
  const hasSfcVariable = /^const __sfc__ = /m.test(merged);

  if (!hasSfcVariable) {
    // Non-scoped: transform "export default" → "const __sfc__ ="
    const before = merged;
    merged = merged.replace(/^export default /m, "const __sfc__ = ");
    if (merged === before) {
      // No "export default" found either — template-only component (no script block).
      // Create an empty component object so __sfc__ is defined.
      merged = "const __sfc__ = {};\n" + merged;
    }
  }

  // Only attach render if the output contains a render function declaration
  const hasRender = /^function render\s*\(/m.test(merged);

  // Find insertion point: before existing "export default __sfc__" or at end
  const exportMatch = merged.indexOf("\nexport default __sfc__");
  const insertPoint = exportMatch !== -1 ? exportMatch : merged.length;

  let attachment = "";
  if (hasRender) {
    attachment += "\n__sfc__.render = render;";
  }
  if (exportMatch === -1) {
    // No "export default __sfc__" yet — add it
    attachment += "\nexport default __sfc__;\n";
  }

  merged = merged.slice(0, insertPoint) + attachment + merged.slice(insertPoint);
  return merged;
}

function compileVueWithHost(file: File, options: CompilerOptions | undefined): CompileTiming {
  const start = performance.now();
  const profile = toHostProfile(file, options);

  const upsertResult = wasmHost!.upsert({
    inputId: file.filename,
    source: file.code,
    fileKind: "vue",
    aliases: [],
    compileProfile: profile,
  });

  const nodes = wasmHost!.listVirtualFiles(file.filename);
  const nodeKinds = new Set(nodes.map((node) => node.kind));
  const diagnosticsSnapshots: Array<HostDiagnosticsSnapshot | undefined> = [upsertResult.diagnostics];

  let assembledJs = "";
  let templateSourceMap = "";

  if (nodeKinds.has("script")) {
    const script = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=script`,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(script.diagnostics);
    assembledJs += script.code;
  }

  if (nodeKinds.has("template")) {
    const template = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=template`,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(template.diagnostics);
    if (assembledJs) assembledJs += "\n";
    assembledJs += template.code;
    templateSourceMap = template.sourceMap ?? "";
  }

  if (!assembledJs) {
    const main = wasmHost!.getVirtualFile({
      rawId: file.filename,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(main.diagnostics);
    assembledJs = main.code;
  }

  const styleIndices = nodes
    .filter((node): node is HostVirtualNodeKind => node.kind === "style" && node.index != null)
    .map((node) => node.index as number)
    .sort((a, b) => a - b);

  const styleChunks: string[] = [];
  for (const index of styleIndices) {
    const style = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=style&index=${index}`,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(style.diagnostics);
    styleChunks.push(style.code);
  }

  file.compiled.js = mergeRenderIntoComponent(assembledJs);
  file.compiled.css = styleChunks.join("\n");
  file.compiled.verterSourceMap = templateSourceMap;
  file.compiled.errors = formatDiagnostics(collectUniqueHostDiagnostics(diagnosticsSnapshots));

  // Retrieve analysis data if available (backward compat: older WASM may lack getAnalysis)
  let analysis: FileAnalysis | null = null;
  if (typeof wasmHost!.getAnalysis === "function") {
    try {
      analysis = wasmHost!.getAnalysis(file.filename) ?? null;
    } catch {
      // Silently ignore - analysis is optional
    }
  }
  file.compiled.analysis = analysis;

  // Retrieve TSX types output (backward compat: older WASM may not produce this node)
  if (nodeKinds.has("tsx")) {
    try {
      const tsx = wasmHost!.getVirtualFile({
        rawId: `${file.filename}?vue&type=tsx`,
        compileProfile: profile,
      });
      file.compiled.types = tsx.code ?? "";
    } catch {
      // Silently ignore - TSX output is optional
    }
  } else {
    file.compiled.types = "";
  }

  return {
    verterNew: null,
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
  };
}

function compileTsWithHost(file: File, options: CompilerOptions | undefined): CompileTiming {
  const start = performance.now();
  const vueFilename = file.filename.replace(/\.ts$/, ".vue");
  const sfc = `<script setup lang="ts">\n${file.code}\n</script>`;
  const profile = toHostProfile(file, options);
  profile.filename = vueFilename;

  const upsertResult = wasmHost!.upsert({
    inputId: vueFilename,
    source: sfc,
    fileKind: "vue",
    aliases: [],
    compileProfile: profile,
  });

  const diagnosticsSnapshots: Array<HostDiagnosticsSnapshot | undefined> = [upsertResult.diagnostics];

  const script = wasmHost!.getVirtualFile({
    rawId: `${vueFilename}?vue&type=script`,
    compileProfile: profile,
  });
  diagnosticsSnapshots.push(script.diagnostics);

  file.compiled.js = script.code;
  file.compiled.errors = formatDiagnostics(collectUniqueHostDiagnostics(diagnosticsSnapshots));

  // Retrieve analysis data if available
  let analysis: FileAnalysis | null = null;
  if (typeof wasmHost!.getAnalysis === "function") {
    try {
      analysis = wasmHost!.getAnalysis(vueFilename) ?? null;
    } catch {
      // Silently ignore
    }
  }
  file.compiled.analysis = analysis;

  // Retrieve TSX types output for .ts files (backward compat: older WASM may not produce this)
  try {
    const tsx = wasmHost!.getVirtualFile({
      rawId: `${vueFilename}?vue&type=tsx`,
      compileProfile: profile,
    });
    file.compiled.types = tsx.code ?? "";
  } catch {
    file.compiled.types = "";
  }

  return {
    verterNew: null,
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
  };
}

const HOST_UNAVAILABLE_ERROR =
  "VerterHost is not available in this WASM version. Please switch to a newer version.";

export async function compileFile(
  file: File,
  options?: CompilerOptions,
): Promise<CompileTiming> {
  await initCompilers();
  const timing: CompileTiming = { verterNew: null, verterNewJs: null, parseDurationMs: null };

  if (file.filename.endsWith(".vue")) {
    if (!wasmHost) {
      file.compiled.errors = [HOST_UNAVAILABLE_ERROR];
      return timing;
    }
    return compileVueWithHost(file, options);
  } else if (file.filename.endsWith(".ts")) {
    if (!wasmHost) {
      file.compiled.js = "";
      file.compiled.errors = [HOST_UNAVAILABLE_ERROR];
      return timing;
    }
    return compileTsWithHost(file, options);
  } else if (file.filename.endsWith(".js")) {
    file.compiled.js = file.code;
    file.compiled.errors = [];
  } else if (file.filename.endsWith(".css")) {
    file.compiled.css = file.code;
    file.compiled.errors = [];
  }

  return timing;
}
