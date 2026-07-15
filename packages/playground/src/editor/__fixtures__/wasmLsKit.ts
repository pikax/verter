/**
 * Shared test kit for the in-context LanguageService guards. Builds the REAL
 * `ts.createLanguageService` over the committed WASM-produced carrier
 * fixtures (`wasm-carriers.json`) — never a live WASM host load — with module
 * resolution falling back to the playground's real `node_modules` (vue types,
 * `@verter/types`, TS libs), so prop types resolve exactly like the browser
 * worker wiring.
 */
import type * as tsNs from "typescript";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { normalizePath } from "@verter/language-shared";
import { InMemoryCarrierStore, type CarrierSurfaces } from "../carrierStore";
import { createInContextLanguageService, fallbackFromSys, type InContextLs } from "../inContextLs";
import fixturesJson from "./wasm-carriers.json";

/** One captured surface in the committed fixture JSON. */
export interface FixtureSurface {
  code: string;
  sourceMap: string | null;
  destructuredBlock: unknown | null;
}

export interface FixtureEntry {
  filename: string;
  source: string;
  ide: FixtureSurface | null;
  ideUnavailable: string | null;
  decl: FixtureSurface | null;
  api: FixtureSurface | null;
}

export const fixtures = fixturesJson as unknown as Record<string, FixtureEntry>;

/** The playground package dir (real, so `node_modules` fallback resolution works). */
export const PLAYGROUND_DIR = normalizePath(
  resolve(dirname(fileURLToPath(import.meta.url)), "../../.."),
);

/** A virtual project root INSIDE the playground dir (never exists on disk). */
export const VROOT = `${PLAYGROUND_DIR}/__virtual__`;

/** Convert a fixture entry's captured surfaces into store-upsert form. */
export function surfacesOf(
  entry: FixtureEntry,
  pick: { ide?: boolean; decl?: boolean; api?: boolean } = { ide: true, decl: true, api: true },
): CarrierSurfaces {
  return {
    ide: pick.ide !== false && entry.ide ? entry.ide : undefined,
    decl: pick.decl !== false && entry.decl ? entry.decl : undefined,
    api: pick.api !== false && entry.api ? entry.api : undefined,
  };
}

export interface TestLs {
  store: InMemoryCarrierStore;
  ls: InContextLs;
  userFiles: Map<string, { content: string; version: number }>;
  /** Absolute virtual path for a bare filename. */
  path(filename: string): string;
}

/**
 * Build a REAL in-context LanguageService over fixture carriers + authored
 * user files, rooted at {@link VROOT}.
 */
export function createFixtureLs(
  ts: typeof tsNs,
  options: {
    user?: Record<string, string>;
    carriers?: Record<string, CarrierSurfaces>;
  },
): TestLs {
  const store = new InMemoryCarrierStore(`${VROOT}/tsconfig.json`);
  for (const [filename, surfaces] of Object.entries(options.carriers ?? {})) {
    store.upsertSource(`${VROOT}/${filename}`, surfaces);
  }
  const userFiles = new Map<string, { content: string; version: number }>();
  for (const [filename, content] of Object.entries(options.user ?? {})) {
    userFiles.set(`${VROOT}/${filename}`, { content, version: 1 });
  }
  const ls = createInContextLanguageService({
    ts,
    store,
    userFiles,
    currentDirectory: VROOT,
    fallbackFs: fallbackFromSys(ts),
  });
  return { store, ls, userFiles, path: (filename: string) => `${VROOT}/${filename}` };
}

/** Flatten diagnostics into `{ code, message }` rows for assertions. */
export function diagRows(
  ts: typeof tsNs,
  diagnostics: readonly tsNs.Diagnostic[],
): Array<{ code: number; message: string }> {
  return diagnostics.map((d) => ({
    code: d.code,
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
  }));
}
