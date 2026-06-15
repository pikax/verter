/**
 * The immutable `MaterializedWorkspace` DTO and the orchestration that builds it.
 *
 * One physical workspace root holds two owned layers: B owns the immutable
 * Verter-facing scaffold (copied + anchor-stripped fixtures, deterministic tool
 * roots, the committed vendored Vue shim reference, the workspace settings), and
 * C owns ALL baseline materialization. This orchestration ties B's layer together
 * and CALLS C — it never duplicates C's work:
 *
 *   1. create a temp workspace root,
 *   2. copy authored fixtures into it, stripping test anchors and recording an
 *      anchor map keyed by globally-unique name,
 *   3. resolve deterministic tool roots,
 *   4. compute the expected vendored-Vue version from the committed shims,
 *   5. call C's `materialize` one-shot (which compiles `.vue`→TSX, emits twins,
 *      shifts source maps, injects `@verter/types`, synthesises tsconfig, copies
 *      the vendored shims, and runs the vendored-Vue version sync),
 *   6. write the workspace settings + extension-host env handoff.
 *
 * C's emitted `sourceMap`s are read as authoritative — B never recomputes them.
 */

import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, posix } from "node:path";

import { addFileAnchors, stripAnchors, type Anchor, type AnchorMap } from "./anchors.js";
import {
  buildMaterializeRequest,
  runMaterialize,
  type MaterializeResult,
  type MaterializeWireRequest,
} from "./baseline/materializeClient.js";
import { canonicalizePath, joinCanonical } from "./paths.js";
import { resolveToolRoots, type ToolRoots } from "./toolRoots.js";
import {
  buildVendorManifest,
  computeExpectedVueVersion,
  vendorShimsDir,
  type VendorManifest,
} from "./vendorManifest.js";
import { writeWorkspaceSettings, type WorkspaceSettings } from "./workspaceSettings.js";

/** The committed vendored shim reference carried on the workspace DTO. */
export interface VendorReference {
  /** Canonical vendored shim directory passed to C as `vendorNodeModules`. */
  readonly shimsDir: string;
  /** The Vue line every vendored package is pinned to. */
  readonly expectedVueVersion: string;
  /** Tamper-evident content manifest of the vendored shim tree. */
  readonly manifest: VendorManifest;
}

/**
 * The named TypeScript-config handoff: the resolved/copied project config and
 * the synthesized fallback C produced, lifted out of {@link MaterializeResult}
 * into a stable record so downstream drivers read a named contract rather than
 * scraping the C report's `tsconfigPath` / `synthesizedTsconfig` pair.
 */
export interface TsconfigSet {
  /**
   * The tsconfig C resolved for the baseline — a copied/resolved project config,
   * or the synthesized fallback when no project config was found. `null` when C
   * emitted no tsconfig at all.
   */
  readonly tsconfigPath: string | null;
  /** Whether {@link tsconfigPath} is a synthesized fallback (vs a copied/resolved project config). */
  readonly synthesized: boolean;
  /**
   * The copied/resolved project config path — non-`null` only when C used a real
   * project config (i.e. `synthesized` is `false`).
   */
  readonly projectConfigPath: string | null;
  /**
   * The synthesized fallback config path on disk — non-`null` only when C
   * synthesized a fallback (i.e. `synthesized` is `true`).
   */
  readonly synthesizedConfigPath: string | null;
}

/** Derive the named {@link TsconfigSet} handoff from C's materialization report. */
function tsconfigSetFromReport(report: MaterializeResult): TsconfigSet {
  const { tsconfigPath, synthesizedTsconfig: synthesized } = report;
  return {
    tsconfigPath,
    synthesized,
    projectConfigPath: synthesized ? null : tsconfigPath,
    synthesizedConfigPath: synthesized ? tsconfigPath : null,
  };
}

/** The immutable materialized-workspace scaffold + the C report reference. */
export interface MaterializedWorkspace {
  /** Canonical temp workspace root holding both owned layers. */
  readonly root: string;
  /** Relative (forward-slashed) paths of the stripped sources written in. */
  readonly sourceFiles: readonly string[];
  /**
   * Globally-unique anchor name → `{ file, line, character, encoding }`. Source
   * positions after the test-anchor strip, each carrying its column encoding
   * (`"utf-16"`) so a raw-LSP / extension consumer reads the unit off the DTO.
   */
  readonly anchorMap: ReadonlyMap<string, Anchor>;
  /** Deterministic, pinned tool roots. */
  readonly toolRoots: ToolRoots;
  /** Written `.vscode/settings.json` + the `DX_HARNESS_WORKSPACE` env handoff. */
  readonly workspaceSettings: WorkspaceSettings;
  /** The committed vendored Vue shim reference (manifest + expected version). */
  readonly vendor: VendorReference;
  /**
   * The named TypeScript-config handoff (resolved/copied project config plus the
   * synthesized fallback), derived from {@link materializeReport} so downstream
   * reads a stable contract instead of scraping the C report shape.
   */
  readonly tsconfigSet: TsconfigSet;
  /** C's materialization report — its `sourceMap`s are authoritative. */
  readonly materializeReport: MaterializeResult;
}

/** A function that runs C's `materialize` one-shot for a request. */
export type MaterializeRunner = (req: MaterializeWireRequest) => Promise<MaterializeResult>;

/** Options for {@link createMaterializedWorkspace}. */
export interface CreateMaterializedWorkspaceOptions {
  /** Directory of authored fixture sources (with anchors). */
  fixtureDir: string;
  /** Repository root used to resolve the deterministic tool roots. */
  repoRoot: string;
  /** Type provider pinned in the workspace settings (e.g. `"tsgo"`). */
  typeProvider?: string;
  /**
   * Whether a vendored-Vue version mismatch hard-fails. Unset ⇒ strict (the B↔C
   * contract default); pass `false` to downgrade vendored-Vue drift to a warning.
   */
  strictVueVersion?: boolean;
  /** Optional pinned `tsgo` binary. */
  tsgoBin?: string;
  /**
   * The materialize runner. Defaults to spawning C's binary at {@link baselineBin}.
   * Injected in tests so the orchestration runs without the Rust build.
   */
  materialize?: MaterializeRunner;
  /** Path to the built `verter-dx-baseline` binary (the default runner spawns it). */
  baselineBin?: string;
  /** Parent directory for the temp workspace root (defaults to the OS temp dir). */
  tmpRootParent?: string;
}

/** Source extensions whose anchors are stripped on copy. */
const TEXT_SOURCE = /\.(vue|ts|tsx|js|jsx|mts|cts)$/;

/**
 * Recursively freeze a plain object/array graph in place. Already-frozen nodes
 * (and primitives) are skipped, so cycles and shared subtrees terminate. Used to
 * make the materialized-workspace scaffold immutable once a scenario starts — the
 * DTO is shared across four drivers (raw-LSP, extension-host, baseline bridge,
 * semantic-oracle runner) and none may mutate it.
 */
function deepFreeze<T>(value: T): T {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }
  Object.freeze(value);
  for (const key of Object.keys(value as object)) {
    deepFreeze((value as Record<string, unknown>)[key]);
  }
  return value;
}

/**
 * Build a genuinely read-only anchor map: every {@link Anchor} value is frozen so
 * an entry cannot be mutated, and the map's own mutators throw so it cannot gain
 * or lose entries. The read surface (`get`/`has`/`size`/iteration) is unchanged.
 */
function freezeAnchorMap(source: AnchorMap): ReadonlyMap<string, Anchor> {
  const map = new Map<string, Anchor>();
  for (const [name, anchor] of source) map.set(name, Object.freeze({ ...anchor }));
  const block = (op: string) => (): never => {
    throw new TypeError(`anchorMap is read-only: ${op} is not permitted`);
  };
  Object.defineProperties(map, {
    set: { value: block("set") },
    delete: { value: block("delete") },
    clear: { value: block("clear") },
  });
  return Object.freeze(map);
}

function walkFiles(root: string, rel: string, out: string[]): void {
  const here = rel === "" ? root : join(root, rel);
  for (const entry of readdirSync(here, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
    const childRel = rel === "" ? entry.name : posix.join(rel, entry.name);
    if (entry.isDirectory()) walkFiles(root, childRel, out);
    else if (entry.isFile()) out.push(childRel);
  }
}

function defaultRunner(baselineBin?: string): MaterializeRunner {
  return (req) => {
    if (!baselineBin) {
      throw new Error(
        "createMaterializedWorkspace requires a `baselineBin` (built verter-dx-baseline) " +
          "or an injected `materialize` runner",
      );
    }
    return runMaterialize(baselineBin, req);
  };
}

/**
 * Build a {@link MaterializedWorkspace}: scaffold the temp root, copy + strip
 * fixtures, resolve tool roots, compute the vendored-Vue version, call C's
 * materializer, and write the workspace settings.
 */
export async function createMaterializedWorkspace(
  opts: CreateMaterializedWorkspaceOptions,
): Promise<MaterializedWorkspace> {
  const root = canonicalizePath(mkdtempSync(join(opts.tmpRootParent ?? tmpdir(), "dx-ws-")));

  // The temp root exists now; if anything below throws, remove it before
  // rethrowing so a failed scenario never leaks a temp dir. Only a successful
  // return keeps the root (released later via disposeMaterializedWorkspace).
  try {
    // Copy + strip authored fixtures, merging a globally-unique anchor map.
    const rels: string[] = [];
    walkFiles(opts.fixtureDir, "", rels);
    rels.sort();
    const anchorMap: AnchorMap = new Map();
    const sourceFiles: string[] = [];
    for (const rel of rels) {
      const srcAbs = join(opts.fixtureDir, rel);
      const dstAbs = joinCanonical(root, rel);
      mkdirSync(dirname(dstAbs), { recursive: true });
      if (TEXT_SOURCE.test(rel)) {
        const result = stripAnchors(readFileSync(srcAbs, "utf-8"));
        writeFileSync(dstAbs, result.stripped, "utf-8");
        addFileAnchors(anchorMap, rel, result); // throws on a cross-file duplicate
      } else {
        copyFileSync(srcAbs, dstAbs);
      }
      sourceFiles.push(rel);
    }

    const toolRoots = resolveToolRoots(opts.repoRoot, { tsgoBin: opts.tsgoBin });

    const shimsDir = vendorShimsDir();
    const expectedVueVersion = computeExpectedVueVersion(shimsDir);
    const manifest = buildVendorManifest(shimsDir);

    const entries = sourceFiles
      .filter((f) => f.endsWith(".vue"))
      .map((f) => joinCanonical(root, f));

    const request = buildMaterializeRequest({
      workspaceRoot: root,
      entries,
      vendorNodeModules: shimsDir,
      expectedVueVersion,
      // Forward the optional unchanged so `buildMaterializeRequest` owns the single
      // strict-by-default policy; an unset caller hard-fails on vendored-Vue drift.
      strictVueVersion: opts.strictVueVersion,
    });
    const run = opts.materialize ?? defaultRunner(opts.baselineBin);
    const materializeReport = await run(request);

    const workspaceSettings = writeWorkspaceSettings(root, {
      tsdk: toolRoots.tsserverTsdk,
      typeProvider: opts.typeProvider,
    });

    // Deep-freeze the whole scaffold so it is immutable once a scenario starts; the
    // anchor map is frozen separately so its entries and structure are read-only.
    return deepFreeze({
      root,
      sourceFiles: [...sourceFiles],
      anchorMap: freezeAnchorMap(anchorMap),
      toolRoots,
      workspaceSettings,
      vendor: { shimsDir, expectedVueVersion, manifest },
      tsconfigSet: tsconfigSetFromReport(materializeReport),
      materializeReport,
    }) as MaterializedWorkspace;
  } catch (err) {
    rmSync(root, { recursive: true, force: true });
    throw err;
  }
}

/** Remove a materialized workspace's temp root (best-effort). */
export function disposeMaterializedWorkspace(ws: MaterializedWorkspace): void {
  rmSync(ws.root, { recursive: true, force: true });
}
