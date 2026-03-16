import type { VerterHost, HostDependencyResolution } from "@verter/native";

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
}

function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

function normalizeSourcePath(fileName: string): string {
  const normalized = normalizePath(fileName);
  if (normalized.endsWith(".vue.d.ts")) {
    return normalized.slice(0, -5);
  }
  if (normalized.endsWith(".vue.ts")) {
    return normalized.slice(0, -3);
  }
  return normalized;
}

function isRelativeImport(specifier: string): boolean {
  return specifier.startsWith(".");
}

function isVueFile(fileName: string): boolean {
  return normalizePath(fileName).endsWith(".vue");
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
        resolvedCanonicalId: normalizeSourcePath(next),
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

  const normalizedEntry = normalizeSourcePath(entryFile);
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
      const sourcePath = normalizeSourcePath(resolved);
      if (visited.has(sourcePath)) {
        continue;
      }

      const nextSource = access.readSource(sourcePath) ?? access.readSource(resolved);
      if (nextSource == null) {
        continue;
      }

      host.upsert({
        inputId: sourcePath,
        source: nextSource,
        fileKind: isVueFile(sourcePath) ? "vue_sfc" : "non_sfc",
      });
      queue.push(sourcePath);
    }
  }
}
