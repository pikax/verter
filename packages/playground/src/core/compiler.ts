import type {
  File,
  CompilerOptions,
  CompileTiming,
  FileAnalysis,
  LintDiagnostic,
  HostDiagnostic,
  PublicApiProjectionError,
  PublicApiModeOutcome,
  PublicApiResponse,
  OrderedSfcStructure,
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
import type {
  BrowserHostCompileRequest,
  BrowserHostRequestedProduct,
  HostCompileIdentity,
  HostCompileRequestResponse,
  HostCompiledVirtualNode,
} from "@verter/wasm";

// Inline types matching Rust WASM bindings for surfaces the playground still
// owns locally (lint, public API, document symbols). Compile request/response
// shapes come from `@verter/wasm` so the tagged wrappers cannot drift.
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

interface HostIdeResponse {
  code: string;
  sourceMap?: string;
  destructuredBlock?: {
    bindings: Array<{ name: string; sourceStart: number; sourceEnd: number }>;
    blockStart: number;
    blockEnd: number;
  } | null;
}

interface HostPublicApiResult {
  value: PublicApiResponse | null;
  error: PublicApiProjectionError | null;
}

class HostPublicApiProjectionFailure extends Error {
  readonly projection: PublicApiProjectionError;

  constructor(projection: PublicApiProjectionError) {
    super(`public API projection failed: ${projection.code}/${projection.detailCode}`);
    this.name = "HostPublicApiProjectionFailure";
    this.projection = projection;
  }
}

function publicApiOutcome(result: HostPublicApiResult): PublicApiModeOutcome {
  if (result.error !== null) return { kind: "projectionFailure", error: result.error };
  if (result.value !== null) return { kind: "value", value: result.value };
  return { kind: "absent" };
}

function preservePublicApiProjectionFailure(
  file: File,
  projection: PublicApiProjectionError,
): void {
  const error = new HostPublicApiProjectionFailure(projection);
  const subject =
    projection.subject.kind === "macro"
      ? `macro(${projection.subject.syntaxIndex})`
      : `scriptSetupAttrs(${projection.subject.sourceRange.start}..${projection.subject.sourceRange.end})`;
  const diagnostic: HostDiagnostic = {
    severity: "error",
    code: `${projection.code}/${projection.detailCode}`,
    message: `${error.message} (subject=${subject}, declarationShapeReason=${projection.declarationShapeReason ?? "null"}, memberOrdinal=${projection.memberOrdinal ?? "null"}, outcomeKind=${projection.outcomeKind ?? "null"}, outcomeReason=${projection.outcomeReason ?? "null"}, outcomeDiagnostic=${projection.outcomeDiagnostic ?? "null"})`,
    projectionError: projection,
  };
  file.compiled.compilerDiagnostics.push(diagnostic);
  file.compiled.errors.push(formatDiagnostics([diagnostic])[0]!);
}

function applyPublicApiOutputs(
  file: File,
  canonicalId: string,
  getPublicApi: NonNullable<HostBinding["getPublicApi"]>,
): void {
  const publicOutcome = publicApiOutcome(getPublicApi(canonicalId, "public"));
  file.compiled.publicApiOutcome = publicOutcome;
  if (publicOutcome.kind === "value") {
    file.compiled.tscCode = publicOutcome.value.code;
  } else {
    file.compiled.tscCode = "";
    if (publicOutcome.kind === "projectionFailure") {
      preservePublicApiProjectionFailure(file, publicOutcome.error);
    }
  }

  const declarationOutcome = publicApiOutcome(getPublicApi(canonicalId, "declaration"));
  file.compiled.declarationOutcome = declarationOutcome;
  if (declarationOutcome.kind === "value") {
    file.compiled.declCode = declarationOutcome.value.code;
    file.compiled.declSourceMap = declarationOutcome.value.sourceMap ?? "";
  } else {
    file.compiled.declCode = "";
    file.compiled.declSourceMap = "";
    if (declarationOutcome.kind === "projectionFailure") {
      preservePublicApiProjectionFailure(file, declarationOutcome.error);
    }
  }
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

interface HostBinding {
  upsert(request: {
    inputId: string;
    source: string;
    // The host `fileKind`: a registered framework adapter id (descriptor-driven,
    // e.g. "vue" / "svelte") or a plain non-framework kind ("non_sfc" / "text" /
    // "file" / a script dialect). Driven by the manifest, never a Vue+Svelte literal.
    fileKind: HostFileKind;
    aliases?: string[];
  }): HostUpdateResult;
  compileRequest(
    canonicalId: string,
    request: BrowserHostCompileRequest,
  ): HostCompileRequestResponse;
  getDocumentStructure?(canonicalId: string): OrderedSfcStructure | null;
  getAnalysis(canonicalOrAlias: string): FileAnalysis | null;
  getPublicApi?(canonicalId: string, mode?: "public" | "declaration"): HostPublicApiResult;
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

function isHostBinding(value: unknown): value is HostBinding {
  if (value === null || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return ["upsert", "compileRequest", "getAnalysis", "lint"].every(
    (method) => typeof candidate[method] === "function",
  );
}

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

function compileIdentity(options?: CompilerOptions): HostCompileIdentity {
  return {
    isProduction: options?.isProduction ?? false,
    forceJs: true,
  };
}

function ideCompanionProduct(
  frameworkId: string,
  options?: CompilerOptions,
): BrowserHostRequestedProduct {
  const vueOnly = frameworkId === "vue";
  return {
    ideCompanion: {
      wantSourceMap: true,
      embedAmbientTypes: false,
      conditionalRootNarrowing: false,
      // Vue-only IDE axes: a true value is refused on Svelte.
      strictSlots: vueOnly ? (options?.strictSlots ?? false) : false,
      ideChunkBoundaries: false,
    },
  };
}

function clientRuntimeProducts(
  frameworkId: string,
  options?: CompilerOptions,
): BrowserHostRequestedProduct[] {
  return [{ runtimeClient: { runtimeSourceMap: true } }, ideCompanionProduct(frameworkId, options)];
}

function vueCompileRequest(
  products: BrowserHostRequestedProduct[],
  ssr: boolean,
  options?: CompilerOptions,
): BrowserHostCompileRequest {
  return {
    vue: {
      identity: compileIdentity(options),
      products,
      options: {
        backend: "inferred",
        ssr,
        isCustomElement: [],
        babelParserPlugins: [],
      },
    },
  };
}

function svelteCompileRequest(
  products: BrowserHostRequestedProduct[],
  options?: CompilerOptions,
): BrowserHostCompileRequest {
  return {
    svelte: {
      identity: compileIdentity(options),
      products,
      options: {},
    },
  };
}

function frameworkCompileRequest(
  framework: ClientFramework,
  products: BrowserHostRequestedProduct[],
  ssr: boolean,
  options?: CompilerOptions,
): BrowserHostCompileRequest {
  if (framework.frameworkId === "vue") {
    return vueCompileRequest(products, ssr, options);
  }
  if (framework.frameworkId === "svelte") {
    return svelteCompileRequest(products, options);
  }
  throw new Error(`typed compile request has no arm for framework '${framework.frameworkId}'`);
}

function runtimeNodes(
  products: HostCompileRequestResponse["products"],
  kind: "runtimeClient" | "runtimeServer",
): HostCompiledVirtualNode[] {
  const row = products.find((product) => product.kind === kind);
  if (!row || !("nodes" in row)) return [];
  return row.nodes;
}

function firstNode(
  nodes: HostCompiledVirtualNode[],
  kind: HostCompiledVirtualNode["node"]["kind"],
): HostCompiledVirtualNode | undefined {
  return nodes.find((node) => node.node.kind === kind);
}

function styleNodeCodes(nodes: HostCompiledVirtualNode[]): string[] {
  return nodes
    .filter((node) => node.node.kind === "style")
    .sort((a, b) => (a.node.index ?? 0) - (b.node.index ?? 0))
    .map((node) => node.code);
}

function hostRefusalMessage(error: unknown): string {
  if (typeof error === "string" && error.length > 0) return error;
  if (error instanceof Error && error.message.length > 0) return error.message;
  return "typed compile request was refused";
}

function emptyCompileTiming(overrides: Partial<CompileTiming> = {}): CompileTiming {
  return {
    verterNewJs: null,
    parseDurationMs: null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs: null,
    tscMs: null,
    lintMs: null,
    ...overrides,
  };
}

function wipeCompiledSurfaces(file: File): void {
  file.compiled.js = "";
  file.compiled.css = "";
  file.compiled.templateCode = "";
  file.compiled.verterSourceMap = "";
  file.compiled.ssrCode = "";
  applyTsxOutput(file, null);
  file.compiled.analysis = null;
  file.compiled.lintDiagnostics = [];
  file.compiled.tscCode = "";
  file.compiled.publicApiOutcome = { kind: "absent" };
  file.compiled.declCode = "";
  file.compiled.declarationOutcome = { kind: "absent" };
  file.compiled.declSourceMap = "";
}

function recordCompileHalt(
  file: File,
  diagnostic: HostDiagnostic,
  upsertDiagnostics?: HostDiagnosticsSnapshot,
): void {
  const allDiagnostics = collectUniqueHostDiagnostics([
    upsertDiagnostics,
    { diagnostics: [diagnostic], hasErrors: true },
  ]);
  wipeCompiledSurfaces(file);
  file.compiled.compilerDiagnostics = allDiagnostics;
  file.compiled.errors = formatDiagnostics(allDiagnostics);
}

function recordCompileRefusal(
  file: File,
  message: string,
  upsertDiagnostics?: HostDiagnosticsSnapshot,
): void {
  recordCompileHalt(
    file,
    {
      severity: "error",
      code: "compile-request-refused",
      message,
    },
    upsertDiagnostics,
  );
}

function recordUnexpectedCompileFailure(file: File, error: unknown): void {
  recordCompileHalt(file, {
    severity: "error",
    code: "compile-unexpected-error",
    message: hostRefusalMessage(error),
  });
}

const MISSING_RUNTIME_PRODUCT: HostDiagnostic = {
  severity: "error",
  code: "missing-runtime-product",
  message: "typed compile request returned no runtimeClient output",
};

function hasRuntimeJsNodes(nodes: HostCompiledVirtualNode[]): boolean {
  return nodes.some(
    (node) =>
      node.node.kind === "main" || node.node.kind === "script" || node.node.kind === "template",
  );
}

function withMissingRuntimeGuard(
  diagnostics: HostDiagnostic[],
  nodes: HostCompiledVirtualNode[],
): HostDiagnostic[] {
  if (hasRuntimeJsNodes(nodes)) return diagnostics;
  if (diagnostics.some((diagnostic) => diagnostic.severity === "error")) return diagnostics;
  return [...diagnostics, MISSING_RUNTIME_PRODUCT];
}

function requestCompile(
  canonicalId: string,
  request: BrowserHostCompileRequest,
): { ok: true; response: HostCompileRequestResponse } | { ok: false; message: string } {
  try {
    return { ok: true, response: wasmHost!.compileRequest(canonicalId, request) };
  } catch (error) {
    return { ok: false, message: hostRefusalMessage(error) };
  }
}

function ideFromProducts(products: HostCompileRequestResponse["products"]): HostIdeResponse | null {
  const row = products.find((product) => product.kind === "ideCompanion");
  if (!row || !("code" in row)) return null;
  return {
    code: row.code,
    sourceMap: row.sourceMap,
    destructuredBlock: row.destructuredBlock ?? null,
  };
}

function assembleVueRuntime(nodes: HostCompiledVirtualNode[]): {
  assembledJs: string;
  scriptCode: string;
  scriptSourceMap: string;
  templateCode: string;
  templateSourceMap: string;
  styleChunks: string[];
} {
  const script = firstNode(nodes, "script");
  const template = firstNode(nodes, "template");
  const main = firstNode(nodes, "main");
  let assembledJs = "";
  let scriptCode = "";
  let scriptSourceMap = "";
  let templateCode = "";
  let templateSourceMap = "";
  if (script) {
    scriptCode = script.code;
    scriptSourceMap = script.sourceMap ?? "";
    assembledJs += script.code;
  }
  if (template) {
    if (assembledJs) assembledJs += "\n";
    assembledJs += template.code;
    templateCode = template.code;
    templateSourceMap = template.sourceMap ?? "";
  }
  if (!assembledJs && main) {
    assembledJs = main.code;
  }
  return {
    assembledJs,
    scriptCode,
    scriptSourceMap,
    templateCode,
    templateSourceMap,
    styleChunks: styleNodeCodes(nodes),
  };
}

function applyHostAnalysisLintAndPublicApi(
  file: File,
  canonicalId: string,
  disabledRules?: ReadonlySet<string>,
): { lintMs: number | null; tscMs: number | null } {
  let analysis: FileAnalysis | null = null;
  try {
    analysis = wasmHost!.getAnalysis(canonicalId) ?? null;
  } catch {
    // Analysis is optional.
  }
  file.compiled.analysis = analysis;

  let lintMs: number | null = null;
  try {
    const t0 = performance.now();
    file.compiled.lintDiagnostics =
      wasmHost!.lint(canonicalId, buildLintConfig(disabledRules)) ?? [];
    lintMs = performance.now() - t0;
  } catch {
    file.compiled.lintDiagnostics = [];
  }

  let tscMs: number | null = null;
  if (typeof wasmHost!.getPublicApi === "function") {
    const t0 = performance.now();
    applyPublicApiOutputs(file, canonicalId, wasmHost!.getPublicApi.bind(wasmHost!));
    tscMs = performance.now() - t0;
  } else {
    file.compiled.tscCode = "";
    file.compiled.publicApiOutcome = { kind: "absent" };
    file.compiled.declCode = "";
    file.compiled.declSourceMap = "";
    file.compiled.declarationOutcome = { kind: "absent" };
  }
  return { lintMs, tscMs };
}

function configureHost(wasmModule: WasmModule): void {
  const hostCtor = wasmModule.VerterHost;
  if (!hostCtor) {
    wasmHost = null;
    return;
  }
  try {
    const candidate = new hostCtor({
      devMode: true,
      compileErrorPolicy: "devServeLastKnownGood",
      maxProfilesPerFile: 8,
    });
    wasmHost = isHostBinding(candidate) ? candidate : null;
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
 * VDOM render-function pipeline (script + template + style product nodes merged
 * via {@link mergeRenderIntoComponent}, plus an SSR pass). Other frameworks
 * read a single main runtime node and never touch `mergeRenderIntoComponent`.
 */
function usesVueRenderAssembly(framework: ClientFramework): boolean {
  return framework.frameworkId === "vue";
}

/**
 * Compile a framework carrier file through the host, descriptor-driven: the
 * upsert `fileKind` is the framework adapter id. Vue uses the VDOM render
 * assembly; every other framework reads runtimeClient nodes from one typed
 * compileRequest, plus the shared IDE-TSX / public-API / analysis / lint
 * surfaces.
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
  const upsertResult = wasmHost!.upsert({
    inputId: file.filename,
    source: file.code,
    fileKind: framework.frameworkId,
    aliases: [],
  });
  file.structure = wasmHost!.getDocumentStructure?.(file.filename) ?? null;

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(file.filename, upsertResult.moduleReferences, knownFiles);
  }

  const tCompile = performance.now();
  const compiled = requestCompile(
    file.filename,
    vueCompileRequest(clientRuntimeProducts("vue", options), false, options),
  );
  const tsxMs = performance.now() - tCompile;
  if (!compiled.ok) {
    recordCompileRefusal(file, compiled.message, upsertResult.diagnostics);
    return emptyCompileTiming({
      verterNewJs: performance.now() - start,
      parseDurationMs: upsertResult.parseDurationMs ?? null,
    });
  }
  const response = compiled.response;
  const clientNodes = runtimeNodes(response.products, "runtimeClient");
  const assembled = assembleVueRuntime(clientNodes);

  const allDiagnostics = withMissingRuntimeGuard(
    collectUniqueHostDiagnostics([upsertResult.diagnostics, response.diagnostics]),
    clientNodes,
  );
  file.compiled.js = hasRuntimeJsNodes(clientNodes)
    ? mergeRenderIntoComponent(assembled.assembledJs)
    : "";
  file.compiled.css = assembled.styleChunks.join("\n");
  file.compiled.templateCode = assembled.templateCode;
  const templateSection = file.structure?.blocks.find(
    (block) => block.kind === "section" && block.section.role.kind === "templateHost",
  );
  // Combine script + template source maps into a single map covering file.compiled.js.
  // This handles all offsets: SFC prefix lines, host import prepend, mergeRenderIntoComponent.
  file.compiled.verterSourceMap = combineSourceMaps({
    scriptMap: assembled.scriptSourceMap,
    scriptCode: assembled.scriptCode,
    templateMap: assembled.templateSourceMap,
    templateCode: assembled.templateCode,
    vueSource: file.code,
    templateStartUtf8:
      templateSection?.kind === "section" ? templateSection.section.openingRange.start : null,
    finalJs: file.compiled.js,
  });
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;
  applyTsxOutput(file, ideFromProducts(response.products));

  const { lintMs, tscMs } = applyHostAnalysisLintAndPublicApi(file, file.filename, disabledRules);

  if (options?.ssr) {
    try {
      const ssrResponse = wasmHost!.compileRequest(
        file.filename,
        vueCompileRequest([{ runtimeServer: { runtimeSourceMap: true } }], true, options),
      );
      const ssrAssembled = assembleVueRuntime(runtimeNodes(ssrResponse.products, "runtimeServer"));
      file.compiled.ssrCode = mergeRenderIntoComponent(ssrAssembled.assembledJs);
    } catch {
      file.compiled.ssrCode = "// SSR compilation failed";
    }
  } else {
    file.compiled.ssrCode = "";
  }

  return {
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
    tsxMs,
    tscMs,
    lintMs,
  };
}

/**
 * Compile a non-Vue framework carrier through the shared host surfaces: a
 * single main runtimeClient node for the client JS, plus the shared IDE-TSX,
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
  const upsertResult = wasmHost!.upsert({
    inputId: file.filename,
    source: file.code,
    fileKind: framework.frameworkId,
    aliases: [],
  });
  file.structure = wasmHost!.getDocumentStructure?.(file.filename) ?? null;

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(file.filename, upsertResult.moduleReferences, knownFiles);
  }

  const tCompile = performance.now();
  const compiled = requestCompile(
    file.filename,
    frameworkCompileRequest(
      framework,
      clientRuntimeProducts(framework.frameworkId, options),
      false,
      options,
    ),
  );
  const tsxMs = performance.now() - tCompile;
  if (!compiled.ok) {
    recordCompileRefusal(file, compiled.message, upsertResult.diagnostics);
    return emptyCompileTiming({
      verterNewJs: performance.now() - start,
      parseDurationMs: upsertResult.parseDurationMs ?? null,
    });
  }
  const response = compiled.response;
  const clientNodes = runtimeNodes(response.products, "runtimeClient");
  const main = firstNode(clientNodes, "main");
  const styleChunks = styleNodeCodes(clientNodes);

  const allDiagnostics = withMissingRuntimeGuard(
    collectUniqueHostDiagnostics([upsertResult.diagnostics, response.diagnostics]),
    clientNodes,
  );
  file.compiled.js = main?.code ?? "";
  file.compiled.css = styleChunks.join("\n");
  file.compiled.ssrCode = "";
  file.compiled.verterSourceMap = main?.sourceMap ?? "";
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;
  applyTsxOutput(file, ideFromProducts(response.products));

  const { lintMs, tscMs } = applyHostAnalysisLintAndPublicApi(file, file.filename, disabledRules);

  return {
    verterNewJs: performance.now() - start,
    parseDurationMs: upsertResult.parseDurationMs ?? null,
    scriptMs: null,
    templateMs: null,
    styleMs: null,
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

  const upsertResult = wasmHost!.upsert({
    inputId: vueFilename,
    source: sfc,
    fileKind: "vue",
    aliases: [],
  });

  if (knownFiles) {
    syncKnownModuleReferenceDependencies(vueFilename, upsertResult.moduleReferences, knownFiles);
  }

  const compiled = requestCompile(
    vueFilename,
    vueCompileRequest(clientRuntimeProducts("vue", options), false, options),
  );
  if (!compiled.ok) {
    recordCompileRefusal(file, compiled.message, upsertResult.diagnostics);
    return emptyCompileTiming({
      verterNewJs: performance.now() - start,
      parseDurationMs: upsertResult.parseDurationMs ?? null,
    });
  }
  const response = compiled.response;
  const clientNodes = runtimeNodes(response.products, "runtimeClient");
  const script = firstNode(clientNodes, "script");
  const allDiagnostics = withMissingRuntimeGuard(
    collectUniqueHostDiagnostics([upsertResult.diagnostics, response.diagnostics]),
    clientNodes,
  );
  file.compiled.js = script?.code ?? "";
  file.compiled.errors = formatDiagnostics(allDiagnostics);
  file.compiled.compilerDiagnostics = allDiagnostics;
  applyTsxOutput(file, ideFromProducts(response.products));
  applyHostAnalysisLintAndPublicApi(file, vueFilename, disabledRules);

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

  try {
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
  } catch (error) {
    recordUnexpectedCompileFailure(file, error);
    return emptyCompileTiming({ verterNewJs: timing.verterNewJs });
  }
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
