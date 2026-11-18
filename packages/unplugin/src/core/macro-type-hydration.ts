/**
 * Where the type declarations behind a specifier live.
 *
 * When a Vue SFC uses `defineProps<ExternalType>()` where the type comes from
 * an external package (e.g. `import type { AnimationOptions } from "motion"`),
 * the host needs the `.d.ts` declaration file loaded, not just the runtime `.js`.
 *
 * The workspace's resolver owns every rule of that lookup (declaration
 * companions, package manifests, `exports` maps). This module only lets the
 * bundler's resolver go first, so bundler aliases apply.
 *
 * Loading the files into the host, and the completion tracking that makes
 * concurrent demands wait for each other, live in `pipeline/type-hydration`.
 */
import { resolveThroughBundler, type ResolveHook } from "./bundler-resolve";
import type { TypeFileAccess } from "./pipeline/type-hydration";

export interface MacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

export interface AnalysisImport {
  source: string;
}

export interface AnalysisModuleReference {
  syntax: string; // "StaticImport" | "ExportFrom" | "DynamicImport" | "RequireCall"
  literalSpecifier?: string;
}

export interface AnalysisExportSignature {
  name: string;
  isType: boolean;
  reexportSource?: string;
  reexportLocal?: string;
}

export interface FileAnalysisSnapshot {
  imports?: AnalysisImport[];
  moduleReferences?: AnalysisModuleReference[];
  macroTypeDeps?: MacroTypeDep[];
  exportSignatures?: AnalysisExportSignature[];
}

/**
 * The file that declares the types of `specifier` as imported from `importer`,
 * or `null` when nothing does.
 *
 * The bundler is asked first for the runtime target, so its aliases apply; the
 * workspace then resolves that target (or the specifier itself when the bundler
 * has none) for types. A resolver failure of the bundler is thrown only when
 * the workspace finds nothing either.
 */
export async function typeTargetOf(
  importer: string,
  specifier: string,
  resolve: ResolveHook | null,
  access: TypeFileAccess,
): Promise<string | null> {
  const bundler = resolve ? await resolveThroughBundler(resolve, specifier, importer) : null;
  const target = await access.resolveImport(importer, bundler?.id ?? specifier);
  if (target === null && bundler?.failure) throw bundler.failure.error;
  return target;
}
