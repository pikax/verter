import { CLIENT_FRAMEWORKS, type ClientFramework } from "@verter/language-shared";
import { PREVIEW_RUNTIME_FRAMEWORK_IDS } from "./previewRuntime";

export { PREVIEW_RUNTIME_FRAMEWORK_IDS } from "./previewRuntime";

/**
 * Playground framework wiring DERIVED from the descriptor-generated client
 * framework manifest (`@verter/language-shared`). This is NOT a second
 * manifest — every map below is computed from `CLIENT_FRAMEWORKS`, the single
 * authority rendered from the Rust framework-adapter registry. No literal
 * Vue+Svelte list drives the playground language UI.
 */

export type { ClientFramework };

/** The registered client frameworks, in manifest order. */
export const FRAMEWORKS: readonly ClientFramework[] = CLIENT_FRAMEWORKS;

/**
 * A language pin: a registered framework id, or `null` for the "Auto" state
 * (auto-detect from the active file extension).
 */
export type LanguagePin = string | null;

/** The fileKind values the WASM FFI accepts for a host upsert. */
export type HostFileKind = string;

/** The plain (non-framework) fallback fileKind. */
export const NON_FRAMEWORK_FILE_KIND = "non_sfc";

/** Look up a framework by id; returns undefined for an unknown id. */
export function frameworkById(id: string | null | undefined): ClientFramework | undefined {
  if (!id) return undefined;
  return FRAMEWORKS.find((f) => f.frameworkId === id);
}

/** Every extension a framework owns: carrier extensions plus adapter-module extensions. */
export function frameworkExtensions(framework: ClientFramework): string[] {
  return [...framework.carrierExtensions, ...framework.adapterModuleExtensions];
}

/**
 * Auto-detect the owning framework id for a filename using LONGEST-SUFFIX
 * matching across BOTH carrier extensions AND adapter-module extensions, so a
 * file like `store.svelte.ts` resolves to `svelte` (suffix `.svelte.ts`) rather
 * than the plain `.ts` of no framework. Returns null when no framework owns the
 * extension.
 */
export function detectFrameworkId(filename: string): string | null {
  const lower = filename.toLowerCase();
  let bestId: string | null = null;
  let bestLen = 0;
  for (const framework of FRAMEWORKS) {
    for (const ext of frameworkExtensions(framework)) {
      const e = ext.toLowerCase();
      if (lower.endsWith(e) && e.length > bestLen) {
        bestLen = e.length;
        bestId = framework.frameworkId;
      }
    }
  }
  return bestId;
}

/**
 * The host `fileKind` for a filename: the owning framework's id when a
 * framework owns the extension (longest-suffix), else the plain non-framework
 * fallback.
 */
export function fileKindForFilename(filename: string): HostFileKind {
  return detectFrameworkId(filename) ?? NON_FRAMEWORK_FILE_KIND;
}

/**
 * A framework's primary carrier extension (e.g. `.vue`, `.svelte`) — the FIRST
 * registered carrier extension, or `""` for a carrier-less adapter.
 */
export function frameworkCarrierExtension(framework: ClientFramework): string {
  return framework.carrierExtensions[0] ?? "";
}

/**
 * The default carrier filename for a framework (e.g. `App.vue`, `App.svelte`).
 * Uses the framework's FIRST carrier extension.
 */
export function frameworkCarrierFilename(framework: ClientFramework, base = "App"): string {
  return `${base}${frameworkCarrierExtension(framework)}`;
}

/**
 * The Monaco language id for a framework's carrier/adapter-module documents:
 * the FIRST client language id the framework attaches to.
 */
export function frameworkClientLanguageId(framework: ClientFramework): string {
  return framework.clientLanguageIds[0] ?? framework.frameworkId;
}

/**
 * Resolve the Monaco editor language id for a filename: a framework's client
 * language id when a framework owns the extension (longest-suffix), else null
 * (the caller falls back to the base TS/JS/CSS/JSON ids or plaintext).
 */
export function frameworkLanguageIdForFilename(filename: string): string | null {
  const id = detectFrameworkId(filename);
  if (!id) return null;
  const framework = frameworkById(id);
  return framework ? frameworkClientLanguageId(framework) : null;
}

/** Every framework carrier extension across all registered frameworks. */
export function carrierExtensions(): string[] {
  return FRAMEWORKS.flatMap((f) => [...f.carrierExtensions]);
}

/**
 * Every framework-owned extension across all registered frameworks: carrier
 * extensions plus adapter-module extensions, de-duplicated. Manifest-derived —
 * the single source for the playground's framework resolve extensions.
 */
export function allFrameworkExtensions(): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const framework of FRAMEWORKS) {
    for (const ext of frameworkExtensions(framework)) {
      if (!seen.has(ext)) {
        seen.add(ext);
        out.push(ext);
      }
    }
  }
  return out;
}

/** Whether a filename is a framework CARRIER (single-file-component) file. */
export function isCarrierFilename(filename: string): boolean {
  const lower = filename.toLowerCase();
  return carrierExtensions().some((ext) => lower.endsWith(ext.toLowerCase()));
}

/** Whether a framework's effective language is considered experimental. */
export function isExperimentalFramework(id: string | null | undefined): boolean {
  // Vue is the reference adapter and production-ready; every other registered
  // adapter is currently experimental in the playground surface.
  return !!id && id !== "vue";
}

/**
 * The framework ids with a browser-side live-preview runtime. This is the
 * playground's PreviewRuntime registry: only frameworks listed here render the
 * iframe preview; everything else shows the "preview not yet supported" state.
 * Future frameworks add their id here once a `PreviewRuntime` consuming the host
 * runtime ESM exists. Keep this list limited to adapters whose runtime is
 * preloaded by the preview iframe and whose public mount protocol is implemented.
 */
/** Whether a framework id has a browser-side live-preview runtime. */
export function supportsRuntimePreview(id: string | null | undefined): boolean {
  return !!id && PREVIEW_RUNTIME_FRAMEWORK_IDS.some((frameworkId) => frameworkId === id);
}

/** A language dropdown option: a framework id, or `null` for the Auto state. */
export interface LanguageOption {
  id: string | null;
  label: string;
  experimental: boolean;
}

/**
 * The language dropdown option set: an explicit Auto state followed by every
 * registered framework in manifest order. This IS the manifest — there is no
 * separate hardcoded option list.
 */
export function languageOptions(): LanguageOption[] {
  return [
    { id: null, label: "Auto", experimental: false },
    ...FRAMEWORKS.map((f) => ({
      id: f.frameworkId,
      label: f.frameworkId,
      experimental: isExperimentalFramework(f.frameworkId),
    })),
  ];
}
