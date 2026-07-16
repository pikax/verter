import { VIRTUAL_FILE_NAMING, type VirtualPathPolicy } from "../virtual-file-naming.generated";
import { normalizePath } from "./naming";

// The carrier script-kind / root-membership POLICY — the single shared answer
// to "what ScriptKind does a generated carrier file get" and "how does it
// enter the program", consumed by both the Node tsserver plugin host hooks and
// the WASM in-context LanguageService host. BROWSER-SAFE and framework-neutral:
// every rule derives from the generated `virtual-file-naming.generated.ts`
// descriptor-column mirror (never a hardcoded framework literal), and the TS
// enum values are INJECTED (this package never imports the `typescript`
// package at module scope).

/**
 * The `ts.ScriptKind` members the policy needs, INJECTED by the host (pass the
 * real `ts` module or just its `ScriptKind` values). Generic over the member
 * type so the host's own enum type flows through unchanged.
 */
export interface CarrierScriptKindEnum<T> {
  readonly TS: T;
  readonly TSX: T;
  readonly JS: T;
  readonly JSX: T;
}

/** The injected TS facade shape (`{ ScriptKind }` — the `typescript` module satisfies it). */
export interface CarrierTsFacade<T> {
  readonly ScriptKind: CarrierScriptKindEnum<T>;
}

/**
 * How a generated carrier file participates in the program:
 * - `selfDiagnosticRoot` — the IDE carrier (`Comp.<ext>.tsx`): a root in its
 *   own right; the host diagnoses it directly.
 * - `importDriven` — the declaration carrier (`Comp.d.<ext>.ts`): enters the
 *   program only when an import resolves to it; NOT a root.
 * - `redirectReached` — the API carrier (`Comp.<ext>.verter.ts`): reached via
 *   the module-resolution redirect, never a root.
 */
export type CarrierRootMembership = "selfDiagnosticRoot" | "importDriven" | "redirectReached";

/** The script language a carrier virtual-file suffix implies. */
type CarrierScriptLanguage = "ts" | "tsx" | "js" | "jsx";

interface CarrierVirtualRule {
  /** The full path suffix (`{carrierExt}{virtualSuffix}` or `.d{carrierExt}.ts`). */
  readonly suffix: string;
  readonly language: CarrierScriptLanguage;
  readonly membership: CarrierRootMembership;
}

/** The script language implied by a virtual suffix's trailing extension. */
function languageForSuffix(suffix: string): CarrierScriptLanguage | null {
  if (suffix.endsWith(".tsx")) return "tsx";
  if (suffix.endsWith(".jsx")) return "jsx";
  if (suffix.endsWith(".ts")) return "ts";
  if (suffix.endsWith(".js")) return "js";
  return null;
}

/** The IDE-column suffix variants a policy projects (both jsxConditional branches). */
function ideSuffixesFor(policy: VirtualPathPolicy): string[] {
  switch (policy.kind) {
    case "suffix":
      return [policy.suffix];
    case "jsxConditional":
      return [policy.nonJsx, policy.jsx];
    case "selfFile":
    case "none":
      return [];
  }
}

/**
 * The classification table, built once from the generated descriptor column.
 * A `selfFile` row (a real standalone rune module — `store.svelte.ts`)
 * contributes NOTHING: it is a user file, not a generated carrier virtual.
 * Longest-suffix-first so a more specific suffix always wins.
 */
const CARRIER_VIRTUAL_RULES: readonly CarrierVirtualRule[] = Object.values(VIRTUAL_FILE_NAMING)
  .flatMap((row): CarrierVirtualRule[] => {
    const ext = row.carrierExtension;
    if (ext === null) {
      return [];
    }
    const rules: CarrierVirtualRule[] = [];
    // IDE carrier (`Comp.vue.tsx` / `Comp.vue.jsx` / `Comp.svelte.tsx` /
    // `Comp.svelte.jsx`) —
    // the self-diagnostic root; TSX or JSX per the source-language branch.
    for (const ideSuffix of ideSuffixesFor(row.ide)) {
      const language = languageForSuffix(ideSuffix);
      if (language !== null) {
        rules.push({ suffix: `${ext}${ideSuffix}`, language, membership: "selfDiagnosticRoot" });
      }
    }
    // Declaration carrier — extension-MIDDLE `{stem}.d.{ext}.ts` — a TS
    // declaration reached import-driven (tsgo's basename-append probe).
    if (row.declarationSurface.kind === "extensionMiddleTs") {
      rules.push({ suffix: `.d${ext}.ts`, language: "ts", membership: "importDriven" });
    }
    // API carrier (`Comp.vue.verter.ts`) — the redirect-reached import surface.
    if (row.importSurface.kind === "suffix") {
      const language = languageForSuffix(row.importSurface.suffix) ?? "ts";
      rules.push({
        suffix: `${ext}${row.importSurface.suffix}`,
        language,
        membership: "redirectReached",
      });
    }
    return rules;
  })
  .sort((a, b) => b.suffix.length - a.suffix.length);

/** The matching classification rule for a path, or `null` for a non-carrier path. */
function carrierVirtualRuleFor(fileName: string): CarrierVirtualRule | null {
  const normalized = normalizePath(fileName);
  for (const rule of CARRIER_VIRTUAL_RULES) {
    if (normalized.endsWith(rule.suffix)) {
      return rule;
    }
  }
  return null;
}

/**
 * The `ScriptKind` a generated carrier virtual file is served with:
 * IDE carrier (`.<ext>.tsx`) — TSX (`.jsx` branch — JSX); declaration carrier
 * (`.d.<ext>.ts`) — TS; API carrier (`.<ext>.verter.ts`) — TS. Returns
 * `undefined` for any non-carrier path (including a REAL rune module like
 * `store.svelte.ts` and a bare carrier source `Comp.vue`) — the host falls
 * through to its own classification.
 */
export function scriptKindForCarrier<T>(fileName: string, ts: CarrierTsFacade<T>): T | undefined {
  const rule = carrierVirtualRuleFor(fileName);
  if (rule === null) {
    return undefined;
  }
  switch (rule.language) {
    case "ts":
      return ts.ScriptKind.TS;
    case "tsx":
      return ts.ScriptKind.TSX;
    case "js":
      return ts.ScriptKind.JS;
    case "jsx":
      return ts.ScriptKind.JSX;
  }
}

/**
 * How a generated carrier virtual file participates in the program (see
 * [`CarrierRootMembership`]). Returns `null` for a non-carrier path.
 */
export function carrierRootMembership(fileName: string): CarrierRootMembership | null {
  return carrierVirtualRuleFor(fileName)?.membership ?? null;
}
