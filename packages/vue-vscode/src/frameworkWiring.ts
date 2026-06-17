/**
 * Manifest-driven client framework wiring.
 *
 * The single authority for the extension's framework wiring — the document
 * selector, the framework-carrier document predicate, and the built-in
 * TypeScript-plugin configure trigger — is the descriptor-generated client
 * framework manifest in `@verter/language-shared`
 * (`client-framework-manifest.generated.ts`, byte-pinned by the Rust
 * `client_framework_manifest_ts_freshness` guard).
 *
 * Adding a framework is a descriptor + regen change with NO per-framework edit
 * here: every registered adapter is active (Svelte is NOT opt-in-gated), and
 * the wiring iterates the manifest rather than branching on a framework id.
 *
 * File-watching is intentionally NOT a client concern: the LSP server registers
 * the `workspace/didChangeWatchedFiles` watcher (from the same descriptor
 * authority), so the manifest exposes no client watch-glob surface.
 */

import {
  BASE_TYPESCRIPT_LANGUAGE_IDS,
  CLIENT_DOCUMENT_SELECTOR_LANGUAGE_IDS,
  CLIENT_FRAMEWORK_LANGUAGE_IDS,
} from "@verter/language-shared";

/**
 * A file-scheme language document filter — a structural match for the
 * `vscode-languageclient` `DocumentFilter` (scheme + language both present) so
 * the returned array is directly assignable to a `DocumentSelector` while this
 * module stays `vscode`-free and unit-testable without the VS Code runtime.
 */
export interface FileLanguageFilter {
  scheme: string;
  language: string;
}

/** Every framework client language id the manifest declares (e.g. vue, svelte). */
export function frameworkClientLanguageIds(): string[] {
  return [...CLIENT_FRAMEWORK_LANGUAGE_IDS];
}

/** The set of framework CARRIER client language ids, for fast membership tests. */
const FRAMEWORK_CARRIER_LANGUAGE_IDS = new Set(CLIENT_FRAMEWORK_LANGUAGE_IDS);

/**
 * Whether a document's language id is a framework CARRIER the LSP attaches to
 * (e.g. `"vue"`, `"svelte"`). Drives the "start the language server for this
 * document" decision — manifest-derived, not a hardcoded `=== "vue"` check.
 */
export function isFrameworkCarrierLanguageId(languageId: string | undefined): boolean {
  return languageId !== undefined && FRAMEWORK_CARRIER_LANGUAGE_IDS.has(languageId);
}

/** The set of language ids the built-in TypeScript plugin is configured for. */
const TYPESCRIPT_PLUGIN_TRIGGER_LANGUAGE_IDS = new Set(BASE_TYPESCRIPT_LANGUAGE_IDS);

/**
 * Whether opening a document with this language id should configure the
 * built-in VS Code TypeScript-server plugin (`_typescript.configurePlugin`).
 * That plugin operates on the TS/JS surface, so the trigger set is the
 * manifest's `BASE_TYPESCRIPT_LANGUAGE_IDS` (the TS/JS language ids).
 */
export function shouldConfigureTypeScriptPluginForLanguageId(
  languageId: string | undefined,
): boolean {
  return languageId !== undefined && TYPESCRIPT_PLUGIN_TRIGGER_LANGUAGE_IDS.has(languageId);
}

/**
 * The LSP document selector — one `{ scheme: "file", language }` filter per
 * document-selector language id the manifest declares (the plain js/ts base
 * plus every registered framework's client language ids; NO React dialects —
 * those are activation/plugin-configure surfaces only, preserving the
 * pre-manifest Vue selector surface). The caller appends any extra non-framework
 * schemes (e.g. the virtual-file content scheme).
 */
export function frameworkDocumentSelector(): FileLanguageFilter[] {
  return CLIENT_DOCUMENT_SELECTOR_LANGUAGE_IDS.map((language) => ({ scheme: "file", language }));
}
