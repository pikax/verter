// Re-export audit helpers so `@verter/wasm` consumers can import
// `whyLoaded`, `whyInstantiated`, `assertLoadedFilesExactly`, etc.
// directly from the package root.
export * from "./audit";

// =============================================================================
// VerterHost types — shared with @verter/native
// =============================================================================

export type {
  CompileCacheMode,
  HostConfig,
  HostIdeResponse,
  HostVirtualNodeKind,
  HostSliceChanges,
  HostDiagnostic,
  HostDiagnosticsSnapshot,
  HostExternalSourceRequest,
  HostScriptImportInfo,
  HostModuleReference,
  HostPreprocessorRequest,
  HostBlockOverrideEntry,
  HostUpdateResult,
  HostResolvedId,
  HostVirtualMeta,
  HostUpsertRequest,
  HostRemoveResult,
  HostTextEdit,
  HostCodeAction,
  HostLintRuleMetadata,
  HostLintDiagnostic,
  HostDocumentSymbol,
  HostElementMatch,
  HostSelectorMatchResult,
} from "@verter/native/host-types";

export type {
  ArtifactBlockToken,
  FrameworkArtifactToken,
  BlockContentOwnerRevisionToken,
  BlockContentBasisToken,
  BlockContentCorrelationToken,
  BlockContentSourceSpaceToken,
  BlockContentArtifactToken,
  BlockContentHashToken,
  HostBlockContentPreCaptureEcho,
  HostBlockContentCapturedEcho,
  HostBlockContentCapturedEchoFields,
  WasmStampedBlockResult,
} from "./handoff-types";

import type {
  HostConfig,
  HostModuleReference,
  HostResolvedId,
  HostUpsertRequest,
  HostUpdateResult,
  HostVirtualNodeKind,
  HostRemoveResult,
  HostCodeAction,
  HostLintRuleMetadata,
  HostLintDiagnostic,
  HostDocumentSymbol,
  HostSelectorMatchResult,
  HostDependencyResolution,
} from "@verter/native/host-types";

export type {
  BrowserHostCompileRequest,
  BrowserHostRequestedProduct,
  BrowserHostSvelteCompileRequest,
  BrowserHostVueCompileRequest,
  HostAnalysisProductOptions,
  HostCompileIdentity,
  HostCompileRequestResponse,
  HostCompiledAnalysisProduct,
  HostCompiledIdeProduct,
  HostCompiledProduct,
  HostCompiledRuntimeProduct,
  HostCompiledVirtualNode,
  HostDestructuredBlockMeta,
  HostIdeProductOptions,
  HostRuntimeProductOptions,
  HostSvelteCompileOptions,
  HostTemplateAnalysisSnapshot,
  HostVueCompileOptions,
} from "./compile-request-types";

import type {
  BrowserHostCompileRequest,
  HostCompileRequestResponse,
} from "./compile-request-types";

// =============================================================================
// WASM binding types
// =============================================================================

type WasmInitFn = () => Promise<unknown>;
type WasmHostResolveFn = (rawId: string) => HostResolvedId | null;
type WasmHostUpsertFn = (request: HostUpsertRequest) => HostUpdateResult;
type WasmHostListVirtualFilesFn = (canonicalId: string) => HostVirtualNodeKind[];
type WasmHostRemoveFn = (canonicalOrAlias: string) => HostRemoveResult | null;
type WasmHostCompileRequestFn = (
  canonicalId: string,
  request: BrowserHostCompileRequest,
) => HostCompileRequestResponse;
type WasmHostGetAnalysisFn = (canonicalOrAlias: string) => unknown | null;
type WasmHostSetImportDependenciesFn = (
  canonicalOrAlias: string,
  resolutions: HostDependencyResolution[],
) => void;
type WasmHostCollectResolvableModuleReferenceSpecifiersFn = (
  moduleReferences: HostModuleReference[],
) => string[];
type WasmHostResolveKnownModuleReferenceDependenciesFn = (
  ownerCanonicalId: string,
  moduleReferences: HostModuleReference[],
  knownIds: string[],
  extensions?: string[],
) => string[];
type WasmHostLintFn = (canonicalOrAlias: string, config?: unknown) => HostLintDiagnostic[];
type WasmHostGetCodeActionsFn = (canonicalOrAlias: string, offset: number) => HostCodeAction[];
type WasmHostGetLintRuleMetadataFn = () => HostLintRuleMetadata[];
type WasmHostGetDocumentSymbolsFn = (canonicalOrAlias: string) => HostDocumentSymbol[];
type WasmHostMatchCssSelectorsFn = (canonicalOrAlias: string) => HostSelectorMatchResult[];
type WasmHostListSymbolsFn = (canonicalId: string) => string;
type WasmHostResolveSymbolWithAuditFn = (
  canonicalId: string,
  name: string,
  typeArgsJson?: string | null,
  mode?: string | null,
) => unknown;
type WasmHostResolveTypeWithAuditFn = (canonicalId: string, declName: string) => unknown;
type WasmHostCommitInputSnapshotFn = (
  files: HostAcquiredInputRow[],
  missing: string[],
) => HostCommittedInputSnapshotReceipt;
type WasmHostObserveInputSnapshotFn = (
  basisId: string,
  canonical: string,
) => HostInputSnapshotObservation;
interface WasmHostBinding {
  resolve: WasmHostResolveFn;
  upsert: WasmHostUpsertFn;
  compileRequest: WasmHostCompileRequestFn;
  listVirtualFiles: WasmHostListVirtualFilesFn;
  remove: WasmHostRemoveFn;
  getAnalysis: WasmHostGetAnalysisFn;
  setImportDependencies: WasmHostSetImportDependenciesFn;
  collectResolvableModuleReferenceSpecifiers: WasmHostCollectResolvableModuleReferenceSpecifiersFn;
  resolveKnownModuleReferenceDependencies: WasmHostResolveKnownModuleReferenceDependenciesFn;
  lint: WasmHostLintFn;
  getCodeActions: WasmHostGetCodeActionsFn;
  getLintRuleMetadata: WasmHostGetLintRuleMetadataFn;
  getDocumentSymbols: WasmHostGetDocumentSymbolsFn;
  matchCssSelectors: WasmHostMatchCssSelectorsFn;
  listSymbols: WasmHostListSymbolsFn;
  resolveSymbolWithAudit: WasmHostResolveSymbolWithAuditFn;
  resolveTypeWithAudit: WasmHostResolveTypeWithAuditFn;
  commitInputSnapshot: WasmHostCommitInputSnapshotFn;
  observeInputSnapshot: WasmHostObserveInputSnapshotFn;
}
type WasmHostCtor = new (config?: HostConfig) => WasmHostBinding;

/**
 * One asynchronously acquired file row: canonical identity plus the
 * captured bytes. Acquisition happens outside any semantic callback;
 * committing the wave is the synchronous handoff.
 */
export interface HostAcquiredInputRow {
  canonical: string;
  content: string;
}

/**
 * Receipt for one committed input snapshot. `basisId` is the handle
 * later observations present; the receipt also binds the execution
 * host (`"browser"` on wasm32) and the semantic-host engine version
 * the snapshot was committed under.
 */
export interface HostCommittedInputSnapshotReceipt {
  basisId: string;
  files: number;
  missing: number;
  executionHost: string;
  engineVersion: string;
}

/**
 * Typed observation over a committed input snapshot.
 *
 * - `status: "file"` — committed bytes in `content`.
 * - `status: "absent"` — the acquisition wave probed this canonical and
 *   recorded it missing (a complete negative).
 * - `status: "needInputs"` — the canonical was never acquired: the
 *   caller must run the next asynchronous acquisition wave. The host
 *   boundary never fetches to fill the gap.
 */
export type HostInputSnapshotObservation =
  | { canonical: string; status: "file"; content: string }
  | { canonical: string; status: "absent" }
  | { canonical: string; status: "needInputs" };

let wasmHostCtor: WasmHostCtor | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

function getOptionalExport<T>(mod: object, key: string): T | null {
  if (Object.prototype.hasOwnProperty.call(mod, key)) {
    return (mod as Record<string, unknown>)[key] as T;
  }
  return null;
}

/**
 * Initialize the WASM module. Must be called before `createHost()`.
 * Safe to call multiple times - will only initialize once.
 */
export async function initialize(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    // Dynamic import to avoid bundler issues
    const wasm = await import("../wasm/verter_wasm.js");
    await (wasm.default as WasmInitFn)();
    wasmHostCtor = getOptionalExport<WasmHostCtor>(wasm, "VerterHost");
    initialized = true;
  })();

  return initPromise;
}

/**
 * Check if the WASM module has been initialized.
 */
export function isInitialized(): boolean {
  return initialized;
}

/**
 * In-memory host facade exposed by the WASM runtime.
 * Requires initialize() (or createHost()) before construction.
 */
export class Host {
  private readonly inner: WasmHostBinding;

  constructor(config?: HostConfig) {
    if (!initialized || !wasmHostCtor) {
      throw new Error("WASM host not initialized. Call initialize() first.");
    }
    this.inner = new wasmHostCtor(config);
  }

  resolve(rawId: string): HostResolvedId | null {
    return this.inner.resolve(rawId);
  }

  upsert(request: HostUpsertRequest): HostUpdateResult {
    return this.inner.upsert(request);
  }

  /**
   * Execute one typed compile request against an already-registered source.
   *
   * The whole transaction is one call: `upsert` the carrier once, source
   * only, then hand this the canonical id and the request. There is no
   * ensure-then-read pair to order correctly and no boolean to interpret.
   *
   * The request is the demand document end to end — its product set is what
   * gets compiled, and no compile profile is built from it on any path. The
   * source is never copied into it.
   *
   * Returns every requested product, in request order, each row tagged with
   * the same `kind` spelling the request used. This route can produce
   * `runtimeClient`, `runtimeServer`, `ideCompanion`, and `analysis`.
   * `publicApi` and `declarations` remain shared-schema arms but are refused
   * for both frameworks.
   *
   * Diagnostic spans and destructured binding source spans are UTF-16
   * offsets into the registered source. Destructured block bounds are
   * UTF-16 offsets into the IDE row's generated `code`; the `analysis` row's
   * own spans are UTF-8 byte offsets into the source, exactly as
   * `getAnalysis()` publishes them.
   *
   * Every call is a COMPLETE compile: this route consults and publishes no
   * compile cache slot, so two identical calls compile twice.
   *
   * Complete-only: a payload the schema refuses, a request the compiler
   * refuses, a framework arm the registered carrier contradicts, an
   * unproducible product, or an execution refusal all THROW the refusal
   * message as a string — never a partial result, a `null`, or a boolean.
   */
  compileRequest(
    canonicalId: string,
    request: BrowserHostCompileRequest,
  ): HostCompileRequestResponse {
    return this.inner.compileRequest(canonicalId, request);
  }

  listVirtualFiles(canonicalId: string): HostVirtualNodeKind[] {
    return this.inner.listVirtualFiles(canonicalId);
  }

  remove(canonicalOrAlias: string): HostRemoveResult | null {
    return this.inner.remove(canonicalOrAlias);
  }

  /**
   * Returns the analysis snapshot for a file as a native JS object, or null
   * if the file doesn't exist. When `analysisLevel` is not "full", computes
   * analysis on demand.
   */
  getAnalysis(canonicalOrAlias: string): unknown | null {
    return this.inner.getAnalysis(canonicalOrAlias);
  }

  /**
   * Sets the resolved import dependencies for a file, enabling
   * cross-file smart invalidation (change tracking).
   */
  setImportDependencies(canonicalOrAlias: string, resolutions: HostDependencyResolution[]): void {
    this.inner.setImportDependencies(canonicalOrAlias, resolutions);
  }

  collectResolvableModuleReferenceSpecifiers(moduleReferences: HostModuleReference[]): string[] {
    return this.inner.collectResolvableModuleReferenceSpecifiers(moduleReferences);
  }

  resolveKnownModuleReferenceDependencies(
    ownerCanonicalId: string,
    moduleReferences: HostModuleReference[],
    knownIds: string[],
    extensions?: string[],
  ): string[] {
    return this.inner.resolveKnownModuleReferenceDependencies(
      ownerCanonicalId,
      moduleReferences,
      knownIds,
      extensions,
    );
  }

  /** Runs lint rules against a file and returns diagnostics with UTF-16 spans. */
  lint(canonicalOrAlias: string, config?: unknown): HostLintDiagnostic[] {
    return this.inner.lint(canonicalOrAlias, config);
  }

  /** Returns code actions (quick fixes) at a given UTF-16 offset. */
  getCodeActions(canonicalOrAlias: string, offset: number): HostCodeAction[] {
    return this.inner.getCodeActions(canonicalOrAlias, offset);
  }

  /** Returns metadata for all registered lint rules. */
  getLintRuleMetadata(): HostLintRuleMetadata[] {
    return this.inner.getLintRuleMetadata();
  }

  /** Returns document symbols for a file (SFC blocks → children). */
  getDocumentSymbols(canonicalOrAlias: string): HostDocumentSymbol[] {
    return this.inner.getDocumentSymbols(canonicalOrAlias);
  }

  /** Matches CSS selectors against template elements (three-valued matrix). */
  matchCssSelectors(canonicalOrAlias: string): HostSelectorMatchResult[] {
    return this.inner.matchCssSelectors(canonicalOrAlias);
  }

  /** Top-level symbol inventory JSON (`FfiSymbolEntry[]`) for a registered file. */
  listSymbols(canonicalId: string): unknown {
    return JSON.parse(this.inner.listSymbols(canonicalId));
  }

  /**
   * Resolve `name` in `canonicalId`'s scope. Returns `{ typeExpr, auditRecord, error }`
   * from the existing session typeinfo substrate — not a second engine.
   */
  resolveSymbolWithAudit(
    canonicalId: string,
    name: string,
    typeArgsJson?: string | null,
    mode?: string | null,
  ): unknown {
    return this.inner.resolveSymbolWithAudit(canonicalId, name, typeArgsJson, mode);
  }

  /** Resolve `declName` in the ordinary module top-level and return the audit record. */
  resolveTypeWithAudit(canonicalId: string, declName: string): unknown {
    return this.inner.resolveTypeWithAudit(canonicalId, declName);
  }

  /**
   * Commit one asynchronously-acquired input wave as an immutable
   * input snapshot.
   *
   * The browser acquires file bytes outside any semantic callback, then
   * hands the acquired rows (`files`) and probed-missing canonicals
   * (`missing`) here in one synchronous call. Probed-missing keys are
   * committed as explicit negatives, so observations answer `absent` —
   * a complete negative distinct from `needInputs`.
   *
   * This route writes no source and compiles nothing — registering
   * acquired bytes for compilation stays on `upsert()`. An incoherent
   * wave (one canonical with two contents, or both acquired and
   * probed-missing) throws and stores nothing.
   */
  commitInputSnapshot(
    files: HostAcquiredInputRow[],
    missing: string[],
  ): HostCommittedInputSnapshotReceipt {
    return this.inner.commitInputSnapshot(files, missing);
  }

  /**
   * Observe one canonical under a committed input snapshot.
   *
   * Returns the typed observation: `"file"` (committed bytes),
   * `"absent"` (probed missing by the acquiring wave), or
   * `"needInputs"` (never acquired — the caller must run the next
   * asynchronous acquisition wave; this boundary never fetches to fill
   * the gap). An unknown basis id throws.
   */
  observeInputSnapshot(basisId: string, canonical: string): HostInputSnapshotObservation {
    return this.inner.observeInputSnapshot(basisId, canonical);
  }
}

export async function createHost(config?: HostConfig): Promise<Host> {
  await initialize();
  return new Host(config);
}

export { Host as VerterHost };
