// =============================================================================
// Shared Host* interfaces for VerterHost API
//
// These types match the verter_ffi Rust crate's camelCase convention.
// Shared by @verter/native (Node.js) and @verter/wasm (browser).
//
// IMPORTANT: Native-specific overrides (e.g., Buffer support) live in index.ts.
// This file must remain environment-agnostic (no Node.js or browser-only types).
// =============================================================================

export interface HostConfig {
  devMode?: boolean;
  compileErrorPolicy?: "strict" | "strictError" | "devServeLastKnownGood";
  lspScheme?: string;
  maxProfilesPerFile?: number;
  resolveExtensions?: string[];
  /** Controls static analysis level during upsert(). Default: "full". */
  analysisLevel?: "full" | "essential" | "none";
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
  forceVapor?: boolean;
  forceJs?: boolean;
  sourceMap?: boolean;
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
