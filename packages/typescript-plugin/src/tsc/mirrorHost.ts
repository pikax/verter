// The TS<7 whole-project batch typecheck driver — the `vue-tsc`/`svelte-check`
// equivalent for the npm `typescript@<7` package.
//
// ## Why a separate backend
//
// A tsserver language-service plugin does NOT load for the `tsc` command line
// (LS plugins affect interactive editing only — the `@verter/typescript-plugin`
// `create()` hook never runs under `tsc`). So whole-project batch typecheck needs
// its OWN backend: a `ts.CompilerHost`/tsc-API driver. This module is that driver.
// `index.ts` (the interactive plugin) NEVER imports it — the two surfaces are
// independent.
//
// ## The mirror-root model (zero working-tree writes)
//
// Generated framework carriers NEVER touch the user's working tree. For
// `src/Foo.vue`, the IDE carrier is materialised into a Verter-owned MIRROR root
// (`<mirror>/<src-relative-to-mirror-base>/Foo.vue.tsx`), NOT next to the source.
// The driver's `ts.CompilerHost` serves the mirror carriers (and the generated
// `extends` tsconfigs, and any emit) and FALLS THROUGH to the real user tree for
// everything else. The user's checkout is read-only as far as the batch is
// concerned. The mirror root is a fresh OS-temp directory (`fs.mkdtemp` under
// `os.tmpdir()`), never under the user's project root.
//
// The mirror BASE is the common ancestor of every carrier source and the project
// configs, so the mirror reproduces the user tree's layout one-to-one (no source
// path escapes the mirror via a `..` segment, and `rootDir` re-roots cleanly).
//
// ## `.x` → companion module redirection
//
// `import C from "./C.vue"` is redirected to the carrier via the host
// `resolveModuleNames`/`resolveModuleNameLiterals` OVERRIDE — resolver-version
// independent, NOT reliant on stock `tsc`'s appended-extension probing.
// `paths`/`baseUrl` do NOT solve this (they apply only to bare specifiers, never
// a relative `import "./C.vue"`). TypeScript's own resolver runs FIRST; the
// override supplies a redirect only when resolution reaches a framework-carrier
// specifier. All other resolution (`./bar`, `../lib/x`, bare `node_modules`,
// `paths`) is re-rooted into the user tree and is byte-for-byte the user's, with
// zero working-tree mutation.
//
// ## Emit mode is reference-dependent
//
// - No project references → `noEmit: true` diagnostics-only mode (nothing
//   written).
// - With project references → BUILD MODE as EMIT-THEN-CHECK: each referenced
//   project is compiled `emitDeclarationOnly` (its `composite` declaration
//   boundary) so its real `.d.ts` (from the `CarrierApi` `.verter.ts`
//   declaration carrier) is written into the mirror; the leaf is then checked
//   `noEmit` and consumes each referenced project through that emitted `.d.ts` —
//   the way the user's own `tsc -b` does. A referenced project gets its OWN
//   generated `extends`-tsconfig in its mirror subtree with mirror-local
//   `outDir`/`tsBuildInfoFile`, so all emit lands in the MIRROR tree, NEVER the
//   user's working tree. (Emit-then-check is used instead of a
//   `ts.createSolutionBuilder` driver, whose per-project Programs do not honour
//   the host `resolveModuleNames` override the carrier redirect depends on.)
//
// ## Diagnostics
//
// Per file: syntactic + semantic (`getSyntacticDiagnostics` +
// `getSemanticDiagnostics`), plus program-level global/options/config
// diagnostics, all mapped back to the `.vue`/`.svelte` through the carrier source
// map. Generated-only spans (a span that does not map to any source) are
// SUPPRESSED. Diagnostics naming a generated mirror artifact (the generated
// tsconfig, an injected companion) are STRIPPED (the batch promises the user's
// own config/options diagnostic set). A `.vue` with `NoProject`/`Ambiguous`
// ownership is EXCLUDED from the batch.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { SourceMap } from "node:module";
import type * as TS from "typescript";

import { VIRTUAL_FILE_NAMING, type VirtualPathPolicy } from "../generated/virtual-file-naming";

/**
 * The component-carrier source extensions (`.vue`, `.svelte`), derived from the
 * generated virtual-file-naming column — the single descriptor authority. A
 * row contributes iff it declares a `carrierExtension` AND a distinct
 * import-surface suffix (a `selfFile` rune module is not a component carrier).
 * Deriving these (rather than hardcoding `.vue`/`.svelte`) keeps the driver
 * carrier-generic: adding a framework needs no edit here.
 */
const CARRIER_SOURCE_EXTENSIONS: readonly string[] = Object.values(VIRTUAL_FILE_NAMING)
  .filter((row) => row.carrierExtension !== null && row.importSurface.kind === "suffix")
  .map((row) => row.carrierExtension as string);

/** The carrier source extension a path ends with (`.vue`/`.svelte`), or undefined. */
function carrierSourceExtensionOf(p: string): string | undefined {
  return CARRIER_SOURCE_EXTENSIONS.find((ext) => p.endsWith(ext));
}

/**
 * The minimal `VerterHost` surface the driver consumes for carrier content. The
 * concrete binding is `@verter/native`'s `VerterHost`; the driver accepts it as
 * an interface so a test can inject a fake host without the `.node` binary.
 */
export interface CarrierCodegenHost {
  /** Register a framework source (`.vue`/`.svelte`) so its carriers can be read. */
  upsert(request: {
    canonicalId?: string;
    inputId: string;
    source: string;
    fileKind?: "vue" | "svelte" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
    aliases?: string[];
  }): unknown;
  /**
   * Ensure the IDE (`CachedTsx`) projection exists for a file + profile.
   * `getIde` is a PURE CACHED READ — it returns `null` until the IDE surface
   * has been compiled, so the driver calls this first. Returns `true` when the
   * projection now exists, `false` for a non-carrier.
   */
  ensureIdeCompiled?(
    canonicalId: string,
    profile?: { target?: "bundler" | "ide" | "analysis"; sourceMap?: boolean },
  ): boolean;
  /** The IDE diagnostic carrier (`.vue.tsx`/`.svelte.tsx`) content + map. */
  getIde(
    canonicalId: string,
    profile?: { target?: "bundler" | "ide" | "analysis"; sourceMap?: boolean },
  ): { code: string; sourceMap?: string; isJsx: boolean } | null;
  /** The API/declaration carrier (`.verter.ts`) content + map for referenced projects. */
  getPublicApi(
    canonicalId: string,
    mode?: "public" | "testing",
  ): { code: string; sourceMap?: string } | null;
  /** Release host resources before the process exits (prevents hangs). */
  close?(): void;
}

/** The framework of a carrier source — selects the virtual-file-naming row. */
export type CarrierFramework = "vue" | "svelte";

/** Project-ownership disposition for a `.vue`/`.svelte` (from the §2.6 resolver). */
export type CarrierOwnership = "Owned" | "NoProject" | "Ambiguous";

/**
 * One carrier source the project owns. The driver materialises a carrier for
 * each `Owned` entry (the leaf project's IDE carrier, or a referenced project's
 * declaration carrier) and EXCLUDES `NoProject`/`Ambiguous` from the batch.
 */
export interface CarrierSource {
  /** Absolute path of the `.vue`/`.svelte` source on the user's disk. */
  sourcePath: string;
  /** Raw source text (read from the VFS/disk by the lean caller). */
  source: string;
  framework: CarrierFramework;
  /** §2.6 ownership; `NoProject`/`Ambiguous` ⇒ excluded from the batch. */
  ownership: CarrierOwnership;
  /**
   * The carrier role to materialise:
   * - `"ide"` — the leaf diagnostic carrier (`getIde` → `.vue.tsx`).
   * - `"api"` — a referenced project's declaration carrier (`getPublicApi` →
   *   `.verter.ts`), the surface build mode emits a `.d.ts` from.
   */
  role: "ide" | "api";
  /**
   * Absolute path of the `tsconfig.json` of the project that OWNS this carrier.
   * Defaults to the leaf `tsconfigPath` when omitted (the single-project case).
   * For a referenced project's `role: "api"` carrier this is the REFERENCED
   * project's own tsconfig, so the driver groups it under that project's mirror
   * subtree + generated config (§2.4 — a referenced project's carriers
   * materialise in its own mirror subtree under its own generated tsconfig).
   */
  projectTsconfigPath?: string;
}

/** A single batch-typecheck diagnostic, mapped back to a source position. */
export interface BatchDiagnostic {
  /**
   * The file the diagnostic belongs to. For a carrier-mapped diagnostic this is
   * the original `.vue`/`.svelte` source path; for a real `.ts`/global/options
   * diagnostic it is that file path (or `undefined` for a global diagnostic).
   */
  fileName: string | undefined;
  /** UTF-16 offset into `fileName`'s text, or `undefined` for a global diagnostic. */
  start: number | undefined;
  /** Length in UTF-16 code units, or `undefined` for a global diagnostic. */
  length: number | undefined;
  /** The flattened message text. */
  messageText: string;
  /** The TS diagnostic code (e.g. `2322`). */
  code: number;
  /** 0=warning, 1=error, 2=suggestion, 3=message — the `ts.DiagnosticCategory`. */
  category: number;
  /**
   * True when this diagnostic was produced inside a carrier and mapped back to
   * its source through the carrier source map. False for a real-file / global /
   * options diagnostic.
   */
  mappedFromCarrier: boolean;
}

/** Arguments to {@link runBatchTypecheck}. */
export interface RunBatchTypecheckArgs {
  /** Absolute path of the leaf project's `tsconfig.json` (the user's config). */
  tsconfigPath: string;
  /**
   * The project-owned carrier sources. The leaf project's entries use
   * `role: "ide"`; referenced-project entries use `role: "api"` (+ their own
   * `projectTsconfigPath`). The caller (lean ownership resolver) supplies this
   * set; `NoProject`/`Ambiguous` entries are excluded by the driver.
   */
  carrierSources: CarrierSource[];
  /**
   * The mirror root: a Verter-owned directory under which ALL carriers, the
   * generated tsconfigs, and any emit are materialised. When omitted, a fresh
   * `fs.mkdtemp` directory under `os.tmpdir()` is created (and removed on
   * completion unless `keepMirror` is set). MUST NOT be under the user's project
   * root.
   */
  mirrorRoot?: string;
  /**
   * The `typescript` module to drive (the user's `<7` install). When omitted,
   * the driver `require`s `"typescript"` from its own resolution. Injected by a
   * test to drive a specific version.
   */
  ts?: typeof TS;
  /**
   * The carrier-codegen host. When omitted, the driver constructs a
   * `@verter/native` `VerterHost`. Injected by a test to avoid the `.node`
   * binary.
   */
  host?: CarrierCodegenHost;
  /** Keep the auto-created mirror dir after the run (debugging). Default: false. */
  keepMirror?: boolean;
}

/** The result of a batch typecheck. */
export interface BatchTypecheckResult {
  /** All diagnostics, mapped back to source positions; generated-only suppressed. */
  diagnostics: BatchDiagnostic[];
  /** The mirror root the run used (created or supplied). */
  mirrorRoot: string;
  /** True when the run drove build mode (emit-then-check) for project references. */
  buildMode: boolean;
  /**
   * The absolute carrier paths materialised under the mirror, keyed by source
   * path. Lets a caller (and the zero-working-tree guard) assert nothing landed
   * outside the mirror.
   */
  materializedCarriers: Map<string, string>;
}

/** Resolve a virtual-file-naming suffix policy to a concrete file path. */
function applySuffixPolicy(
  policy: VirtualPathPolicy,
  carrierFullPath: string,
  isJsx: boolean,
): string | null {
  switch (policy.kind) {
    case "none":
      return null;
    case "selfFile":
      return carrierFullPath;
    case "suffix":
      return carrierFullPath + policy.suffix;
    case "jsxConditional":
      return carrierFullPath + (isJsx ? policy.jsx : policy.nonJsx);
  }
}

/**
 * The framework-tag key into `VIRTUAL_FILE_NAMING` for a carrier framework. The
 * map is keyed by the closed `CarrierFramework` union (the descriptor's
 * `FrameworkTag` names), so adding a framework is a single-row change with no
 * literal carrier gate.
 */
const NAMING_KEY_BY_FRAMEWORK: Record<CarrierFramework, keyof typeof VIRTUAL_FILE_NAMING> = {
  vue: "FRAMEWORK_TAG_VUE",
  svelte: "FRAMEWORK_TAG_SVELTE",
};
function namingKeyFor(framework: CarrierFramework): keyof typeof VIRTUAL_FILE_NAMING {
  return NAMING_KEY_BY_FRAMEWORK[framework];
}

/**
 * The companion carrier path for a source, under the mirror, derived from the
 * descriptor-owned virtual-file-naming column (the single authority for the
 * `.vue.tsx`/`.svelte.tsx`/`.verter.ts` suffixes).
 *
 * - `role: "ide"` uses the `ide` policy (Vue: `.jsx`/`.tsx` by JSX; Svelte:
 *   `.tsx`).
 * - `role: "api"` uses the `importSurface` policy (`.verter.ts`).
 *
 * The companion is placed at the source's MIRRORED location
 * (`mirrorPathFor(sourcePath)`) plus the policy suffix — so the mirror mirrors
 * the user tree's layout (keeping `rootDir` valid).
 */
function companionMirrorPath(
  src: CarrierSource,
  mirroredSource: string,
  isJsx: boolean,
): string | null {
  const naming = VIRTUAL_FILE_NAMING[namingKeyFor(src.framework)];
  const policy = src.role === "ide" ? naming.ide : naming.importSurface;
  // The carrier suffix appends to the FULL carrier canonical (e.g. `App.vue` +
  // `.tsx` => `App.vue.tsx`), so the policy applies to the mirrored source path.
  return applySuffixPolicy(policy, mirroredSource, isJsx);
}

/** Normalise a path to forward slashes (TS internal path convention). */
function toSlash(p: string): string {
  return p.replace(/\\/g, "/");
}

/**
 * The common-ancestor DIRECTORY of a set of absolute FILE paths (slash form).
 *
 * Every input is a FILE path (a tsconfig, a `.vue`/`.svelte` source), so its
 * DIRECTORY is taken FIRST — the mirror base is a directory the mirrored layout
 * re-roots under. Without the dirname, a SINGLETON input (one source + one
 * co-located tsconfig collapse to a single dir, or a lone path) returned the
 * full FILE path as the "ancestor", so `mirrorPathFor` re-rooted every path
 * RELATIVE to a file — and the generated `tsconfig.verter.json`
 * (`path.dirname(mirrorPathFor(tsconfig))`) could land OUTSIDE the mirror root.
 * Taking the directory first makes the singleton case yield that file's
 * directory, so the mirror layout is always rooted at a real directory.
 *
 * Exported for the zero-working-tree guard's singleton characterization test.
 */
export function commonAncestorDir(paths: string[]): string {
  if (paths.length === 0) return "/";
  const split = paths.map((p) => toSlash(path.dirname(path.resolve(p))).split("/"));
  const first = split[0];
  let commonLen = first.length;
  for (const parts of split.slice(1)) {
    let i = 0;
    while (i < commonLen && i < parts.length && parts[i] === first[i]) i += 1;
    commonLen = i;
  }
  const ancestor = first.slice(0, commonLen).join("/");
  return ancestor.length > 0 ? ancestor : "/";
}

/** Convert a UTF-16 offset into a generated text to a 1-based line/column. */
function offsetToLineColumn(text: string, offset: number): { line: number; column: number } {
  let line = 1;
  let lineStart = 0;
  for (let i = 0; i < offset && i < text.length; i += 1) {
    if (text.charCodeAt(i) === 10 /* \n */) {
      line += 1;
      lineStart = i + 1;
    }
  }
  return { line, column: offset - lineStart + 1 };
}

/** Convert a 1-based line/column in the original text to a UTF-16 offset. */
function lineColumnToOffset(text: string, line: number, column: number): number | null {
  if (line < 1 || column < 1) return null;
  let offset = 0;
  let currentLine = 1;
  while (currentLine < line) {
    const nl = text.indexOf("\n", offset);
    if (nl === -1) return null;
    offset = nl + 1;
    currentLine += 1;
  }
  return offset + (column - 1);
}

/**
 * One materialised carrier's mapping state: the carrier path, the source path it
 * came from, the generated carrier text, and the parsed V3 source map (or null
 * when the carrier carries no map).
 */
interface CarrierMapState {
  carrierPath: string;
  sourcePath: string;
  carrierText: string;
  sourceText: string;
  map: SourceMap | null;
}

/**
 * Map a diagnostic span inside a materialised carrier back to its source
 * position via the carrier's V3 source map. Returns `null` when the carrier
 * carries no map or the span has no source origin (a GENERATED-ONLY span — the
 * caller then SUPPRESSES the diagnostic, never emitting a mis-mapped span).
 *
 * The source position is resolved against the carrier's KNOWN source path (the
 * `.vue`/`.svelte` the driver materialised it from), not the map's `sources[0]`
 * filename — so mapping is robust whether the codegen embeds an absolute,
 * relative, or bare source name.
 */
function mapCarrierSpanToSource(
  state: CarrierMapState,
  start: number,
  length: number,
): { fileName: string; start: number; length: number } | null {
  if (!state.map) return null;
  const { line, column } = offsetToLineColumn(state.carrierText, start);
  const origin = state.map.findOrigin(line, column);
  if (!("fileName" in origin) || !origin.fileName) return null;

  const originalStart = lineColumnToOffset(
    state.sourceText,
    origin.lineNumber,
    origin.columnNumber,
  );
  if (originalStart === null) return null;

  // Map the span END too, so a multi-character carrier span maps to a faithful
  // source span (rather than a fixed length-1). Fall back to length-1 when the
  // end has no origin.
  let mappedLength = 1;
  const endPos = offsetToLineColumn(state.carrierText, start + length);
  const endOrigin = state.map.findOrigin(endPos.line, endPos.column);
  if ("fileName" in endOrigin && endOrigin.fileName) {
    const originalEnd = lineColumnToOffset(
      state.sourceText,
      endOrigin.lineNumber,
      endOrigin.columnNumber,
    );
    if (originalEnd !== null && originalEnd >= originalStart) {
      mappedLength = Math.max(1, originalEnd - originalStart);
    }
  }

  return { fileName: state.sourcePath, start: originalStart, length: mappedLength };
}

/** Parse a V3 source-map JSON string into a `node:module` `SourceMap`, or null. */
function parseMap(raw: string | undefined): SourceMap | null {
  if (raw === undefined) return null;
  try {
    return new SourceMap(JSON.parse(raw) as ConstructorParameters<typeof SourceMap>[0]);
  } catch {
    return null;
  }
}

/** One project's grouping: its user tsconfig + the carriers it owns. */
interface ProjectGroup {
  /** Absolute (slash) path of the user's tsconfig for this project. */
  userTsconfigPath: string;
  /** Absolute (slash) path of the generated `extends`-tsconfig in the mirror. */
  generatedTsconfigPath: string;
  /** The carriers this project owns. */
  carriers: { src: CarrierSource; carrierPath: string }[];
  /** True for the leaf project under check (vs a referenced project). */
  isLeaf: boolean;
}

/**
 * The generated `extends`-tsconfig content for one project's mirror subtree.
 * `extends` the user config; adds the project's materialised carrier companions
 * to a `files` list; preserves everything else (the user controls
 * `rootDir`/`outDir`/`composite`/`paths`/etc. via the extended config). When the
 * project participates in build mode, mirror-local `outDir`/`tsBuildInfoFile` and
 * the `composite`/`declaration` emit boundary are layered in so emit lands in the
 * mirror; the leaf's `references[].path` is re-pointed at each referenced
 * project's generated (mirror) tsconfig.
 */
function buildGeneratedTsconfig(args: { group: ProjectGroup; buildMode: boolean }): {
  json: string;
} {
  const genDir = path.dirname(args.group.generatedTsconfigPath);
  // `extends` is resolved relative to the generated config's own directory.
  const extendsRel = relSlash(genDir, args.group.userTsconfigPath);
  const filesRel = args.group.carriers.map((c) => relSlash(genDir, c.carrierPath));

  const config: Record<string, unknown> = {
    extends: extendsRel,
    files: filesRel,
  };

  const compilerOptions: Record<string, unknown> = {};

  if (args.buildMode && !args.group.isLeaf) {
    // A REFERENCED project emits its `.d.ts` (the declaration boundary the leaf
    // consumes). Re-root `rootDir` to its own mirror subtree (so the materialised
    // carriers are inside it) and keep emit inside the mirror.
    compilerOptions.rootDir = ".";
    compilerOptions.composite = true;
    compilerOptions.declaration = true;
    compilerOptions.emitDeclarationOnly = true;
    compilerOptions.noEmit = false;
    compilerOptions.outDir = relSlash(genDir, path.join(genDir, ".verter-out"));
    compilerOptions.tsBuildInfoFile = relSlash(
      genDir,
      path.join(genDir, ".verter-out", "tsconfig.tsbuildinfo"),
    );
  } else {
    // The leaf (and the no-emit single-project case) is diagnostics-only. It is
    // NOT `composite` and does NOT constrain `rootDir` — its program legitimately
    // pulls in cross-project sources/`.d.ts` through `paths`, which a `composite`
    // `rootDir` would reject (TS6059). The leaf does not reference the generated
    // referenced configs (emit-then-check resolves them through their emitted
    // `.d.ts`, not a `references` edge).
    compilerOptions.noEmit = true;
  }

  config.compilerOptions = compilerOptions;

  return { json: JSON.stringify(config, null, 2) };
}

/** A `./`-prefixed forward-slash relative path from `fromDir` to `to`. */
function relSlash(fromDir: string, to: string): string {
  let rel = toSlash(path.relative(fromDir, to));
  if (rel.length === 0) rel = ".";
  if (!rel.startsWith(".")) rel = "./" + rel;
  return rel;
}

/**
 * Run a whole-project batch typecheck through the mirror-host driver.
 *
 * Guarantees ZERO writes outside `mirrorRoot`: every carrier, the generated
 * tsconfigs, and all emit are materialised under the mirror; the user's working
 * tree is read-only.
 */
export function runBatchTypecheck(args: RunBatchTypecheckArgs): BatchTypecheckResult {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const ts: typeof TS = args.ts ?? (require("typescript") as typeof TS);

  const leafTsconfigPath = toSlash(path.resolve(args.tsconfigPath));

  // The user-tree directories the batch reads from (the leaf tsconfig dir, every
  // owned source dir, every owning-project tsconfig dir). The mirror MUST NOT be
  // inside, equal to, or an ANCESTOR of any of them — that is the zero-working-
  // tree-writes boundary, enforced IN the driver (not merely asserted by a
  // cooperative test). Computed BEFORE the mirror is created/validated so a
  // rejected caller-supplied root never has a directory mkdir'd under it.
  const ownedSourceDirs = args.carrierSources
    .filter((s) => s.ownership === "Owned")
    .flatMap((s) => {
      const dirs = [toSlash(path.dirname(path.resolve(s.sourcePath)))];
      if (s.projectTsconfigPath !== undefined) {
        dirs.push(toSlash(path.dirname(path.resolve(s.projectTsconfigPath))));
      }
      return dirs;
    });
  const userTreeDirs = [toSlash(path.dirname(leafTsconfigPath)), ...ownedSourceDirs];

  // Mirror root: a fresh OS-temp dir, NEVER under the user's project root. A
  // caller-supplied root is VALIDATED against the user tree and rejected when it
  // collides; the default is always a fresh `mkdtemp` under `os.tmpdir()`.
  let ownsMirror = false;
  let mirrorRoot: string;
  if (args.mirrorRoot === undefined) {
    mirrorRoot = toSlash(
      path.resolve(fs.mkdtempSync(path.join(os.tmpdir(), "verter-tsc-mirror-"))),
    );
    ownsMirror = true;
  } else {
    mirrorRoot = toSlash(path.resolve(args.mirrorRoot));
    assertMirrorRootOutsideUserTree(mirrorRoot, userTreeDirs);
  }
  fs.mkdirSync(mirrorRoot, { recursive: true });

  const host: CarrierCodegenHost =
    args.host ??
    (() => {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const native = require("@verter/native") as { VerterHost: new () => CarrierCodegenHost };
      return new native.VerterHost();
    })();

  const result: BatchTypecheckResult = {
    diagnostics: [],
    mirrorRoot,
    buildMode: false,
    materializedCarriers: new Map(),
  };

  try {
    // Exclude NoProject/Ambiguous sources from the batch (fail closed — never
    // check a `.vue` under a synthesised config when ownership is unresolved).
    const owned = args.carrierSources.filter((s) => s.ownership === "Owned");

    // The mirror BASE = common ancestor of every source + every project config,
    // so every mirrored path stays inside the mirror (no `..` escape).
    const ancestorInputs = [
      leafTsconfigPath,
      ...owned.map((s) => toSlash(path.resolve(s.sourcePath))),
      ...owned
        .map((s) => s.projectTsconfigPath)
        .filter((p): p is string => p !== undefined)
        .map((p) => toSlash(path.resolve(p))),
    ];
    const mirrorBase = commonAncestorDir(ancestorInputs);
    const mirrorPathFor = (abs: string): string =>
      toSlash(path.join(mirrorRoot, path.relative(mirrorBase, toSlash(path.resolve(abs)))));

    const carrierStates: CarrierMapState[] = [];

    // Group carriers by owning project (defaulting to the leaf).
    const groups = new Map<string, ProjectGroup>();
    const groupFor = (userTsconfig: string): ProjectGroup => {
      const key = toSlash(path.resolve(userTsconfig));
      let g = groups.get(key);
      if (g === undefined) {
        g = {
          userTsconfigPath: key,
          generatedTsconfigPath: toSlash(
            path.join(path.dirname(mirrorPathFor(key)), "tsconfig.verter.json"),
          ),
          carriers: [],
          isLeaf: key === leafTsconfigPath,
        };
        groups.set(key, g);
      }
      return g;
    };
    // Always create the leaf group so a leaf with no carriers still produces a
    // config (and the no-emit path has a config to drive).
    groupFor(leafTsconfigPath);

    // The host canonical id is the source BASENAME, so the carrier's self-import
    // (`import('./{canonical}.verter.ts')`, the IDE carrier's instance typing)
    // is a clean sibling path that resolves next to the materialised carrier.
    // Collisions (same basename, different dirs) are disambiguated with a numeric
    // suffix on the basename STEM, preserving the `.vue`/`.svelte` extension.
    const usedCanonicals = new Map<string, number>();
    const canonicalForSource = (sourcePath: string): string => {
      const base = path.basename(toSlash(sourcePath));
      const seen = usedCanonicals.get(base);
      if (seen === undefined) {
        usedCanonicals.set(base, 1);
        return base;
      }
      usedCanonicals.set(base, seen + 1);
      // `Foo.vue` → `Foo.2.vue` (keep the framework extension last).
      const ext = carrierSourceExtensionOf(base) ?? path.extname(base);
      const stem = base.slice(0, base.length - ext.length);
      return `${stem}.${seen + 1}${ext}`;
    };

    /** Materialise a carrier file + its `.map`, register its map state. */
    const materialize = (
      mirroredSource: string,
      src: CarrierSource,
      role: "ide" | "api",
      code: string,
      mapRaw: string | undefined,
      isJsx: boolean,
    ): string | null => {
      const carrierPath = companionMirrorPath({ ...src, role }, mirroredSource, isJsx);
      if (carrierPath === null) return null;
      const carrierAbs = toSlash(carrierPath);
      fs.mkdirSync(path.dirname(carrierAbs), { recursive: true });
      fs.writeFileSync(carrierAbs, code, "utf8");
      if (mapRaw !== undefined) fs.writeFileSync(carrierAbs + ".map", mapRaw, "utf8");
      carrierStates.push({
        carrierPath: carrierAbs,
        sourcePath: toSlash(src.sourcePath),
        carrierText: code,
        sourceText: src.source,
        map: parseMap(mapRaw),
      });
      return carrierAbs;
    };

    for (const src of owned) {
      const sourcePath = toSlash(src.sourcePath);
      const canonical = canonicalForSource(sourcePath);
      host.upsert({
        canonicalId: canonical,
        inputId: canonical,
        source: src.source,
        fileKind: src.framework,
      });

      // The carrier is materialised at the source's MIRRORED location, but its
      // BASENAME follows the host canonical (so the carrier file name matches the
      // self-import's `{canonical}.verter.ts` sibling path).
      const mirroredSourceDir = path.dirname(mirrorPathFor(sourcePath));
      const mirroredSource = toSlash(path.join(mirroredSourceDir, canonical));

      const group = groupFor(src.projectTsconfigPath ?? leafTsconfigPath);

      if (src.role === "ide") {
        // `getIde` is a pure cached read — populate the IDE projection first.
        const ideProfile = { target: "ide" as const, sourceMap: true };
        host.ensureIdeCompiled?.(canonical, ideProfile);
        const ide = host.getIde(canonical, ideProfile);
        if (!ide) continue; // no IDE surface (non-carrier) — skip.

        const ideCarrier = materialize(
          mirroredSource,
          src,
          "ide",
          ide.code,
          ide.sourceMap,
          ide.isJsx,
        );
        if (ideCarrier === null) continue;
        result.materializedCarriers.set(sourcePath, ideCarrier);
        group.carriers.push({ src, carrierPath: ideCarrier });

        // The IDE carrier self-imports `./{canonical}.verter.ts` for instance
        // typing — materialise the leaf's OWN API carrier as a sibling so that
        // self-import resolves. It is NOT added to the project's `files` (it is a
        // dependency reached through the self-import, not a root), and its
        // diagnostics are mapped back to the same source.
        const api = host.getPublicApi(canonical, "public");
        if (api) {
          materialize(mirroredSource, src, "api", api.code, api.sourceMap, false);
        }
      } else {
        const api = host.getPublicApi(canonical, "public");
        if (!api) continue;
        const apiCarrier = materialize(mirroredSource, src, "api", api.code, api.sourceMap, false);
        if (apiCarrier === null) continue;
        result.materializedCarriers.set(sourcePath, apiCarrier);
        group.carriers.push({ src, carrierPath: apiCarrier });
      }
    }

    // A carrier-path → map-state lookup for diagnostic mapping.
    const carrierByPath = new Map<string, CarrierMapState>();
    for (const cs of carrierStates) carrierByPath.set(cs.carrierPath, cs);

    // Detect project references on the leaf user config to choose emit mode.
    const hasReferences = userConfigHasReferences(ts, leafTsconfigPath);
    result.buildMode = hasReferences;

    // Generated-mirror artifacts whose diagnostics must be stripped (the batch
    // promises the user's own config/options set).
    const generatedArtifacts = new Set<string>();

    // Emit the generated tsconfig per project. In emit-then-check build mode each
    // referenced project emits its `.d.ts`; the leaf consumes those `.d.ts`
    // boundaries through resolution (not a `references` edge).
    const leafGroup = groups.get(leafTsconfigPath)!;
    const referencedGroups = [...groups.values()].filter((g) => !g.isLeaf);

    for (const group of groups.values()) {
      fs.mkdirSync(path.dirname(group.generatedTsconfigPath), { recursive: true });
      const { json } = buildGeneratedTsconfig({ group, buildMode: hasReferences });
      fs.writeFileSync(group.generatedTsconfigPath, json, "utf8");
      generatedArtifacts.add(toSlash(group.generatedTsconfigPath));
      for (const c of group.carriers) generatedArtifacts.add(toSlash(c.carrierPath));
    }

    // The CompilerHost / build host serves mirror files and falls through to the
    // user tree. `resolveModuleNames` redirects `.vue`/`.svelte` specifiers to
    // the materialised carrier.
    const mirrorFs = new MirrorFs(mirrorRoot, carrierByPath);

    const rawDiagnostics: TS.Diagnostic[] = hasReferences
      ? runBuildMode(ts, leafGroup, referencedGroups, mirrorFs)
      : runNoEmitMode(ts, leafGroup.generatedTsconfigPath, mirrorFs);

    // Classify + map every diagnostic.
    for (const diag of rawDiagnostics) {
      const mapped = classifyDiagnostic(ts, diag, carrierByPath, generatedArtifacts);
      if (mapped !== null) {
        result.diagnostics.push(mapped);
      }
    }
  } finally {
    if (args.host === undefined) {
      host.close?.();
    }
    if (ownsMirror && !args.keepMirror) {
      try {
        fs.rmSync(mirrorRoot, { recursive: true, force: true });
      } catch {
        // best-effort cleanup; a failed rm of a temp dir is non-fatal.
      }
    }
  }

  return result;
}

/** Parse the leaf user config and report whether it declares project references. */
function userConfigHasReferences(ts: typeof TS, userTsconfigPath: string): boolean {
  const read = ts.readConfigFile(userTsconfigPath, ts.sys.readFile);
  if (read.error || read.config === undefined) return false;
  const parsed = ts.parseJsonConfigFileContent(
    read.config,
    ts.sys,
    path.dirname(userTsconfigPath),
    undefined,
    userTsconfigPath,
  );
  return (parsed.projectReferences?.length ?? 0) > 0;
}

/**
 * Classify and map a raw TS diagnostic:
 * - A diagnostic inside a materialised carrier is mapped back to its source via
 *   the carrier map; a GENERATED-ONLY span (no source origin) is SUPPRESSED
 *   (returns `null`).
 * - A diagnostic naming a generated mirror artifact (the generated tsconfig, an
 *   injected companion) is STRIPPED (returns `null`) — the batch promises the
 *   user's own config/options set; the generated config's deprecation/rootDir
 *   noise is Verter's, not the user's.
 * - Every other diagnostic (real `.ts`, global, user-config options) passes
 *   through unmapped.
 */
function classifyDiagnostic(
  ts: typeof TS,
  diag: TS.Diagnostic,
  carrierByPath: Map<string, CarrierMapState>,
  generatedArtifacts: Set<string>,
): BatchDiagnostic | null {
  const messageText = ts.flattenDiagnosticMessageText(diag.messageText, "\n");
  const base = {
    code: diag.code,
    category: diag.category as number,
    messageText,
  };

  // Strip any diagnostic whose MESSAGE names a generated mirror artifact (the
  // generated tsconfig path, an injected companion path) — Verter's own, never
  // the user's. This covers options/config diagnostics keyed to the generated
  // config (deprecations, rootDir-membership) regardless of `diag.file`.
  for (const artifact of generatedArtifacts) {
    if (messageText.includes(artifact)) return null;
  }

  if (diag.file === undefined) {
    return {
      ...base,
      fileName: undefined,
      start: undefined,
      length: undefined,
      mappedFromCarrier: false,
    };
  }

  const filePath = toSlash(diag.file.fileName);

  // A diagnostic ON a generated artifact file (the generated tsconfig) is
  // stripped too.
  if (generatedArtifacts.has(filePath) && !carrierByPath.has(filePath)) {
    return null;
  }

  const carrier = carrierByPath.get(filePath);
  if (carrier !== undefined) {
    if (diag.start === undefined || diag.length === undefined) {
      return {
        ...base,
        fileName: carrier.sourcePath,
        start: undefined,
        length: undefined,
        mappedFromCarrier: true,
      };
    }
    const mapped = mapCarrierSpanToSource(carrier, diag.start, diag.length);
    if (mapped === null) {
      // GENERATED-ONLY span — suppress (never emit a mis-mapped diagnostic).
      return null;
    }
    return {
      ...base,
      fileName: mapped.fileName,
      start: mapped.start,
      length: mapped.length,
      mappedFromCarrier: true,
    };
  }

  // A real user-tree file (`.ts`/`.tsx`/`.d.ts`) — pass through unmapped.
  return {
    ...base,
    fileName: filePath,
    start: diag.start,
    length: diag.length,
    mappedFromCarrier: false,
  };
}

/**
 * The mirror-aware filesystem seam: it answers reads for mirror files (carriers,
 * the generated tsconfigs, emit) and the redirection of `.vue`/`.svelte`
 * specifiers, and otherwise falls through to the real user tree.
 *
 * It is shared by both the no-emit `CompilerHost` and the build-mode solution
 * builder host so the read/redirection behaviour is identical across emit modes.
 */
class MirrorFs {
  /** Emitted real mirror files captured in memory (the referenced `.d.ts`s). */
  private readonly emittedFiles = new Map<string, string>();

  constructor(
    readonly mirrorRoot: string,
    private readonly carrierByPath: Map<string, CarrierMapState>,
  ) {}

  /** Record an emitted mirror file's content (a referenced project's `.d.ts`). */
  registerRealFile(fileName: string, text: string): void {
    this.emittedFiles.set(toSlash(path.resolve(fileName)), text);
  }

  /** Whether `fileName` is the materialised carrier text (served from the map state). */
  isCarrier(fileName: string): boolean {
    return this.carrierByPath.has(toSlash(fileName));
  }

  /**
   * Read a file: a carrier's generated text, a real mirror file (generated
   * tsconfig / emitted `.d.ts`), else fall through to real disk.
   */
  readFile(fileName: string, _encoding?: string): string | undefined {
    const slash = toSlash(fileName);
    const carrier = this.carrierByPath.get(slash);
    if (carrier !== undefined) return carrier.carrierText;
    const emitted = this.emittedFiles.get(toSlash(path.resolve(slash)));
    if (emitted !== undefined) return emitted;
    return realReadFile(slash);
  }

  /** `fileExists`: true for a carrier or emitted mirror file, else the real disk. */
  fileExists(fileName: string): boolean {
    const slash = toSlash(fileName);
    if (this.carrierByPath.has(slash)) return true;
    if (this.emittedFiles.has(toSlash(path.resolve(slash)))) return true;
    return realFileExists(slash);
  }

  /** The set of carrier source paths (the `.vue`/`.svelte` specifiers to redirect). */
  carrierBySourceSpecifier(): Map<string, string> {
    const m = new Map<string, string>();
    for (const cs of this.carrierByPath.values()) {
      m.set(cs.sourcePath, cs.carrierPath);
    }
    return m;
  }

  /**
   * Map a carrier path (in the mirror) → its REAL source path in the user tree.
   * Module resolution from a carrier re-roots to this real source directory so
   * relative/`baseUrl`/`paths`/`node_modules` resolution is byte-for-byte the
   * user's (§2.4 — the host re-roots resolution into the user tree).
   */
  realContainingFileFor(containingFile: string): string {
    const carrier = this.carrierByPath.get(toSlash(containingFile));
    return carrier !== undefined ? carrier.sourcePath : toSlash(containingFile);
  }
}

// Real-disk reads are direct `node:fs` so the driver never couples to a
// particular `ts.sys` instance — the user tree (and the real mirror files on
// disk) is the fallback for every non-carrier read.
function realReadFile(fileName: string): string | undefined {
  try {
    return fs.readFileSync(fileName, "utf8");
  } catch {
    return undefined;
  }
}
function realFileExists(fileName: string): boolean {
  try {
    return fs.statSync(fileName).isFile();
  } catch {
    return false;
  }
}

/**
 * The shared per-specifier resolver: a framework-carrier specifier redirects to
 * its materialised carrier (host `resolveModuleNames` override — D7,
 * resolver-version independent); everything else goes through stock
 * `ts.resolveModuleName` against the real user tree (TS's own resolver runs
 * first).
 */
function makeResolveOne(
  ts: typeof TS,
  // Default options kept for signature parity with `resolveModuleNames`; the
  // per-call `optionsArg` is the authority for each resolution.
  _options: TS.CompilerOptions,
  mirrorFs: MirrorFs,
  emittedDtsForCarrier?: Map<string, string>,
): (
  name: string,
  containingFile: string,
  optionsArg: TS.CompilerOptions,
  redirectedReference: TS.ResolvedProjectReference | undefined,
) => TS.ResolvedModuleFull | undefined {
  const sourceToCarrier = mirrorFs.carrierBySourceSpecifier();
  const resolutionHost: TS.ModuleResolutionHost = {
    fileExists: (f) => mirrorFs.fileExists(f),
    readFile: (f) => mirrorFs.readFile(f),
  };
  return (name, containingFile, optionsArg, redirectedReference) => {
    const containingSlash = toSlash(containingFile);
    const realContaining = mirrorFs.realContainingFileFor(containingSlash);

    // (1) A framework-carrier specifier redirects to its materialised carrier.
    //     Verter's IDE carrier rewrites a `.vue` import to the IDE-carrier
    //     identity (`./C.vue.tsx`, `@lib/C.vue.tsx`); the source-level form is
    //     `./C.vue`. Both forms must redirect. The carrier specifier is resolved
    //     to the REAL `.vue`/`.svelte` source path (relative from the carrier's
    //     real source dir, OR through `paths`/stock resolution for an aliased
    //     specifier), then mapped to the materialised carrier. When that carrier
    //     belongs to a REFERENCED project that has emitted a `.d.ts` (build
    //     mode), the redirect targets the `.d.ts` boundary — the same way the
    //     user's own `tsc -b` consumes it.
    const sourceSpecifier = carrierSpecifierToSource(name);
    if (sourceSpecifier !== undefined) {
      const realSourcePath = resolveCarrierSourcePath(
        ts,
        sourceSpecifier,
        realContaining,
        optionsArg,
        resolutionHost,
        redirectedReference,
      );
      if (realSourcePath !== undefined) {
        const carrierPath = sourceToCarrier.get(realSourcePath);
        if (carrierPath !== undefined) {
          const dts = emittedDtsForCarrier?.get(toSlash(carrierPath));
          const target = dts ?? carrierPath;
          return {
            resolvedFileName: target,
            extension: carrierExtension(ts, target),
            isExternalLibraryImport: false,
          };
        }
      }
    }

    // (2) A relative specifier targeting a materialised carrier in the MIRROR
    //     (the IDE carrier's `./{name}.verter.ts` self-import) resolves in the
    //     mirror, relative to the carrier's mirror directory — it is a
    //     Verter-generated sibling, NOT a user-tree file, so it must NOT re-root.
    if (mirrorFs.isCarrier(containingSlash) && (name.startsWith("./") || name.startsWith("../"))) {
      const mirrorCandidate = toSlash(path.resolve(path.dirname(containingSlash), name));
      if (mirrorFs.isCarrier(mirrorCandidate) || mirrorFs.fileExists(mirrorCandidate)) {
        return {
          resolvedFileName: mirrorCandidate,
          extension: carrierExtension(ts, mirrorCandidate),
          isExternalLibraryImport: false,
        };
      }
    }

    // (3) Everything else: stock resolution against the real user tree, re-rooted
    //     to the carrier's real source directory so relative / `baseUrl` /
    //     `paths` / `node_modules` resolution is byte-for-byte the user's (§2.4).
    const resolved = ts.resolveModuleName(
      name,
      realContaining,
      optionsArg,
      resolutionHost,
      undefined,
      redirectedReference,
    );
    return resolved.resolvedModule;
  };
}

/** Build the `resolveModuleNames` host override over {@link makeResolveOne}. */
function makeResolveModuleNames(
  ts: typeof TS,
  options: TS.CompilerOptions,
  mirrorFs: MirrorFs,
  emittedDtsForCarrier?: Map<string, string>,
): (
  moduleNames: string[],
  containingFile: string,
  reusedNames: string[] | undefined,
  redirectedReference: TS.ResolvedProjectReference | undefined,
  optionsArg: TS.CompilerOptions,
) => (TS.ResolvedModuleFull | undefined)[] {
  const resolveOne = makeResolveOne(ts, options, mirrorFs, emittedDtsForCarrier);
  return (moduleNames, containingFile, _reusedNames, redirectedReference, optionsArg) =>
    moduleNames.map((name) =>
      resolveOne(name, containingFile, optionsArg ?? options, redirectedReference),
    );
}

/**
 * Recover the `.vue`/`.svelte` SOURCE specifier from a framework-carrier import
 * specifier, or `undefined` when `name` is not a framework-carrier import.
 *
 * Two forms occur: the source-level `./C.vue` (a user import the carrier may
 * preserve), and the IDE-carrier identity `./C.vue.tsx` / `@lib/C.svelte.tsx`
 * (the form Verter's IDE codegen rewrites a `.vue` import to — the bare-import
 * probe identity). Both reduce to the `.vue`/`.svelte` source specifier.
 */
function carrierSpecifierToSource(name: string): string | undefined {
  for (const ext of CARRIER_SOURCE_EXTENSIONS) {
    if (name.endsWith(ext)) return name;
    for (const ide of [ext + ".tsx", ext + ".jsx"]) {
      if (name.endsWith(ide)) return name.slice(0, name.length - (ide.length - ext.length));
    }
  }
  return undefined;
}

/**
 * Resolve a `.vue`/`.svelte` SOURCE specifier to its real source path in the
 * user tree. A relative specifier resolves against the carrier's real source
 * directory; an aliased/bare specifier (`@lib/C.vue`) resolves through
 * `paths`/`baseUrl`/`node_modules` via stock resolution with the extension
 * temporarily stripped (TS would not otherwise resolve a `.vue` literal). Returns
 * the real source path (slash form) or `undefined`.
 */
function resolveCarrierSourcePath(
  ts: typeof TS,
  sourceSpecifier: string,
  realContaining: string,
  optionsArg: TS.CompilerOptions,
  resolutionHost: TS.ModuleResolutionHost,
  redirectedReference: TS.ResolvedProjectReference | undefined,
): string | undefined {
  if (sourceSpecifier.startsWith("./") || sourceSpecifier.startsWith("../")) {
    return toSlash(path.resolve(path.dirname(realContaining), sourceSpecifier));
  }
  // A bare/aliased specifier: TS cannot resolve a `.vue` literal, so resolve the
  // extension-less stem through `paths`/`baseUrl`. Try stock resolution first
  // (handles a co-located `.ts` shim), then a direct `paths` substitution
  // (handles the common case where only the `.vue` source exists at the target).
  const ext = carrierSourceExtensionOf(sourceSpecifier) ?? path.extname(sourceSpecifier);
  const stem = sourceSpecifier.slice(0, sourceSpecifier.length - ext.length);

  const resolved = ts.resolveModuleName(
    stem,
    realContaining,
    optionsArg,
    resolutionHost,
    undefined,
    redirectedReference,
  );
  if (resolved.resolvedModule !== undefined) {
    const resolvedDir = path.dirname(toSlash(resolved.resolvedModule.resolvedFileName));
    return toSlash(path.join(resolvedDir, path.basename(stem) + ext));
  }

  // Direct `paths` substitution: for each matching `paths` pattern, substitute
  // the wildcard and re-root the target against `baseUrl` to get the candidate
  // source path. `paths`/`baseUrl` are absolute in the parsed options.
  for (const candidate of pathsSubstitutions(stem, optionsArg)) {
    return toSlash(candidate + ext);
  }
  return undefined;
}

/**
 * Apply the parsed `paths` patterns to a bare specifier stem, yielding candidate
 * extension-less target paths (absolute). `baseUrl` and the `paths` targets are
 * already absolute in a parsed `CompilerOptions`. Supports the single trailing
 * `*` wildcard form TS uses.
 */
function* pathsSubstitutions(stem: string, options: TS.CompilerOptions): Generator<string> {
  const paths = options.paths;
  const baseUrl = options.baseUrl;
  if (paths === undefined) return;
  for (const [pattern, targets] of Object.entries(paths)) {
    const star = pattern.indexOf("*");
    let matched: string | undefined;
    if (star === -1) {
      if (pattern === stem) matched = "";
    } else {
      const prefix = pattern.slice(0, star);
      const suffix = pattern.slice(star + 1);
      if (
        stem.startsWith(prefix) &&
        stem.endsWith(suffix) &&
        stem.length >= prefix.length + suffix.length
      ) {
        matched = stem.slice(prefix.length, stem.length - suffix.length);
      }
    }
    if (matched === undefined) continue;
    for (const target of targets) {
      const substituted = target.includes("*") ? target.replace("*", matched) : target;
      // A `paths` target is resolved relative to `baseUrl` (when relative).
      const abs =
        path.isAbsolute(substituted) || baseUrl === undefined
          ? substituted
          : path.resolve(baseUrl, substituted);
      yield toSlash(abs);
    }
  }
}

/** The `ts.Extension` for a materialised carrier (or emitted `.d.ts`) path. */
function carrierExtension(ts: typeof TS, carrierPath: string): TS.Extension {
  if (carrierPath.endsWith(".d.ts")) return ts.Extension.Dts;
  if (carrierPath.endsWith(".tsx")) return ts.Extension.Tsx;
  if (carrierPath.endsWith(".jsx")) return ts.Extension.Jsx;
  if (carrierPath.endsWith(".ts")) return ts.Extension.Ts;
  return ts.Extension.Ts;
}

/**
 * Run the no-emit diagnostics-only mode (`ts.createProgram` with `noEmit:
 * true`). Collects syntactic + semantic per-file diagnostics plus program-level
 * global/options/config diagnostics. Nothing is written.
 */
function runNoEmitMode(
  ts: typeof TS,
  generatedTsconfigPath: string,
  mirrorFs: MirrorFs,
  emittedDtsForCarrier?: Map<string, string>,
): TS.Diagnostic[] {
  const parsed = parseGeneratedConfig(ts, generatedTsconfigPath, mirrorFs);
  const options: TS.CompilerOptions = { ...parsed.options, noEmit: true };

  const compilerHost = ts.createCompilerHost(options);
  patchCompilerHost(ts, compilerHost, options, mirrorFs, emittedDtsForCarrier);

  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options,
    host: compilerHost,
    projectReferences: parsed.projectReferences,
  });

  const diags: TS.Diagnostic[] = [];
  diags.push(...program.getConfigFileParsingDiagnostics());
  diags.push(...program.getOptionsDiagnostics());
  diags.push(...program.getGlobalDiagnostics());
  diags.push(...program.getSyntacticDiagnostics());
  diags.push(...program.getSemanticDiagnostics());
  if (options.declaration || options.composite) {
    diags.push(...program.getDeclarationDiagnostics());
  }
  return diags;
}

/**
 * Run build mode for a referenced-project graph as EMIT-THEN-CHECK:
 *
 * 1. Each referenced project is compiled `emitDeclarationOnly` (its `composite`
 *    declaration boundary), writing its real `.d.ts` into the mirror — so the
 *    referencing project resolves it the way the user's own `tsc -b` does
 *    (§2.4 — build mode consumes a referenced project through its emitted
 *    `.d.ts` boundary, uniformly, regardless of the source-of-project-reference
 *    redirect setting which is an LSP-only concern).
 * 2. The leaf is then type-checked `noEmit` through the SAME patched
 *    `ts.CompilerHost` (`createProgram`), so the `.vue`→carrier redirect, the
 *    `paths`/`baseUrl`/`node_modules` re-rooting, and cross-file type flow all
 *    use the one proven resolution path. A referenced project's `.vue` resolves
 *    to its emitted `.d.ts` (not its source carrier).
 *
 * (This replaces a `createSolutionBuilder` driver, whose per-project Programs do
 * NOT honour the host `resolveModuleNames` override — the `.vue` redirect /
 * `paths` re-rooting silently miss there.) Every write lands in the mirror;
 * nothing touches the user tree.
 */
function runBuildMode(
  ts: typeof TS,
  leafGroup: ProjectGroup,
  referencedGroups: ProjectGroup[],
  mirrorFs: MirrorFs,
): TS.Diagnostic[] {
  const diags: TS.Diagnostic[] = [];

  // Map each referenced carrier (`*.verter.ts`) → its emitted `.d.ts` path, so
  // the leaf resolves an imported referenced `.vue` to the `.d.ts` boundary.
  const emittedDtsForCarrier = new Map<string, string>();

  // Step 1 — emit each referenced project's `.d.ts` into the mirror.
  for (const group of referencedGroups) {
    const parsed = parseGeneratedConfig(ts, group.generatedTsconfigPath, mirrorFs);
    const options: TS.CompilerOptions = {
      ...parsed.options,
      emitDeclarationOnly: true,
      declaration: true,
      noEmit: false,
    };
    const compilerHost = ts.createCompilerHost(options);
    patchCompilerHost(ts, compilerHost, options, mirrorFs, emittedDtsForCarrier);
    // Capture emitted files into the mirror (defence-in-depth: refuse a write
    // outside the mirror) and record each carrier's `.d.ts`.
    const originalWriteFile = compilerHost.writeFile.bind(compilerHost);
    compilerHost.writeFile = (fileName, text, bom, onError, sourceFiles, data) => {
      assertInsideMirror(mirrorFs.mirrorRoot, fileName);
      originalWriteFile(fileName, text, bom, onError, sourceFiles, data);
      mirrorFs.registerRealFile(fileName, text);
    };
    const program = ts.createProgram({
      rootNames: parsed.fileNames,
      options,
      host: compilerHost,
    });
    diags.push(...program.getConfigFileParsingDiagnostics());
    diags.push(...program.getOptionsDiagnostics());
    diags.push(...program.getGlobalDiagnostics());
    diags.push(...program.getSyntacticDiagnostics());
    diags.push(...program.getSemanticDiagnostics());
    diags.push(...program.getDeclarationDiagnostics());
    program.emit();

    // Record the `.d.ts` each carrier produced (`<carrier>.ts` → `<carrier>.d.ts`
    // under outDir, mirroring rootDir → outDir).
    for (const c of group.carriers) {
      const dts = emittedDeclarationPathFor(c.carrierPath, parsed.options);
      if (dts !== undefined && mirrorFs.fileExists(dts)) {
        emittedDtsForCarrier.set(toSlash(c.carrierPath), toSlash(dts));
      }
    }
  }

  // Step 2 — type-check the leaf (no-emit) through the same patched host. The
  // referenced `.vue` carriers now resolve to their emitted `.d.ts`.
  const leafDiags = runNoEmitMode(
    ts,
    leafGroup.generatedTsconfigPath,
    mirrorFs,
    emittedDtsForCarrier,
  );
  diags.push(...leafDiags);
  return diags;
}

/**
 * Whether `candidate` is `dir` itself or strictly inside it — a SEGMENT-aware
 * containment test (NOT a string `startsWith`, which would treat a sibling
 * `<dir>2/...` as inside `<dir>`). Uses `path.relative`: a containment holds iff
 * the relative path is empty (equal) OR is neither absolute nor escapes via a
 * leading `..` segment.
 *
 * Exported for the zero-working-tree guard's sibling-prefix characterization
 * test (a `<dir>2` sibling must NOT count as inside `<dir>`).
 */
export function isInsideDir(dir: string, candidate: string): boolean {
  const rel = path.relative(toSlash(path.resolve(dir)), toSlash(path.resolve(candidate)));
  if (rel === "") return true;
  const relSlashed = toSlash(rel);
  return !path.isAbsolute(rel) && relSlashed !== ".." && !relSlashed.startsWith("../");
}

/**
 * Throw if a write target is outside the mirror root (zero-working-tree rail).
 *
 * Uses segment-aware containment (`path.relative`) so a SIBLING directory that
 * merely shares the mirror's name prefix (`<mirror>2/file`) is REJECTED — a raw
 * `startsWith` would have let it through and corrupted the no-write boundary.
 */
function assertInsideMirror(mirrorRoot: string, fileName: string): void {
  if (!isInsideDir(mirrorRoot, fileName)) {
    throw new Error(`verter batch-tsc: refusing to write outside the mirror root: ${fileName}`);
  }
}

/**
 * Reject a caller-supplied mirror root that collides with the user's working
 * tree — the zero-working-tree-writes boundary enforced IN the driver. The
 * mirror must be a Verter-owned directory that is NOT, and does not CONTAIN, and
 * is not CONTAINED BY, any project/source directory the batch reads. A collision
 * (the mirror equals or is inside a user-tree dir, OR a user-tree dir is inside
 * the mirror) would let the driver materialise carriers / emit into the user's
 * checkout. The default (auto-`mkdtemp`) root never reaches here.
 */
function assertMirrorRootOutsideUserTree(mirrorRoot: string, userTreeDirs: string[]): void {
  for (const dir of userTreeDirs) {
    if (isInsideDir(dir, mirrorRoot) || isInsideDir(mirrorRoot, dir)) {
      throw new Error(
        `verter batch-tsc: refusing a mirror root inside the user tree: ${mirrorRoot} ` +
          `(collides with ${dir}); the mirror must be a Verter-owned temp directory`,
      );
    }
  }
}

/**
 * The emitted `.d.ts` path for a referenced project's carrier, mapping its
 * mirror `rootDir` → `outDir` (the generated config sets `rootDir: "."`,
 * `outDir: ".verter-out"`), and swapping the `.ts`/`.tsx` extension for `.d.ts`.
 */
function emittedDeclarationPathFor(
  carrierPath: string,
  options: TS.CompilerOptions,
): string | undefined {
  const rootDir = options.rootDir;
  const outDir = options.outDir;
  if (rootDir === undefined || outDir === undefined) return undefined;
  const rel = toSlash(path.relative(toSlash(rootDir), toSlash(carrierPath)));
  if (rel.startsWith("..")) return undefined;
  const outPath = toSlash(path.join(toSlash(outDir), rel));
  return outPath.replace(/\.tsx?$/, ".d.ts");
}

/**
 * Parse a generated `extends`-tsconfig through the mirror FS (so `extends`
 * resolves and the carrier `files` are found). Returns the resolved
 * options/fileNames/references for the program/builder.
 */
function parseGeneratedConfig(
  ts: typeof TS,
  generatedTsconfigPath: string,
  mirrorFs: MirrorFs,
): TS.ParsedCommandLine {
  const parseHost: TS.ParseConfigFileHost = {
    useCaseSensitiveFileNames: ts.sys.useCaseSensitiveFileNames,
    readDirectory: ts.sys.readDirectory,
    fileExists: (f) => mirrorFs.fileExists(f),
    readFile: (f) => mirrorFs.readFile(f),
    getCurrentDirectory: () => ts.sys.getCurrentDirectory(),
    onUnRecoverableConfigFileDiagnostic: () => {
      /* collected via program diagnostics */
    },
  };
  const parsed = ts.getParsedCommandLineOfConfigFile(
    generatedTsconfigPath,
    /* optionsToExtend */ undefined,
    parseHost,
  );
  if (parsed === undefined) {
    return { options: {}, fileNames: [], errors: [] };
  }
  return parsed;
}

/**
 * Install the mirror read/redirection seam onto a `ts.CompilerHost`:
 * `readFile`/`fileExists`/`getSourceFile` serve carriers then fall through to
 * disk; `resolveModuleNames`/`resolveModuleNameLiterals` redirect framework
 * specifiers to their carriers.
 */
function patchCompilerHost(
  ts: typeof TS,
  compilerHost: TS.CompilerHost,
  options: TS.CompilerOptions,
  mirrorFs: MirrorFs,
  emittedDtsForCarrier?: Map<string, string>,
): void {
  const originalGetSourceFile = compilerHost.getSourceFile.bind(compilerHost);

  compilerHost.readFile = (fileName) => mirrorFs.readFile(fileName);
  compilerHost.fileExists = (fileName) => mirrorFs.fileExists(fileName);

  compilerHost.getSourceFile = (fileName, languageVersionOrOptions, onError, shouldCreate) => {
    const slash = toSlash(fileName);
    if (mirrorFs.isCarrier(slash)) {
      const text = mirrorFs.readFile(slash);
      if (text === undefined) return undefined;
      return ts.createSourceFile(fileName, text, languageVersionOrOptions, true);
    }
    return originalGetSourceFile(fileName, languageVersionOrOptions, onError, shouldCreate);
  };

  compilerHost.resolveModuleNames = makeResolveModuleNames(
    ts,
    options,
    mirrorFs,
    emittedDtsForCarrier,
  );
  // Newer TS prefers `resolveModuleNameLiterals`; provide both for parity across
  // `<7` minor versions (D7 — resolver-version independent).
  const resolveOne = makeResolveOne(ts, options, mirrorFs, emittedDtsForCarrier);
  compilerHost.resolveModuleNameLiterals = (
    moduleLiterals,
    containingFile,
    redirectedReference,
    optionsArg,
  ): readonly TS.ResolvedModuleWithFailedLookupLocations[] =>
    moduleLiterals.map((literal) => ({
      resolvedModule: resolveOne(literal.text, containingFile, optionsArg, redirectedReference),
    })) as readonly TS.ResolvedModuleWithFailedLookupLocations[];
}
