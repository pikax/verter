import type { VerterHost, HostDependencyResolution } from "@verter/native";
import { normalizePath, stripVueVirtualSuffixBackingAware } from "./utils";

interface MacroTypeDep {
  importSource: string;
}

interface AnalysisImport {
  source: string;
}

interface FileAnalysisSnapshot {
  imports?: AnalysisImport[];
  macroTypeDeps?: MacroTypeDep[];
}

export interface MacroTypeDependencyAccess {
  resolveModule(containingFile: string, specifier: string): string | undefined;
  readSource(fileName: string): string | undefined;
  /**
   * Whether a candidate file exists (the TS language-service host's
   * `fileExists`). Used to disambiguate an AMBIGUOUS carrier virtual suffix
   * (`X.svelte.ts`) from a real standalone rune module: a `X.svelte.ts` path is
   * only normalised to `X.svelte` when the backing `X.svelte` carrier exists.
   */
  fileExists(fileName: string): boolean;
}

/**
 * Strip a carrier virtual-file suffix (`*.vue.ts` / `*.vue.d.ts` /
 * `*.svelte.ts` / …) back to the bare carrier path, BACKING-FILE-AWARE so a real
 * standalone rune module (`store.svelte.ts` with no backing `store.svelte`) is
 * NOT corrupted into a phantom component path. Carrier-generic: derived from the
 * manifest naming table, NOT a hardcoded `.vue` suffix list. The strip fires
 * only when the backing carrier source exists, which it always does for an
 * unambiguous Vue virtual (`Foo.vue.ts` ⇒ `Foo.vue`).
 */
function normalizeSourcePath(fileName: string, access: MacroTypeDependencyAccess): string {
  return stripVueVirtualSuffixBackingAware(fileName, (candidate) => access.fileExists(candidate));
}

function isRelativeImport(specifier: string): boolean {
  return specifier.startsWith(".");
}

function parseAnalysis(host: VerterHost, fileName: string): FileAnalysisSnapshot | null {
  const raw = host.getAnalysis(fileName);
  if (!raw) {
    return null;
  }
  try {
    return JSON.parse(raw) as FileAnalysisSnapshot;
  } catch {
    return null;
  }
}

function resolveRelativeImports(
  currentFile: string,
  imports: AnalysisImport[] | undefined,
  access: MacroTypeDependencyAccess,
): HostDependencyResolution[] {
  if (!imports?.length) {
    return [];
  }
  const resolutions: HostDependencyResolution[] = [];
  const seen = new Set<string>();
  for (const entry of imports) {
    if (!isRelativeImport(entry.source) || seen.has(entry.source)) {
      continue;
    }
    seen.add(entry.source);
    const next = access.resolveModule(currentFile, entry.source);
    if (next) {
      resolutions.push({
        specifier: entry.source,
        resolvedCanonicalId: normalizeSourcePath(next, access),
      });
    }
  }
  return resolutions;
}

export function hydrateMacroTypeDependencies(
  host: VerterHost,
  entryFile: string,
  access?: MacroTypeDependencyAccess,
): void {
  if (!access) {
    return;
  }

  const normalizedEntry = normalizeSourcePath(entryFile, access);
  const queue = [normalizedEntry];
  const visited = new Set<string>();

  while (queue.length > 0) {
    const currentFile = queue.shift()!;
    if (visited.has(currentFile)) {
      continue;
    }
    visited.add(currentFile);

    const analysis = parseAnalysis(host, currentFile);
    if (!analysis) {
      continue;
    }

    const resolvedImportDeps = resolveRelativeImports(currentFile, analysis.imports, access);
    if (resolvedImportDeps.length > 0) {
      host.setImportDependencies(currentFile, resolvedImportDeps);
    }

    const traversalSpecifiers =
      currentFile === normalizedEntry
        ? [...new Set((analysis.macroTypeDeps ?? []).map((dep) => dep.importSource))]
        : (analysis.imports ?? [])
            .filter((entry) => isRelativeImport(entry.source))
            .map((entry) => entry.source);

    for (const specifier of traversalSpecifiers) {
      const resolved = access.resolveModule(currentFile, specifier);
      if (!resolved) {
        continue;
      }
      // Backing-file-aware: a real `store.svelte.ts` rune module (no backing
      // `store.svelte`) keeps its own path here, so its source is upserted under
      // the rune path and classified as a rune module — NOT corrupted into a
      // phantom `store.svelte` component carrier.
      const sourcePath = normalizeSourcePath(resolved, access);
      if (visited.has(sourcePath)) {
        continue;
      }

      const nextSource = access.readSource(sourcePath) ?? access.readSource(resolved);
      if (nextSource == null) {
        continue;
      }

      // No `fileKind` hint: the host classifies the carrier from the canonical
      // path (`LanguageRegistry::classify_static`), which resolves `.vue` AND
      // `.svelte` as non-gated carrier rows. A Vue-only `"vue_sfc"` hint forces
      // `FileLanguage::vue()` and would mis-key a `.svelte` dependency (the old
      // `"non_sfc"` branch stripped its carrier outright), so we defer to the
      // host's carrier-generic path classifier — the single language authority.
      host.upsert({
        inputId: sourcePath,
        source: nextSource,
      });
      queue.push(sourcePath);
    }
  }
}
