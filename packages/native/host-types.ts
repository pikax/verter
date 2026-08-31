// =============================================================================
// Shared Host* interfaces for VerterHost API
//
// These types match the verter_ffi Rust crate's camelCase convention.
// Shared by @verter/native (Node.js) and @verter/wasm (browser).
//
// IMPORTANT: Native-specific overrides (e.g., Buffer support) live in index.ts.
// This file must remain environment-agnostic (no Node.js or browser-only types).
// =============================================================================

declare const blockContentOpaqueBrand: unique symbol;

/** Opaque string issued or validated by the native-content handoff boundary. */
type BlockContentOpaque<Name extends string> = string & {
  readonly [blockContentOpaqueBrand]: Name;
};

export type ArtifactBlockToken = BlockContentOpaque<"ArtifactBlockToken">;
export type FrameworkArtifactToken = BlockContentOpaque<"FrameworkArtifactToken">;
export type BlockContentOwnerRevisionToken = BlockContentOpaque<"BlockContentOwnerRevisionToken">;
export type BlockContentBasisToken = BlockContentOpaque<"BlockContentBasisToken">;
export type BlockContentCorrelationToken = BlockContentOpaque<"BlockContentCorrelationToken">;
export type BlockContentSourceSpaceToken = BlockContentOpaque<"BlockContentSourceSpaceToken">;
export type BlockContentArtifactToken = BlockContentOpaque<"BlockContentArtifactToken">;
export type BlockContentHashToken = BlockContentOpaque<"BlockContentHashToken">;

/**
 * Exact captured-echo fields carried by each flattened JS wire entry.
 *
 * The canonical ID lives on the surrounding update/apply request. All other
 * captured fields are copied unchanged between `HostPreprocessorRequest` and
 * `HostBlockOverrideEntry`.
 */
export interface HostBlockContentCapturedEchoFields {
  readonly correlationToken: BlockContentCorrelationToken;
  readonly blockToken: ArtifactBlockToken;
  readonly ownerRevision: BlockContentOwnerRevisionToken;
  readonly artifactToken: FrameworkArtifactToken;
  readonly expectedLanguage: string;
  readonly priorBasisToken?: BlockContentBasisToken;
  readonly basisToken: BlockContentBasisToken;
}

/** Logical pre-capture echo before its basis is minted. */
export type HostBlockContentPreCaptureEcho = Readonly<
  Omit<HostBlockContentCapturedEchoFields, "basisToken"> & { canonicalId: string }
>;

/** Logical, unflattened form of the host-captured request echo. */
export interface HostBlockContentCapturedEcho {
  readonly request: HostBlockContentPreCaptureEcho;
  readonly basisToken: BlockContentBasisToken;
}

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
  /**
   * Worker count for the host-owned CPU pool used by `compileMany`'s
   * outer coordinator. `undefined` (default) resolves to the platform's
   * available parallelism at host-construction time; `0` is treated as
   * `undefined` (the default), so a misconfigured caller passing `0`
   * still gets a working host pool; other positive values cap the
   * pool's worker count.
   *
   * The host pool is built once at host construction and reused across
   * `compileMany` calls. To change the pool size, construct a new host.
   */
  hostCpuThreads?: number;
  /**
   * Enable host performance-metrics collection. `undefined` (default)
   * keeps the default `false` (counters stay zero; `getMetrics()`
   * returns `null`). A runtime per-host construction choice — not a
   * build-time feature.
   * Default: false.
   */
  metricsEnabled?: boolean;
}

export interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  /** Vue custom-element script policy; unrelated to template `customElements`. */
  customElement?: boolean;
  ssr?: boolean;
  /**
   * SSR asset-collection module id registered on `ssrContext.modules`.
   * Vite's ssr-manifest keys are ROOT-RELATIVE — pass
   * `normalizePath(relative(root, filename))`; absent falls back to the
   * canonical id.
   */
  ssrModuleId?: string;
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
  /**
   * Inline the render function inside `setup()` (Vue production topology,
   * official `compileScript({ inlineTemplate: true })`). Absent resolves to
   * `isProduction` (official default: inline in prod builds). VDOM client
   * only; Vapor inline and inline SSR fall back to non-inline.
   */
  inline?: boolean;
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
  spanStart: number;
  spanEnd: number;
}

export interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[];
  hasErrors: boolean;
}

export interface HostExternalSourceRequest {
  ownerCanonicalId: string;
  blockKind: "script" | "template" | "style" | "custom";
  specifier: string;
  resolvedCanonicalId: string;
  blockToken: ArtifactBlockToken;
  ownerRevision: BlockContentOwnerRevisionToken;
  artifactToken: FrameworkArtifactToken;
  carrierSourceSpaceToken: BlockContentSourceSpaceToken;
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

export interface HostPreprocessorRequest extends HostBlockContentCapturedEchoFields {
  contentClass: "template" | "script" | "style" | "custom";
  /** The `lang` attribute value (e.g., "pug", "coffee", "scss"). */
  lang: string;
  /** Raw content of the block that needs preprocessing. */
  content: string;
  availability:
    | "nativeAvailable"
    | "processedContentRequired"
    | "suppliedAvailable"
    | "missing"
    | "conflict"
    | "stale";
  sourceSpaceToken: BlockContentSourceSpaceToken;
  contentHash: BlockContentHashToken;
  customType?: string;
}

export interface HostBlockOverrideEntry extends HostBlockContentCapturedEchoFields {
  sourceSpaceToken: BlockContentSourceSpaceToken;
  /** Preprocessed code. */
  code: string;
  codeHash: BlockContentHashToken;
  /** Source map from the preprocessor, if available. */
  sourceMap?: string;
  sourceMapHash?: BlockContentHashToken;
  suppliedProvenance?: string;
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
  fileKind?: "vue" | "sfc" | "vue_sfc" | "svelte" | "non_sfc" | "text" | "file";
  aliases?: string[];
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

// =============================================================================
// Host compile request
// =============================================================================
//
// The tag-discriminated request the native host compile adapter decodes.
// Every slot mirrors the Rust wire schema, so the rules below are the
// decoder's rules, not conventions:
//
// - Every object is closed. A key outside its declared set is refused at
//   decode, including a key that belongs to the other framework's arm or
//   to another product.
// - Every non-optional field is required. An absent key is a refusal, not
//   a substituted value.
// - An optional field may be omitted or set to `undefined`; both read as
//   absent, and what absent MEANS is decided by the compiler, not here.
// - Every string union is closed. A spelling outside it is refused at
//   decode.
//
// One slot is deliberately TIGHTER than the wire: `delimiters` is typed
// as a two-element tuple where the wire carries a string list, so a wrong
// arity is a type error at the call site instead of a refusal from the
// compiler. Every other slot is the wire's own type.
//
// Nothing in this shape decides whether a compile is legal: the product
// set, the backend/product pairing and option support are the compiler's
// own rules, reported as its own refusals after the request is accepted.

/** Which Vue client codegen backend a runtime product resolves to.
 * `"inferred"` defers to the source's own marker. */
export type HostVueBackend = "inferred" | "vdom" | "vapor";

export type HostVueWhitespace = "preserve" | "condense";

export type HostVueParsePad = "space" | "line" | "off";

export type HostVueCssModuleScopeBehaviour = "local" | "global";

export type HostVueCssModuleLocalsConvention =
  | "camelCase"
  | "camelCaseOnly"
  | "dashes"
  | "dashesOnly"
  | "asIs";

export interface HostVueAssetUrlOptions {
  base?: string;
  includeAbsolute?: boolean;
  /** Tag name to the attributes rewritten on it. Required, possibly empty. */
  tags: Record<string, string[]>;
}

/** Asset-URL rewriting: off, or on with its options. */
export type HostVueAssetUrlTransform = "disabled" | { enabled: HostVueAssetUrlOptions };

export interface HostVueCssModules {
  scopeBehaviour?: HostVueCssModuleScopeBehaviour;
  hashPrefix?: string;
  localsConvention?: HostVueCssModuleLocalsConvention;
  exportGlobals?: boolean;
}

/**
 * Vue-owned compile options.
 *
 * The `compatConfig*`, `transformCompatConfig` and `codegenMode` slots
 * exist so that a caller who supplies one is told which option is refused;
 * supplying any of them — `false` included — is refused by the compiler on
 * presence.
 */
export interface HostVueCompileOptions {
  backend: HostVueBackend;
  /**
   * Mirrors the Vue `ssr` option. It is not the SSR demand: whether a
   * compile produces server output follows from the requested products.
   */
  ssr: boolean;
  isCustomElement: string[];
  babelParserPlugins: string[];
  /** Exactly two delimiters; any other arity is refused. */
  delimiters?: [string, string];
  whitespace?: HostVueWhitespace;
  comments?: boolean;
  hoistStatic?: boolean;
  cacheHandlers?: boolean;
  hmr?: boolean;
  optimizeImports?: boolean;
  runtimeModuleName?: string;
  ssrRuntimeModuleName?: string;
  parsePad?: HostVueParsePad;
  ignoreEmpty?: boolean;
  genDefaultAs?: string;
  propsDestructure?: boolean;
  scriptCustomElement?: boolean;
  transformAssetUrls?: HostVueAssetUrlTransform;
  styleTrim?: boolean;
  cssModules?: HostVueCssModules;

  compatConfig?: boolean;
  compatConfigMode?: boolean;
  compatConfigCompilerIsOnElement?: boolean;
  compatConfigCompilerVBindSync?: boolean;
  compatConfigCompilerVIfVForPrecedence?: boolean;
  compatConfigCompilerVBindObjectOrder?: boolean;
  compatConfigCompilerVOnNative?: boolean;
  compatConfigCompilerNativeTemplate?: boolean;
  compatConfigCompilerInlineTemplate?: boolean;
  compatConfigCompilerFilters?: boolean;
  transformCompatConfig?: boolean;
  codegenMode?: boolean;
}

export type HostSvelteNamespace = "html" | "svg" | "mathMl" | "foreign";

export type HostSvelteFragments = "html" | "tree";

export type HostSvelteRunes = "true" | "false" | "infer";

export type HostSvelteCss = "injected" | "external";

export type HostSvelteCustomElementPropType =
  | "string"
  | "boolean"
  | "number"
  | "array"
  | "object";

export interface HostSvelteCustomElementProp {
  attribute?: string;
  reflect?: boolean;
  propType?: HostSvelteCustomElementPropType;
}

export interface HostSvelteCustomElementDescriptor {
  tag?: string;
  shadow?: boolean;
  /** Exported prop name to its custom-element descriptor. Required, possibly empty. */
  props: Record<string, HostSvelteCustomElementProp>;
}

/**
 * Presence-only marker for the Svelte `compatibility` object. Its one
 * inventoried field (`componentApi`) is refused, so it has no slot here
 * and the object carries nothing.
 */
export type HostSvelteCompatibility = Record<string, never>;

/**
 * Svelte-owned compile options. `generateModule` and `experimentalAsync`
 * are well-formed options whose capability is refused; the trailing slots
 * are refused unconditionally, on presence.
 */
export interface HostSvelteCompileOptions {
  dev?: boolean;
  generateModule?: boolean;
  experimentalAsync?: boolean;
  customElement?: boolean;
  customElementDescriptor?: HostSvelteCustomElementDescriptor;
  namespace?: HostSvelteNamespace;
  css?: HostSvelteCss;
  preserveComments?: boolean;
  preserveWhitespace?: boolean;
  fragments?: HostSvelteFragments;
  runes?: HostSvelteRunes;
  discloseVersion?: boolean;
  compatibility?: HostSvelteCompatibility;

  loose?: boolean;
  accessors?: boolean;
  immutable?: boolean;
  compatibilityComponentApi?: boolean;
  hmr?: boolean;
  customElementExtend?: boolean;
}

/** Source identity and dev/prod profile shared by every product of one compile. */
export interface HostCompileIdentity {
  filename?: string;
  componentId?: string;
  isProduction: boolean;
  forceJs: boolean;
}

export interface HostRuntimeProductOptions {
  /** Absent resolves to the request's own `isProduction`. */
  inline?: boolean;
  runtimeSourceMap: boolean;
}

export interface HostIdeProductOptions {
  wantSourceMap: boolean;
  embedAmbientTypes: boolean;
  conditionalRootNarrowing: boolean;
  strictSlots: boolean;
  typesModuleName?: string;
  ideChunkBoundaries: boolean;
}

export interface HostAnalysisProductOptions {
  wantScriptBindings: boolean;
  wantTemplateData: boolean;
}

export type HostRuntimeClientProduct = { kind: "runtimeClient" } & HostRuntimeProductOptions;
export type HostRuntimeServerProduct = { kind: "runtimeServer" } & HostRuntimeProductOptions;
export type HostIdeCompanionProduct = { kind: "ideCompanion" } & HostIdeProductOptions;
export type HostPublicApiProduct = { kind: "publicApi" };
export type HostDeclarationsProduct = { kind: "declarations" };
export type HostAnalysisProduct = { kind: "analysis" } & HostAnalysisProductOptions;

/**
 * One requested compiler product. The product set is the demand document:
 * there is no target preset that expands into a bundle of products, and
 * request order is preserved.
 *
 * `publicApi` and `declarations` carry no options — their outputs are
 * shaped by host-resolved profile identities the caller never supplies —
 * so they take no slot beyond `kind`.
 */
export type HostRequestedProduct =
  | HostRuntimeClientProduct
  | HostRuntimeServerProduct
  | HostIdeCompanionProduct
  | HostPublicApiProduct
  | HostDeclarationsProduct
  | HostAnalysisProduct;

export interface HostVueCompileRequest {
  framework: "vue";
  identity: HostCompileIdentity;
  products: HostRequestedProduct[];
  options: HostVueCompileOptions;
}

export interface HostSvelteCompileRequest {
  framework: "svelte";
  identity: HostCompileIdentity;
  products: HostRequestedProduct[];
  options: HostSvelteCompileOptions;
}

/**
 * A host compile request discriminated by framework, so framework-owned
 * options are unreachable from the other framework's arm.
 */
export type HostCompileRequest = HostVueCompileRequest | HostSvelteCompileRequest;
