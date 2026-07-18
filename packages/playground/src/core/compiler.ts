import type {
  File,
  CompilerOptions,
  CompileTiming,
  FileAnalysis,
  LintDiagnostic,
  HostDiagnostic,
} from "./types";
import { loadLocalWasm, loadCommitWasm, loadReleaseWasm, type WasmModule } from "./wasmLoader";
import type { VersionEntry } from "./versions";
import { combineSourceMaps } from "./sourcemap";
import {
  type ClientFramework,
  type HostFileKind,
  allFrameworkExtensions,
  detectFrameworkId,
  fileKindForFilename,
  frameworkById,
} from "./frameworks";

// Inline types matching Rust WASM bindings (avoid @verter/wasm import resolution issues).
// `spanStart`/`spanEnd` are absolute source offsets in UTF-16 unless a field is
// explicitly documented as generated TSX output metadata.
export interface HostTextEdit {
  spanStart: number;
  spanEnd: number;
  newText: string;
}

export interface HostCodeAction {
  title: string;
  kind: string;
  edits: HostTextEdit[];
  isPreferred: boolean;
  diagnosticRule?: string;
}

export interface HostLintRuleMetadata {
  name: string;
  category: string;
  defaultSeverity: string;
}

export interface HostDocumentSymbol {
  name: string;
  detail?: string;
  kind: number;
  spanStart: number;
  spanEnd: number;
  selectionStart: number;
  selectionEnd: number;
  children: HostDocumentSymbol[];
}

export interface HostElementMatch {
  tag: string;
  spanStart: number;
  spanEnd: number;
  result: "match" | "maybe" | "no";
}

export interface HostSelectorMatchResult {
  selectorText: string;
  selectorStart: number;
  selectorEnd: number;
  matches: HostElementMatch[];
}

interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  ssr?: boolean;
  hmrStrategy?: "none" | "vite" | "webpack";
  forceJs?: boolean;
  sourceMap?: boolean;
  target?: "bundler" | "ide" | "analysis" | "full";
  strictSlots?: boolean;
}

interface HostVirtualNodeKind {
  kind: "main" | "script" | "template" | "style" | "custom";
  index?: number;
}

interface HostIdeResponse {
  code: string;
  sourceMap?: string;
  destructuredBlock?: {
    bindings: Array<{ name: string; sourceStart: number; sourceEnd: number }>;
    blockStart: number;
    blockEnd: number;
  } | null;
}

interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[];
  hasErrors: boolean;
}

export interface HostModuleReference {
  syntax: "staticImport" | "exportFrom" | "dynamicImport" | "requireCall";
  semantics: "import" | "require";
  isTypeOnly: boolean;
  rawText: string;
  literalSpecifier?: string;
  finiteSpecifiers: string[];
  staticPrefix?: string;
  analyzability: "exact" | "finiteSet" | "unknownDynamic";
  spanStart: number;
  spanEnd: number;
  exprSpanStart: number;
  exprSpanEnd: number;
}

interface HostDependencyResolution {
  specifier: string;
  resolvedCanonicalId?: string;
  possibleCanonicalIds?: string[];
}

interface HostUpdateResult {
  diagnostics: HostDiagnosticsSnapshot;
  moduleReferences?: HostModuleReference[];
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
    // The host `fileKind`: a registered framework adapter id (descriptor-driven,
    // e.g. "vue" / "svelte") or a plain non-framework kind ("non_sfc" / "text" /
    // "file" / a script dialect). Driven by the manifest, never a Vue+Svelte literal.
    fileKind: HostFileKind;
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
  getAnalysis(canonicalOrAlias: string): FileAnalysis | null;
  getIde(canonicalId: string, profile?: HostCompileProfile): HostIdeResponse | null;
  getPublicApi?(canonicalId: string, mode?: "public" | "declaration"): HostIdeResponse | null;
  lint(canonicalOrAlias: string, config?: unknown): LintDiagnostic[];
  getCodeActions?(canonicalOrAlias: string, offset: number): HostCodeAction[];
  getLintRuleMetadata?(): HostLintRuleMetadata[];
  getDocumentSymbols?(canonicalOrAlias: string): HostDocumentSymbol[];
  matchCssSelectors?(canonicalOrAlias: string): HostSelectorMatchResult[];
  setImportDependencies?(canonicalOrAlias: string, resolutions: HostDependencyResolution[]): void;
  collectResolvableModuleReferenceSpecifiers?(moduleReferences: HostModuleReference[]): string[];
  resolveKnownModuleReferenceDependencies?(
    ownerCanonicalId: string,
    moduleReferences: HostModuleReference[],
    knownIds: string[],
    extensions?: string[],
  ): string[];
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

/**
 * Test-only: directly inject a mock host binding and mark the compiler as initialized.
 * Returns a teardown function that restores the previous state.
 */
export function __setHostForTest(host: HostBinding): () => void {
  const prevHost = wasmHost;
  const prevInit = initialized;
  const prevPromise = initPromise;
  wasmHost = host;
  initialized = true;
  initPromise = null;
  return () => {
    wasmHost = prevHost;
    initialized = prevInit;
    initPromise = prevPromise;
  };
}

function toHostProfile(file: File, options?: CompilerOptions): HostCompileProfile {
  return {
    filename: file.filename,
    isProduction: options?.isProduction ?? false,
    ssr: options?.ssr ?? false,
    hmrStrategy: "none",
    forceJs: true,
    sourceMap: true,
    target: "full",
    strictSlots: options?.strictSlots ?? false,
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

type KnownFiles = Readonly<Record<string, File>>;

// Base TS/JS resolution extensions plus every framework-owned extension derived
// from the manifest (carrier + adapter-module), so a new framework adapter needs
// no edit here.
const BASE_RESOLVE_EXTENSIONS = ["", ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"] as const;
const PLAYGROUND_RESOLVE_EXTENSIONS: readonly string[] = [
  ...BASE_RESOLVE_EXTENSIONS,
  ...allFrameworkExtensions(),
];

function normalizeModuleFileId(fileId: string): string {
  const segments: string[] = [];
  for (const segment of fileId.replace(/\\/g, "/").split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") {
      segments.pop();
      continue;
    }
    segments.push(segment);
  }
  return segments.join("/");
}

function resolveRelativeModuleFileId(fromFile: string, specifier: string): string | null {
  if (!specifier.startsWith(".")) return null;

  const baseSegments = normalizeModuleFileId(fromFile).split("/").filter(Boolean);
  baseSegments.pop();

  for (const segment of specifier.replace(/\\/g, "/").split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") {
      baseSegments.pop();
      continue;
    }
    baseSegments.push(segment);
  }

  return baseSegments.join("/");
}

function buildKnownFileIndex(knownFiles: KnownFiles): Map<string, File> {
  const index = new Map<string, File>();
  for (const file of Object.values(knownFiles)) {
    index.set(normalizeModuleFileId(file.filename), file);
  }
  return index;
}

function collectResolvableModuleReferenceSpecifiers(
  moduleReferences: readonly HostModuleReference[] | undefined,
): string[] {
  const seen = new Set<string>();
  const specifiers: string[] = [];

  for (const reference of moduleReferences ?? []) {
    const candidates =
      reference.analyzability === "exact"
        ? reference.literalSpecifier
          ? [reference.literalSpecifier]
          : []
        : reference.analyzability === "finiteSet"
          ? reference.finiteSpecifiers
          : [];

    for (const specifier of candidates) {
      if (!specifier || seen.has(specifier)) continue;
      seen.add(specifier);
      specifiers.push(specifier);
    }
  }

  return specifiers;
}

function resolveKnownDependencyFile(
  ownerFilename: string,
  specifier: string,
  knownIndex: ReadonlyMap<string, File>,
): File | null {
  const resolvedBase = resolveRelativeModuleFileId(ownerFilename, specifier);
  if (!resolvedBase) return null;

  const candidates = new Set<string>();
  candidates.add(resolvedBase);
  for (const ext of PLAYGROUND_RESOLVE_EXTENSIONS) {
    if (ext) {
      candidates.add(`${resolvedBase}${ext}`);
      candidates.add(`${resolvedBase}/index${ext}`);
    }
  }

  for (const candidate of candidates) {
    const match = knownIndex.get(candidate);
    if (match) return match;
  }

  return null;
}

export function resolveKnownModuleReferenceDependencies(
  ownerFilename: string,
  moduleReferences: readonly HostModuleReference[] | undefined,
  knownFiles: KnownFiles,
): string[] {
  const knownIndex = buildKnownFileIndex(knownFiles);
  const ownerId = normalizeModuleFileId(ownerFilename);
  const resolved: string[] = [];
  const seen = new Set<string>();

  for (const specifier of collectResolvableModuleReferenceSpecifiers(moduleReferences)) {
    const match = resolveKnownDependencyFile(ownerFilename, specifier, knownIndex);
    if (!match) continue;

    const matchId = normalizeModuleFileId(match.filename);
    if (matchId === ownerId || seen.has(matchId)) continue;

    seen.add(matchId);
    resolved.push(match.filename);
  }

  return resolved;
}

function syncKnownModuleReferenceDependencies(
  ownerFilename: string,
  moduleReferences: readonly HostModuleReference[] | undefined,
  knownFiles: KnownFiles,
): void {
  if (!wasmHost || typeof wasmHost.setImportDependencies !== "function") return;

  const knownIndex = buildKnownFileIndex(knownFiles);
  const resolvedDeps =
    typeof wasmHost.resolveKnownModuleReferenceDependencies === "function"
      ? wasmHost.resolveKnownModuleReferenceDependencies(
          ownerFilename,
          [...(moduleReferences ?? [])],
          Object.values(knownFiles).map((file) => file.filename),
          [...PLAYGROUND_RESOLVE_EXTENSIONS],
        )
      : resolveKnownModuleReferenceDependencies(ownerFilename, moduleReferences, knownFiles);
  wasmHost.setImportDependencies(
    ownerFilename,
    resolvedDeps.map((dep) => ({ specifier: dep, resolvedCanonicalId: dep })),
  );

  const visited = new Set<string>();
  const pending = [...resolvedDeps];
  while (pending.length > 0) {
    const depFilename = pending.shift()!;
    const depId = normalizeModuleFileId(depFilename);
    if (visited.has(depId)) continue;
    visited.add(depId);

    const depFile = knownIndex.get(depId);
    if (!depFile) continue;

    const depResult = wasmHost.upsert({
      inputId: depFile.filename,
      source: depFile.code,
      // Descriptor lookup: a framework carrier/adapter-module maps to its
      // framework id, everything else to the plain non-framework fallback.
      fileKind: fileKindForFilename(depFile.filename),
      aliases: [],
    });

    const childDeps =
      typeof wasmHost.resolveKnownModuleReferenceDependencies === "function"
        ? wasmHost.resolveKnownModuleReferenceDependencies(
            depFile.filename,
            depResult.moduleReferences ?? [],
            Object.values(knownFiles).map((file) => file.filename),
            [...PLAYGROUND_RESOLVE_EXTENSIONS],
          )
        : resolveKnownModuleReferenceDependencies(
            depFile.filename,
            depResult.moduleReferences,
            knownFiles,
          );
    wasmHost.setImportDependencies(
      depFile.filename,
      childDeps.map((dep) => ({ specifier: dep, resolvedCanonicalId: dep })),
    );
    pending.push(...childDeps);
  }
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

  // Attach render or ssrRender if the output contains their function declarations
  const hasRender = /^function render\s*\(/m.test(merged);
  const hasSsrRender = /^function ssrRender\s*\(/m.test(merged);

  // Find insertion point: before existing "export default __sfc__" or at end
  const exportMatch = merged.indexOf("\nexport default __sfc__");
  const insertPoint = exportMatch !== -1 ? exportMatch : merged.length;

  let attachment = "";
  if (hasRender) {
    attachment += "\n__sfc__.render = render;";
  }
  if (hasSsrRender) {
    attachment += "\n__sfc__.ssrRender = ssrRender;";
  }
  if (exportMatch === -1) {
    // No "export default __sfc__" yet — add it
    attachment += "\nexport default __sfc__;\n";
  }

  merged = merged.slice(0, insertPoint) + attachment + merged.slice(insertPoint);
  return merged;
}

/** Apply TSX output and keep its source map isolated from template source maps. */
export function applyTsxOutput(file: File, tsx: HostIdeResponse | null | undefined): void {
  file.compiled.types = tsx?.code ?? "";
  file.compiled.typesSourceMap = tsx?.sourceMap ?? "";
  file.compiled.destructuredBlock = tsx?.destructuredBlock ?? null;
}

/** Build a LintConfig object with disabled rules for the WASM host. */
function buildLintConfig(disabledRules?: ReadonlySet<string>): Record<string, unknown> | undefined {
  if (!disabledRules || disabledRules.size === 0) return undefined;
  const rules: Record<string, null> = {};
  for (const name of disabledRules) {
    rules[name] = null;
  }
  return { preset: "All", rules, vaporMode: false, ssrMode: false };
}

/** Re-run only the lint pass for a file (avoids full recompile). */
export function relintFile(file: File, disabledRules?: ReadonlySet<string>): number | null {
  if (!wasmHost || typeof wasmHost.lint !== "function") return null;
  try {
    const t0 = performance.now();
    file.compiled.lintDiagnostics =
      wasmHost.lint(file.filename, buildLintConfig(disabledRules)) ?? [];
    return performance.now() - t0;
  } catch {
    file.compiled.lintDiagnostics = [];
    return null;
  }
}

/**
 * Whether a framework's compiled client output is assembled through the Vue
 * VDOM render-function pipeline (script + template + style virtual files merged
 * via {@link mergeRenderIntoComponent}, plus an SSR pass). This is the
 * Vue-runtime assembly convention — the `?vue&type=script|template|style` raw
 * virtual-file query syntax is owned by the Vue adapter. Other frameworks read
 * a single main virtual file and never touch `mergeRenderIntoComponent`.
 */
function usesVueRenderAssembly(framework: ClientFramework): boolean {
  return framework.frameworkId === "vue";
}

/**
 * Compile a framework carrier file through the host, descriptor-driven: the
 * upsert `fileKind` is the framework adapter id. Vue uses the VDOM render
 * assembly; every other framework reads its main virtual file and the shared
 * IDE-TSX / public-API / analysis / lint surfaces.
 */
function compileFrameworkWithHost(
  file: File,
  framework: ClientFramework,
  options: CompilerOptions | undefined,
  disabledRules?: ReadonlySet<string>,
  knownFiles?: KnownFiles,
): CompileTiming {
  if (usesVueRenderAssembly(framework)) {
    return compileVueRenderAssembly(file, framework, options, disabledRules, knownFiles);
  }
  return compileGenericFrameworkSurfaces(file, framework, options, disabledRules, knownFiles);
}

function compileVueRenderAssembly(
  file: File,
  framework: ClientFramework,
  options: CompilerOptions | undefined,
  disabledRules?: ReadonlySet<string>,
  knownFiles?: KnownFiles,
): CompileTiming {
  const start = performance.now();
  // Always compile client output with ssr: false
  const profile = toHostProfile(file, options);
  profile.ssr = false;

  const upsertResult = wasmHost!.upsert({
    inputId: file.filename,
    source: file.code,
    fileKind: framework.frameworkId,
    aliases: [],
    compileProfile: profile,
  });

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(file.filename, upsertResult.moduleReferences, knownFiles);
  }

  const nodes = wasmHost!.listVirtualFiles(file.filename);
  const nodeKinds = new Set(nodes.map((node) => node.kind));
  const diagnosticsSnapshots: Array<HostDiagnosticsSnapshot | undefined> = [
    upsertResult.diagnostics,
  ];

  let assembledJs = "";
  let scriptCode = "";
  let scriptSourceMap = "";
  let templateCode = "";
  let templateSourceMap = "";

  let scriptMs: number | null = null;
  let templateMs: number | null = null;
  let styleMs: number | null = null;

  if (nodeKinds.has("script")) {
    const t0 = performance.now();
    const script = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=script`,
      compileProfile: profile,
    });
    scriptMs = performance.now() - t0;
    diagnosticsSnapshots.push(script.diagnostics);
    scriptCode = script.code;
    scriptSourceMap = script.sourceMap ?? "";
    assembledJs += script.code;
  }

  if (nodeKinds.has("template")) {
    const t0 = performance.now();
    const template = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=template`,
      compileProfile: profile,
    });
    templateMs = performance.now() - t0;
    diagnosticsSnapshots.push(template.diagnostics);
    if (assembledJs) assembledJs += "\n";
    assembledJs += template.code;
    templateCode = template.code;
    templateSourceMap = template.sourceMap ?? "";
    file.compiled.templateCode = template.code;
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

  const styleStart = performance.now();
  const styleChunks: string[] = [];
  for (const index of styleIndices) {
    const style = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?vue&type=style&index=${index}`,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(style.diagnostics);
    styleChunks.push(style.code);
  }
  styleMs = styleIndices.length > 0 ? performance.now() - styleStart : null;

  const allDiagnostics = collectUniqueHostDiagnostics(diagnosticsSnapshots);
  file.compiled.js = mergeRenderIntoComponent(assembledJs);
  file.compiled.css = styleChunks.join("\n");
  // Combine script + template source maps into a single map covering file.compiled.js.
  // This handles all offsets: SFC prefix lines, host import prepend, mergeRenderIntoComponent.
  file.compiled.verterSourceMap = combineSourceMaps({
    scriptMap: scriptSourceMap,
    scriptCode,
    templateMap: templateSourceMap,
    templateCode,
    vueSource: file.code,
    finalJs: file.compiled.js,
  });
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;

  // Retrieve analysis data.
  let analysis: FileAnalysis | null = null;
  try {
    analysis = wasmHost!.getAnalysis(file.filename) ?? null;
  } catch {
    // Silently ignore - analysis is optional
  }
  file.compiled.analysis = analysis;

  // Run linter.
  let lintMs: number | null = null;
  try {
    const t0 = performance.now();
    file.compiled.lintDiagnostics =
      wasmHost!.lint(file.filename, buildLintConfig(disabledRules)) ?? [];
    lintMs = performance.now() - t0;
  } catch {
    file.compiled.lintDiagnostics = [];
  }

  // Retrieve TSX types output via dedicated API.
  let tsxMs: number | null = null;
  try {
    const t0 = performance.now();
    const tsx = wasmHost!.getIde(file.filename, profile);
    tsxMs = performance.now() - t0;
    applyTsxOutput(file, tsx);
  } catch {
    applyTsxOutput(file, null);
  }

  // Retrieve public API output (minimal .d.ts declarations) + the
  // declaration-carrier surface (getPublicApi(id, "declaration")).
  let tscMs: number | null = null;
  if (typeof wasmHost!.getPublicApi === "function") {
    try {
      const t0 = performance.now();
      const tsc = wasmHost!.getPublicApi(file.filename);
      const decl = wasmHost!.getPublicApi(file.filename, "declaration");
      tscMs = performance.now() - t0;
      file.compiled.tscCode = tsc?.code ?? "";
      file.compiled.declCode = decl?.code ?? "";
      file.compiled.declSourceMap = decl?.sourceMap ?? "";
    } catch {
      file.compiled.tscCode = "";
      file.compiled.declCode = "";
      file.compiled.declSourceMap = "";
    }
  } else {
    file.compiled.tscCode = "";
    file.compiled.declCode = "";
    file.compiled.declSourceMap = "";
  }

  // SSR compilation pass: when SSR is toggled on, compile again with ssr: true
  if (options?.ssr) {
    try {
      const ssrProfile = { ...profile, ssr: true };
      // Upsert with SSR profile (host caches by profile, so this is a separate entry)
      wasmHost!.upsert({
        inputId: file.filename,
        source: file.code,
        fileKind: framework.frameworkId,
        aliases: [],
        compileProfile: ssrProfile,
      });

      let ssrJs = "";
      if (nodeKinds.has("script")) {
        const ssrScript = wasmHost!.getVirtualFile({
          rawId: `${file.filename}?vue&type=script`,
          compileProfile: ssrProfile,
        });
        ssrJs += ssrScript.code;
      }
      if (nodeKinds.has("template")) {
        const ssrTemplate = wasmHost!.getVirtualFile({
          rawId: `${file.filename}?vue&type=template`,
          compileProfile: ssrProfile,
        });
        if (ssrJs) ssrJs += "\n";
        ssrJs += ssrTemplate.code;
      }
      if (!ssrJs) {
        const ssrMain = wasmHost!.getVirtualFile({
          rawId: file.filename,
          compileProfile: ssrProfile,
        });
        ssrJs = ssrMain.code;
      }
      file.compiled.ssrCode = mergeRenderIntoComponent(ssrJs);
    } catch {
      file.compiled.ssrCode = "// SSR compilation failed";
    }
  } else {
    file.compiled.ssrCode = "";
  }

  return {
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
    scriptMs,
    templateMs,
    styleMs,
    tsxMs,
    tscMs,
    lintMs,
  };
}

/**
 * Compile a non-Vue framework carrier through the shared host surfaces: a
 * single main virtual file for the client JS, plus the shared IDE-TSX,
 * public-API, analysis, and lint outputs. Never uses the Vue VDOM render
 * assembly or {@link mergeRenderIntoComponent}.
 */
function compileGenericFrameworkSurfaces(
  file: File,
  framework: ClientFramework,
  options: CompilerOptions | undefined,
  disabledRules?: ReadonlySet<string>,
  knownFiles?: KnownFiles,
): CompileTiming {
  const start = performance.now();
  const profile = toHostProfile(file, options);
  profile.ssr = false;

  const upsertResult = wasmHost!.upsert({
    inputId: file.filename,
    source: file.code,
    fileKind: framework.frameworkId,
    aliases: [],
    compileProfile: profile,
  });

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(file.filename, upsertResult.moduleReferences, knownFiles);
  }

  const diagnosticsSnapshots: Array<HostDiagnosticsSnapshot | undefined> = [
    upsertResult.diagnostics,
  ];

  // Client JS: the single main virtual file (no render-function merge).
  const main = wasmHost!.getVirtualFile({
    rawId: file.filename,
    compileProfile: profile,
  });
  diagnosticsSnapshots.push(main.diagnostics);

  const styleIndices = wasmHost!
    .listVirtualFiles(file.filename)
    .filter((node): node is HostVirtualNodeKind => node.kind === "style" && node.index != null)
    .map((node) => node.index as number)
    .sort((a, b) => a - b);
  const styleStart = performance.now();
  const styleChunks: string[] = [];
  for (const index of styleIndices) {
    const style = wasmHost!.getVirtualFile({
      rawId: `${file.filename}?verter&type=style&index=${index}`,
      compileProfile: profile,
    });
    diagnosticsSnapshots.push(style.diagnostics);
    styleChunks.push(style.code);
  }
  const styleMs = styleIndices.length > 0 ? performance.now() - styleStart : null;

  const allDiagnostics = collectUniqueHostDiagnostics(diagnosticsSnapshots);
  file.compiled.js = main.code;
  file.compiled.css = styleChunks.join("\n");
  file.compiled.ssrCode = "";
  file.compiled.verterSourceMap = main.sourceMap ?? "";
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;

  let analysis: FileAnalysis | null = null;
  if (typeof wasmHost!.getAnalysis === "function") {
    try {
      analysis = wasmHost!.getAnalysis(file.filename) ?? null;
    } catch {
      // Analysis is optional.
    }
  }
  file.compiled.analysis = analysis;

  let lintMs: number | null = null;
  if (typeof wasmHost!.lint === "function") {
    try {
      const t0 = performance.now();
      file.compiled.lintDiagnostics =
        wasmHost!.lint(file.filename, buildLintConfig(disabledRules)) ?? [];
      lintMs = performance.now() - t0;
    } catch {
      file.compiled.lintDiagnostics = [];
    }
  } else {
    file.compiled.lintDiagnostics = [];
  }

  let tsxMs: number | null = null;
  if (typeof wasmHost!.getIde === "function") {
    try {
      const t0 = performance.now();
      const tsx = wasmHost!.getIde(file.filename, profile);
      tsxMs = performance.now() - t0;
      applyTsxOutput(file, tsx);
    } catch {
      applyTsxOutput(file, null);
    }
  } else {
    applyTsxOutput(file, null);
  }

  let tscMs: number | null = null;
  if (typeof wasmHost!.getPublicApi === "function") {
    try {
      const t0 = performance.now();
      const tsc = wasmHost!.getPublicApi(file.filename);
      const decl = wasmHost!.getPublicApi(file.filename, "declaration");
      tscMs = performance.now() - t0;
      file.compiled.tscCode = tsc?.code ?? "";
      file.compiled.declCode = decl?.code ?? "";
      file.compiled.declSourceMap = decl?.sourceMap ?? "";
    } catch {
      file.compiled.tscCode = "";
      file.compiled.declCode = "";
      file.compiled.declSourceMap = "";
    }
  } else {
    file.compiled.tscCode = "";
    file.compiled.declCode = "";
    file.compiled.declSourceMap = "";
  }

  return {
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
    scriptMs: null,
    templateMs: null,
    styleMs,
    tsxMs,
    tscMs,
    lintMs,
  };
}

function compileTsWithHost(
  file: File,
  options: CompilerOptions | undefined,
  disabledRules?: ReadonlySet<string>,
  knownFiles?: KnownFiles,
): CompileTiming {
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

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(vueFilename, upsertResult.moduleReferences, knownFiles);
  }

  const diagnosticsSnapshots: Array<HostDiagnosticsSnapshot | undefined> = [
    upsertResult.diagnostics,
  ];

  const script = wasmHost!.getVirtualFile({
    rawId: `${vueFilename}?vue&type=script`,
    compileProfile: profile,
  });
  diagnosticsSnapshots.push(script.diagnostics);

  const allDiagnostics = collectUniqueHostDiagnostics(diagnosticsSnapshots);
  file.compiled.js = script.code;
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;

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

  // Run linter
  if (typeof wasmHost!.lint === "function") {
    try {
      file.compiled.lintDiagnostics =
        wasmHost!.lint(vueFilename, buildLintConfig(disabledRules)) ?? [];
    } catch {
      file.compiled.lintDiagnostics = [];
    }
  } else {
    file.compiled.lintDiagnostics = [];
  }

  // Retrieve TSX types output via dedicated API
  if (typeof wasmHost!.getIde === "function") {
    try {
      const tsx = wasmHost!.getIde(vueFilename, profile);
      applyTsxOutput(file, tsx);
    } catch {
      applyTsxOutput(file, null);
    }
  } else {
    applyTsxOutput(file, null);
  }

  // Public API output for TS-only mode (+ the declaration surface)
  if (typeof wasmHost!.getPublicApi === "function") {
    try {
      const tsc = wasmHost!.getPublicApi(vueFilename);
      const decl = wasmHost!.getPublicApi(vueFilename, "declaration");
      file.compiled.tscCode = tsc?.code ?? "";
      file.compiled.declCode = decl?.code ?? "";
      file.compiled.declSourceMap = decl?.sourceMap ?? "";
    } catch {
      file.compiled.tscCode = "";
      file.compiled.declCode = "";
      file.compiled.declSourceMap = "";
    }
  } else {
    file.compiled.tscCode = "";
    file.compiled.declCode = "";
    file.compiled.declSourceMap = "";
  }

  return {
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
  };
}

const HOST_UNAVAILABLE_ERROR =
  "VerterHost is not available in this WASM version. Please switch to a newer version.";

export async function compileFile(
  file: File,
  options?: CompilerOptions,
  disabledRules?: ReadonlySet<string>,
  knownFiles?: KnownFiles,
): Promise<CompileTiming> {
  await initCompilers();
  const timing: CompileTiming = {
    verterNewJs: null,
    parseDurationMs: null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
  };

  // Descriptor-driven dispatch: a registered framework carrier / adapter-module
  // compiles through the shared framework path (fileKind = framework id).
  const detectedFrameworkId = detectFrameworkId(file.filename);
  const framework = detectedFrameworkId ? frameworkById(detectedFrameworkId) : undefined;

  if (framework) {
    if (!wasmHost) {
      file.compiled.errors = [HOST_UNAVAILABLE_ERROR];
      return timing;
    }
    return compileFrameworkWithHost(file, framework, options, disabledRules, knownFiles);
  } else if (file.filename.endsWith(".ts")) {
    if (!wasmHost) {
      file.compiled.js = "";
      file.compiled.errors = [HOST_UNAVAILABLE_ERROR];
      return timing;
    }
    return compileTsWithHost(file, options, disabledRules, knownFiles);
  } else if (file.filename.endsWith(".js")) {
    file.compiled.js = file.code;
    file.compiled.errors = [];
  } else if (file.filename.endsWith(".css")) {
    file.compiled.css = file.code;
    file.compiled.errors = [];
  }

  return timing;
}

// =============================================================================
// Host accessor functions for new playground features
// =============================================================================

/** Returns code actions at a UTF-16 offset, or empty array if unavailable. */
export function getCodeActions(canonicalOrAlias: string, offset: number): HostCodeAction[] {
  if (!wasmHost || typeof wasmHost.getCodeActions !== "function") return [];
  try {
    return wasmHost.getCodeActions(canonicalOrAlias, offset) ?? [];
  } catch {
    return [];
  }
}

/** Returns metadata for all registered lint rules, or empty array if unavailable. */
export function getLintRuleMetadata(): HostLintRuleMetadata[] {
  if (!wasmHost || typeof wasmHost.getLintRuleMetadata !== "function") return [];
  try {
    return wasmHost.getLintRuleMetadata() ?? [];
  } catch {
    return [];
  }
}

/** Returns document symbols for a file, or empty array if unavailable. */
export function getDocumentSymbols(canonicalOrAlias: string): HostDocumentSymbol[] {
  if (!wasmHost || typeof wasmHost.getDocumentSymbols !== "function") return [];
  try {
    return wasmHost.getDocumentSymbols(canonicalOrAlias) ?? [];
  } catch {
    return [];
  }
}

/** Returns CSS selector match matrix, or empty array if unavailable. */
export function matchCssSelectors(canonicalOrAlias: string): HostSelectorMatchResult[] {
  if (!wasmHost || typeof wasmHost.matchCssSelectors !== "function") return [];
  try {
    return wasmHost.matchCssSelectors(canonicalOrAlias) ?? [];
  } catch {
    return [];
  }
}
