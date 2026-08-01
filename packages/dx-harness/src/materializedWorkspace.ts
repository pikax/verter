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
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, posix, resolve } from "node:path";

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

/**
 * Remove the baseline's ON-DISK generated layer from a materialized workspace,
 * returning the absolute paths actually deleted.
 *
 * C materializes the baseline's artifacts as REAL FILES beside the authored
 * carriers (`App.vue` → `App.vue.tsx` entry + `App.vue.ts` public-API twin), which
 * is exactly what the tsserver-backed baseline bridge needs to type-check.
 * `verter-lsp` OWNS that companion namespace: under the project-bound external-TS
 * contract a real user file sitting at a carrier's companion path is a detected
 * resolution conflict, so the server marks the carrier ambiguous and fails closed
 * (`CarrierPathOccupiedByRealFile`) rather than shadowing what looks like a user's
 * own file. A driver that points `verter-lsp` at a workspace C materialized
 * therefore gets NO TypeScript semantics for any carrier in it — every provider
 * signal comes back empty — until this generated layer is pruned.
 *
 * The report is the authority for what to delete: every `generatedPath` C
 * actually emitted, never a companion name this layer re-derives. A path outside
 * {@link MaterializedWorkspace.root} is refused loudly — pruning is scoped to the
 * temp workspace and must never reach a caller's real files.
 *
 * @throws {Error} if a reported artifact path lies outside `ws.root`.
 */
export function pruneBaselineGeneratedArtifacts(ws: MaterializedWorkspace): readonly string[] {
  // Containment must be decided on the path `rmSync` will actually act on, which
  // means the PHYSICAL path — lexical normalisation is not enough in either
  // direction:
  //
  //   - `path.resolve` alone collapses `..` but follows no links, so
  //     `${root}/link/victim` stays lexically inside while `link` points out of the
  //     workspace and the deletion lands outside. pnpm produces exactly that shape
  //     throughout this repo (`node_modules/.pnpm/node_modules/@verter/x` →
  //     `packages/x`), so it is a real layout, not a contrived one.
  //   - conversely, comparing unresolved spellings produces FALSE refusals when the
  //     two sides spell the same directory differently (macOS `/tmp` →
  //     `/private/tmp`).
  //
  // So the CHECK resolves links (see `containingDirectoryIsInside`) while the
  // DELETION uses the unresolved requested path — deliberately not one path for
  // both. Resolving the path handed to `rmSync` would follow a symlinked ARTIFACT
  // to its target and delete that instead of the link; leaving it unresolved makes
  // `rmSync` unlink the link itself. The materializer is an injectable interface,
  // so a reported path is untrusted input, not a fixed value.
  const rootPrefix = `${canonicalizePath(realpathSync(resolve(ws.root))).replace(/\/+$/, "")}/`;
  const removed: string[] = [];
  const reported = [...ws.materializeReport.ideArtifacts, ...ws.materializeReport.publicApiTwins];
  for (const artifact of reported) {
    const requested = resolve(artifact.generatedPath);
    if (!containingDirectoryIsInside(requested, rootPrefix)) {
      throw new Error(
        `materialize reported a generated artifact outside the workspace root: ` +
          `${artifact.generatedPath} is not under ${ws.root}`,
      );
    }
    // `existsSync` before `rmSync` so the return value is evidence of a real
    // deletion: a caller gating on `removed.length` must not be satisfied by an
    // artifact the report named but C never wrote.
    if (!existsSync(requested)) continue;
    // Delete the REQUESTED path, never a realpathed one. `rmSync` unlinks a symlink
    // rather than following it, so a reported `A.vue.tsx -> A.vue` removes the link
    // and leaves the authored file intact; handing `rmSync` the resolved target
    // would delete `A.vue` itself.
    rmSync(requested, { force: true });
    removed.push(canonicalizePath(requested));
  }
  return removed;
}

/**
 * Whether `candidate` physically resides inside `rootPrefix` (a canonical,
 * realpathed, trailing-slash directory prefix).
 *
 * Containment is decided on the candidate's DIRECTORY, resolved through symlinks;
 * the final segment is deliberately left unresolved, because `rmSync` unlinks it
 * rather than following it. Resolving the final segment would both mis-locate a
 * symlink artifact and license deleting its target.
 *
 * The directory is found by realpathing the DEEPEST EXISTING ancestor and
 * re-attaching the remaining segments. Realpathing only the immediate parent is not
 * enough: a report may legitimately name an artifact under directories C never
 * created, and if the root itself is reached through a symlinked spelling (macOS
 * `/tmp` → `/private/tmp`) the unresolved ancestors then compare unequal and a
 * perfectly contained path is refused.
 */
function containingDirectoryIsInside(candidate: string, rootPrefix: string): boolean {
  let existing = dirname(candidate);
  const trailing: string[] = [];
  while (!existsSync(existing)) {
    const parent = dirname(existing);
    // `dirname` is idempotent at the filesystem root; stop rather than loop.
    if (parent === existing) return false;
    trailing.unshift(basename(existing));
    existing = parent;
  }
  const directory = join(realpathSync(existing), ...trailing);
  return `${canonicalizePath(directory).replace(/\/+$/, "")}/`.startsWith(rootPrefix);
}

/** Remove a materialized workspace's temp root (best-effort). */
export function disposeMaterializedWorkspace(ws: MaterializedWorkspace): void {
  rmSync(ws.root, { recursive: true, force: true });
}
