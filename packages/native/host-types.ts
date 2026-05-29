// =============================================================================
// Shared Host* interfaces for VerterHost API
//
// These types match the verter_ffi Rust crate's camelCase convention.
// Shared by @verter/native (Node.js) and @verter/wasm (browser).
//
// IMPORTANT: Native-specific overrides (e.g., Buffer support) live in index.ts.
// This file must remain environment-agnostic (no Node.js or browser-only types).
// =============================================================================

/**
 * Caller-requested compile cache mode. `"session"` (the default)
 * consults the fact-validated session cache; `"content"` the pure
 * content-addressed cache; `"stateless"` bypasses both.
 */
export type CompileCacheMode = "stateless" | "content" | "session";

/** Why a requested compile cache mode was constrained. */
export type DowngradeReason =
  | "HasExternalSrc"
  | "HasMacroTypeDeps"
  | "HasWorkspaceAlias"
  | "HasModuleAugmentation"
  | "HasBlockOverride"
  | "HasStyleOverride"
  | "HasIdeOnlyAnalysis"
  | "HasDevLastGood";

export interface HostConfig {
  devMode?: boolean;
  compileErrorPolicy?: "strict" | "strictError" | "devServeLastKnownGood";
  lspScheme?: string;
  maxProfilesPerFile?: number;
  resolveExtensions?: string[];
  /** Controls static analysis level during upsert(). Default: "full". */
  analysisLevel?: "full" | "essential" | "none";
  /**
   * Enable Rust-first native audit for component-meta requests.
   * When true, timing/memory/store data is captured per request.
   * Default: false.
   */
  auditEnabled?: boolean;
  /**
   * Enable per-request semantic footprint capture. Requires
   * `auditEnabled: true`.
   * Default: false.
   */
  footprintCapture?: boolean;
  /**
   * Capacity of the host-owned typeinfo scratch cache used by
   * `evaluateTypeExpressionWithAudit`. `undefined` (default) selects
   * 64 entries; `0` disables the cache; other values cap the LRU at
   * the chosen size — used by the `@verter/typeinfo` LRU eviction
   * tests.
   */
  typeinfoScratchCacheCapacity?: number;
}

export interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  ssr?: boolean;
  hmrStrategy?: "none" | "vite" | "webpack";
  componentId?: string;
  delimiters?: [string, string];
  customElements?: string[];
  comments?: boolean;
  runtimeModuleName?: string;
  typesModuleName?: string;
  forceVapor?: boolean;
  forceJs?: boolean;
  sourceMap?: boolean;
  /** Compilation target preset: "bundler" (default), "ide", or "analysis". */
  target?: "bundler" | "ide" | "analysis";
  /** Requested compile cache mode. Defaults to "session". */
  requestedMode?: CompileCacheMode;
}

export interface HostIdeProjectConfig {
  root: string;
  workspaceRoot: string;
  tsconfigPath?: string;
  providerRoot?: string;
  workspaceAliases?: { find: string; replacement: string }[];
  compilerOptions?: {
    baseUrl?: string;
    paths?: { pattern: string; targets: string[] }[];
  };
  references?: string[];
}

export interface HostIdeResponse {
  code: string;
  sourceMap?: string;
  isJsx: boolean;
}

export interface HostVirtualNodeKind {
  kind: "main" | "script" | "template" | "style" | "custom";
  index?: number;
}

export interface HostSliceChanges {
  scriptChanged: boolean;
  templateChanged: boolean;
  styleIndicesChanged: number[];
  customIndicesChanged: number[];
  structureChanged: boolean;
  descriptorChanged: boolean;
}

export interface HostDiagnostic {
  severity: "error" | "warning" | "info";
  code: string;
  message: string;
  spanStart?: number;
  spanEnd?: number;
}

export interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[];
  hasErrors: boolean;
}

export interface HostExternalSourceRequest {
  ownerCanonicalId: string;
  blockKind: "script" | "template" | "style" | "custom";
  index: number;
  specifier: string;
  resolvedCanonicalId: string;
}

export interface HostScriptImportInfo {
  source: string;
  isTypeOnly: boolean;
  bindings: string[];
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

export interface HostPreprocessorRequest {
  /** Block type: "template", "script", "style", or "custom". */
  blockType: "template" | "script" | "style" | "custom";
  /** Block index (0 for template/script, 0..N for styles/custom blocks). */
  index: number;
  /** The `lang` attribute value (e.g., "pug", "coffee", "scss"). */
  lang: string;
  /** Raw content of the block that needs preprocessing. */
  content: string;
}

export interface HostBlockOverrideEntry {
  /** Block type: "template", "script", "style", or "custom". */
  blockType: "template" | "script" | "style" | "custom";
  /** Block index (0 for template/script, 0..N for styles/custom blocks). */
  index: number;
  /** Preprocessed code. */
  code: string;
  /** Source map from the preprocessor, if available. */
  sourceMap?: string;
}

export interface HostBlockOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: HostBlockOverrideEntry[];
}

export interface HostExportSignature {
  name: string;
  isType: boolean;
  reexportSource?: string;
  reexportLocal?: string;
}

export interface HostResolvedExport {
  name: string;
  isType: boolean;
  sourceCanonicalId?: string;
  sourceName: string;
}

export interface HostUpdateResult {
  canonicalId: string;
  changed: boolean;
  sliceChanges: HostSliceChanges;
  changedVirtualNodes: HostVirtualNodeKind[];
  removedVirtualNodes: HostVirtualNodeKind[];
  changedVirtualIds: string[];
  removedVirtualIds: string[];
  changedLspIds: string[];
  removedLspIds: string[];
  diagnostics: HostDiagnosticsSnapshot;
  externalSourceRequests: HostExternalSourceRequest[];
  importSpecifiers: HostScriptImportInfo[];
  moduleReferences: HostModuleReference[];
  preprocessorRequests: HostPreprocessorRequest[];
  exportSignatures: HostExportSignature[];
  parseDurationMs: number;
}

export interface HostResolvedId {
  canonicalId: string;
  nodeKind: HostVirtualNodeKind;
  existsInHost: boolean;
  bundlerId: string;
  lspId: string;
}

export interface HostVirtualMeta {
  scopeId?: string;
  blockType?: string;
  styleIndex?: number;
  customIndex?: number;
}

export interface HostVirtualFileResponse {
  id: string;
  code: string;
  sourceMap?: string;
  lang?: string;
  stale: boolean;
  diagnostics: HostDiagnosticsSnapshot;
  meta: HostVirtualMeta;
  /**
   * True iff this response was served from a warm cache slot (the
   * fact-validated session slot OR the content-addressed store).
   */
  cacheHit: boolean;
  /** The compile cache mode the caller requested. */
  requestedMode: CompileCacheMode;
  /** The compile cache mode the runtime actually ran under. */
  actualMode: CompileCacheMode;
  /** Highest-priority downgrade reason, or undefined when none fired. */
  downgradeReason?: DowngradeReason;
}

export interface HostUpsertRequest {
  canonicalId?: string;
  inputId: string;
  source: string;
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
  aliases?: string[];
}

export interface HostStyleOverrideEntry {
  index: number;
  code: string;
  sourceMap?: string;
}

export interface HostStyleOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: HostStyleOverrideEntry[];
}

export interface HostVirtualQuery {
  rawId?: string;
  canonicalId?: string;
  nodeKind?: HostVirtualNodeKind;
  compileProfile?: HostCompileProfile;
}

export interface HostRemoveResult {
  canonicalId: string;
}

export interface HostDependencyResolution {
  specifier: string;
  resolvedCanonicalId?: string;
  possibleCanonicalIds?: string[];
}

// =============================================================================
// Code Actions
// =============================================================================

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

// =============================================================================
// Lint Rule Metadata
// =============================================================================

export interface HostLintRuleMetadata {
  name: string;
  category: string;
  defaultSeverity: string;
}

// =============================================================================
// Lint Diagnostics
// =============================================================================

export interface HostLintDiagnostic {
  rule: string;
  category: string;
  severity: string;
  message: string;
  spanStart: number;
  spanEnd: number;
  tags: string[];
  spanKind: string;
}

// =============================================================================
// Document Symbols
// =============================================================================

export interface HostDocumentSymbol {
  name: string;
  detail?: string;
  /** Monaco SymbolKind constant */
  kind: number;
  spanStart: number;
  spanEnd: number;
  selectionStart: number;
  selectionEnd: number;
  children: HostDocumentSymbol[];
}

// =============================================================================
// CSS Selector Matching
// =============================================================================

export interface HostElementMatch {
  tag: string;
  spanStart: number;
  spanEnd: number;
  /** "match", "maybe", or "no" */
  result: "match" | "maybe" | "no";
}

export interface HostSelectorMatchResult {
  selectorText: string;
  selectorStart: number;
  selectorEnd: number;
  matches: HostElementMatch[];
}
