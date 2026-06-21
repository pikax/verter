#!/usr/bin/env node
/**
 * Generator — Svelte helper-topology reference goldens.
 *
 * Runs the PINNED official `svelte@5.56.3` compiler over the vendored
 * `.svelte` corpus and writes committed, NORMALIZED golden files that pin
 * STRUCTURE + helper-call TOPOLOGY — NOT bytes. The goldens are the pinned
 * Svelte reference the drift gate compares against; byte identity is the bar
 * nowhere. (When the native-Svelte runtime codegen lands it will diff its own
 * emitted output against these same goldens — a follow-up conformance use.)
 *
 * This mirrors the `scripts/gen-corpus-audit-tests.mjs` and
 * `scripts/generate-svelte-bind-contract.mjs` patterns: one idempotent
 * command rewrites every committed golden, and a `--check` mode re-runs the
 * compiler, re-normalizes, and asserts the committed goldens equal the fresh
 * normalized output (non-zero exit on drift). The script is the ONLY way the
 * goldens change — goldens are never hand-edited.
 *
 * ## What is normalized (topology, not bytes)
 *
 * For each `.svelte` fixture, for each backend (`client` + `server`):
 *   - `backend`               — `client` | `server`.
 *   - `imports`               — sorted import topology: the bare side-effect
 *                               imports (`import 'svelte/internal/flags/…'`),
 *                               the `import * as $ from 'svelte/internal/{…}'`
 *                               runtime namespace, and named/default module
 *                               imports — names + sources, var-rename-stable.
 *   - `exportDefault`         — the component function shape: `{ name, params }`
 *                               (the param identifier list, e.g. `$$anchor`,
 *                               `$$props` / `$$renderer`, `$$props`).
 *   - `helperSequence`        — the ORDERED list of `$.<helper>` names as they
 *                               appear in the module (the call topology). This
 *                               is the load-bearing oracle: helper families
 *                               used + the order they are emitted.
 *   - `helperSet`             — the sorted UNIQUE helper names (the family set).
 *   - `helperCounts`          — per-helper occurrence count (call-shape).
 *   - `templates`             — the `from_html` / `from_svg` / `from_mathml`
 *                               skeleton template literals (client) with the
 *                               `svelte-<hash>` scoped class MASKED to
 *                               `svelte-<scoped>` (presence preserved, the
 *                               per-build hash byte-noise removed), and the
 *                               multi-root FRAGMENT flag (`1` / absent).
 *   - `css`                   — `{ present, hash, code }`: whether the fixture
 *                               emitted scoped CSS, the extracted `svelte-<hash>`
 *                               scope hash (topology — the SAME hash must reach
 *                               both backends and the template), and the
 *                               normalized scoped CSS code with the hash masked.
 *
 * Whitespace, indentation, and local variable-name noise (`text`, `text_1`,
 * `node`, `var fragment`, …) are intentionally NOT pinned — they are formatting
 * the drift bar does not require. Helper FAMILIES, the import set, the
 * export shape, the template skeletons, the scope-hash topology, and the
 * per-backend decisions ARE pinned.
 *
 * ## Usage
 *
 *     node scripts/gen-svelte-goldens.mjs            # rewrite all goldens
 *     node scripts/gen-svelte-goldens.mjs --check    # assert goldens in sync
 *     node scripts/gen-svelte-goldens.mjs --emit-dir=<dir>
 *                                                    # write fresh normalized
 *                                                    # output to <dir> WITHOUT
 *                                                    # touching the committed
 *                                                    # goldens — the feature-gated
 *                                                    # Rust oracle harness consumes
 *                                                    # this to drive the topology diff.
 *     node scripts/gen-svelte-goldens.mjs --check --goldens-dir=<dir>
 *                                                    # check a COPY of the goldens
 *                                                    # at <dir> instead of the
 *                                                    # committed tree — the
 *                                                    # feature-gated drift
 *                                                    # discrimination self-test
 *                                                    # corrupts a temp copy and
 *                                                    # asserts a non-zero exit.
 *
 * ## Hermeticity
 *
 * `svelte` is a pinned devDependency, so this script (and the `--check` guard)
 * MAY read it from `node_modules`. The DEFAULT canonical Rust run checks the
 * COMMITTED goldens only — the live-compiler oracle harness is feature-gated
 * (`svelte-oracle`) and excluded from the default workspace test set.
 *
 * The pinned version is the single source of truth in `SVELTE_ORACLE_VERSION`
 * below; the Rust guard `svelte_lockfile_matches_oracle_pin` asserts the
 * resolved `pnpm-lock.yaml` version equals it, and the hermetic guard
 * `committed_svelte_goldens_match_oracle_pin` asserts every committed golden's
 * `oracleVersion` equals it (so a `svelte` bump that leaves STALE goldens
 * fails). A version bump is a reviewed delta: re-pin here, bump the lockfile,
 * run this script (which restamps every golden), review the diff.
 */

import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
  existsSync,
} from "node:fs";
import { dirname, join, parse as parsePath, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The shared topology-extraction primitives — the single source of truth both
// Svelte golden generators consume (this hand-vendored corpus + the generated
// differential corpus). The extractors here are byte-equivalent to the logic
// this generator previously inlined; they were lifted into the shared lib so a
// normalization fix lands once for both corpora.
import {
  extractDelegatedEvents,
  extractExportDefault,
  extractImports,
  extractScopeHash,
  extractTemplates,
  helperCountsOf,
  helperSequenceOf,
  loadPinnedCompiler as loadPinnedCompilerFrom,
  maskScopeHash,
  normalizeModuleForComparison,
} from "./svelte-golden-lib.mjs";

// ---------------------------------------------------------------------------
// Pin constant — the single source of truth for the oracle version. The Rust
// guard `svelte_lockfile_matches_oracle_pin` asserts the resolved
// `pnpm-lock.yaml` `svelte@<version>` equals this exact string, and the
// hermetic guard `committed_svelte_goldens_match_oracle_pin` asserts every
// committed golden's `oracleVersion` equals it. A `svelte` bump is therefore a
// reviewed delta: re-pin here, bump the lockfile, regenerate the goldens.
// ---------------------------------------------------------------------------
export const SVELTE_ORACLE_VERSION = "5.56.3";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "..");

// Vendored corpus + committed goldens. The corpus is locally vendored so the
// default Rust run (which checks the goldens) is hermetic; the goldens are
// regenerated mechanically from the pinned compiler.
const ORACLE_ROOT = resolve(REPO_ROOT, "crates/verter_compiler/tests/svelte_oracle_corpus");
const FIXTURES_DIR = join(ORACLE_ROOT, "fixtures");
const GOLDENS_DIR = join(ORACLE_ROOT, "goldens");

// The top-level `generated/` subtree of `fixtures/` and `goldens/` is owned by
// the SEPARATE generated differential corpus generator
// (`scripts/gen-svelte-diff-corpus.mjs`), which writes the EXPANDED schema there.
// This hand-vendored generator owns everything EXCEPT that subtree — it skips it
// in discovery, never deletes it in write mode, and ignores it in the
// stale-orphan walk, so the two generators never collide.
const GENERATED_SUBDIR = "generated";

// Marker file the generator drops at the root of every EMIT dir it creates.
// An EXISTING non-empty emit target is accepted only when it carries this
// sentinel — proving the generator itself produced it — so a mistyped path to
// any pre-existing tree (including a committed JSON-only snapshot dir elsewhere
// in the repo) can never be the destructive target. The name is hidden + tool-
// specific so it cannot collide with a real output leaf.
const EMIT_SENTINEL = ".svelte-oracle-emit";

// The backends captured: the official CLIENT corpus + the SERVER (SSR) pass.
// Both are derived from the pinned compiler and normalized into goldens by the
// same generator pass.
const BACKENDS = ["client", "server"];

// ---------------------------------------------------------------------------
// Destructive-path safety
// ---------------------------------------------------------------------------

/** Path separator for `p` (Windows backslash or POSIX forward slash). */
function pathSep(p) {
  return p.includes("\\") ? "\\" : "/";
}

/**
 * True when `child` IS `ancestor` or is nested anywhere beneath it. The
 * `${ancestor}${sep}` boundary guard prevents a sibling like `<dir>-other`
 * from being mistaken for a path inside `<dir>`.
 */
function isPathInsideOrEqual(child, ancestor) {
  if (child === ancestor) return true;
  const sep = pathSep(ancestor);
  return `${child}${sep}`.startsWith(`${ancestor}${sep}`);
}

/**
 * Validate that `dir` is a safe target for a recursive `rmSync` BEFORE any
 * destructive call. The generator only ever recursively deletes an isolated
 * output target: in WRITE mode that target is EXACTLY the committed
 * `GOLDENS_DIR`; in EMIT mode it is a throwaway tree OUTSIDE the repository —
 * nonexistent, empty, or a prior emit dir the generator stamped with the
 * `EMIT_SENTINEL` marker. A mistyped / empty / hostile argument must never let
 * it wipe the checkout, the filesystem root, the corpus, the committed
 * goldens, or — the extension-only gap this guard now closes — any pre-existing
 * JSON-only snapshot directory committed elsewhere in the repo.
 *
 * Rejects:
 *   - an empty / whitespace-only resolved path (an empty `--emit-dir=` resolves
 *     to the cwd — from the repo root that would delete the checkout);
 *   - the filesystem root;
 *   - the repo root, or any ancestor of it (deleting it would wipe the
 *     checkout);
 *   - the corpus tree (`ORACLE_ROOT` / `FIXTURES_DIR`) — fixtures are inputs,
 *     never destructive targets;
 *   - in EMIT mode, ANY path inside the repository — emit writes a throwaway
 *     tree that lives OUTSIDE the checkout; the committed goldens are the sole
 *     in-repo destructive target and they belong to write mode alone, so an
 *     in-repo emit target (the committed goldens, a descendant like
 *     `goldens/components`, or any other committed snapshot dir) is refused by
 *     containment, never by an extension heuristic;
 *   - in WRITE mode, any target other than EXACTLY `GOLDENS_DIR` — write mode
 *     is the sole owner of the committed goldens and writes nowhere else;
 *   - an existing path that is not a directory, or — for an EMIT target — an
 *     existing non-empty directory that does NOT carry the generator-owned
 *     `EMIT_SENTINEL` marker at its root. An existing emit target is therefore
 *     accepted only when it is empty or a tree the generator itself produced;
 *     a directory whose leaves merely happen to all be `.json` is NOT accepted.
 *
 * @param {string} dir          resolved absolute path
 * @param {{ role: "write" | "emit" }} opts
 */
function assertSafeDestructiveDir(dir, opts) {
  const role = opts.role;
  if (typeof dir !== "string" || dir.trim() === "") {
    throw new Error(
      `refusing destructive rmSync: the resolved ${role} directory is empty. ` +
        `Pass an explicit isolated output directory (an empty \`--emit-dir=\` ` +
        `resolves to the current working directory and is rejected).`,
    );
  }
  const target = resolve(dir);
  const fsRoot = parsePath(target).root;
  if (target === fsRoot) {
    throw new Error(`refusing destructive rmSync on the filesystem root ${target}`);
  }
  // `${REPO_ROOT}${sep}` guards the boundary so a sibling like `<repo>-other`
  // is not treated as inside the repo, while `<repo>` itself and any path
  // *above* it (an ancestor whose deletion takes the checkout with it) are
  // rejected.
  const sep = pathSep(target);
  if (target === REPO_ROOT || `${REPO_ROOT}${sep}`.startsWith(`${target}${sep}`)) {
    throw new Error(
      `refusing destructive rmSync on ${target}: it is the repo root or an ` +
        `ancestor of it — deleting it would wipe the checkout. Target an ` +
        `isolated output directory instead.`,
    );
  }
  if (target === ORACLE_ROOT || target === FIXTURES_DIR) {
    throw new Error(
      `refusing destructive rmSync on the corpus tree ${target}: fixtures are ` +
        `inputs, never destructive targets.`,
    );
  }

  if (role === "write") {
    // Write mode is the SOLE owner of the committed goldens and writes nowhere
    // else: the only legal destructive target is EXACTLY `GOLDENS_DIR`.
    if (target !== GOLDENS_DIR) {
      throw new Error(
        `refusing destructive rmSync on ${target} in write mode: write mode owns ` +
          `exactly the committed goldens directory ${GOLDENS_DIR} and writes ` +
          `nowhere else. (Use --emit-dir for a throwaway tree.)`,
      );
    }
    return;
  }

  // EMIT mode: the throwaway tree must live entirely OUTSIDE the checkout. The
  // committed goldens are the only legitimate in-repo destructive target and
  // they belong to write mode alone, so ANY path inside the repo — the goldens
  // dir, a descendant like `goldens/components`, or any other committed
  // snapshot directory — is refused. This containment check (not an extension
  // heuristic) is what stops a mistyped `--emit-dir` from wiping committed
  // JSON-only data anywhere in the tree.
  if (isPathInsideOrEqual(target, REPO_ROOT)) {
    throw new Error(
      `refusing to --emit-dir into ${target}: it is inside the repository. ` +
        `--emit-dir writes a throwaway tree OUTSIDE the checkout (the committed ` +
        `goldens are owned by write mode — run without --emit-dir to rewrite ` +
        `them). Point --emit-dir at an isolated directory outside the repo.`,
    );
  }
  if (existsSync(target)) {
    const st = statSync(target);
    if (!st.isDirectory()) {
      throw new Error(`refusing destructive rmSync on ${target}: it is not a directory.`);
    }
    // An EXISTING non-empty emit target is acceptable only when the generator
    // itself created it, proven by the `EMIT_SENTINEL` marker at its root. An
    // all-`.json` tree no longer qualifies on extension alone — a committed
    // snapshot directory full of JSON must never be mistaken for a prior emit
    // dir and recursively deleted.
    if (!isEmptyDir(target) && !existsSync(join(target, EMIT_SENTINEL))) {
      throw new Error(
        `refusing destructive rmSync on ${target}: it is a non-empty directory ` +
          `that does not carry the generator-owned \`${EMIT_SENTINEL}\` marker, ` +
          `so the generator did not create it. Target a non-existent / empty ` +
          `directory, or a prior emit dir the generator stamped.`,
      );
    }
  }
}

/** True when `dir` has no entries. */
function isEmptyDir(dir) {
  return readdirSync(dir).length === 0;
}

// ---------------------------------------------------------------------------
// Pinned-compiler loader
// ---------------------------------------------------------------------------

/**
 * Resolve the pinned svelte compiler (delegates to the shared loader, rooted at
 * this generator's `REPO_ROOT`). Pins the EXACT version directory under pnpm so
 * a different installed `svelte` cannot silently satisfy the oracle.
 */
function loadPinnedCompiler() {
  return loadPinnedCompilerFrom(REPO_ROOT);
}

// ---------------------------------------------------------------------------
// Corpus discovery
// ---------------------------------------------------------------------------

/**
 * Recursively collect `.svelte` fixtures, sorted lexicographically. The
 * top-level `generated/` subtree (owned by the differential-corpus generator) is
 * skipped — this generator never produces goldens for it.
 */
function discoverFixtures(dir) {
  const out = [];
  const walk = (d) => {
    const entries = readdirSync(d, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    );
    for (const e of entries) {
      const p = join(d, e.name);
      if (e.isDirectory()) {
        // Skip the differential-corpus subtree directly under `fixtures/`.
        if (d === dir && e.name === GENERATED_SUBDIR) continue;
        walk(p);
      } else if (e.isFile() && e.name.endsWith(".svelte")) {
        out.push(p);
      }
    }
  };
  walk(dir);
  out.sort();
  return out;
}

/** The fixture's stable slug: its path relative to `fixtures/`, `/`-joined. */
function fixtureSlug(absPath) {
  return relative(FIXTURES_DIR, absPath).split("\\").join("/");
}

/** Derive the component `name` compile option from the fixture filename stem. */
function componentNameFor(slug) {
  const stem = slug
    .replace(/\.svelte$/, "")
    .split("/")
    .pop();
  const sanitized = stem.replace(/[^A-Za-z0-9_$]/g, "_");
  return /^[A-Za-z_$]/.test(sanitized) ? sanitized : `_${sanitized}`;
}

// ---------------------------------------------------------------------------
// Normalization (topology, not bytes)
// ---------------------------------------------------------------------------

// The topology extractors (scope-hash masking, non-code masking,
// helperSequenceOf, extractImports, extractExportDefault, extractTemplates,
// extractDelegatedEvents) live in ./svelte-golden-lib.mjs (imported above) — the
// single source of truth shared with scripts/gen-svelte-diff-corpus.mjs.

/** Normalize one compiled fixture to its topology golden object. */
function normalize(slug, backend, compiled) {
  const code = compiled.js.code;
  const helperSequence = helperSequenceOf(code);
  const helperSet = [...new Set(helperSequence)].sort();
  const helperCounts = helperCountsOf(helperSequence);

  const cssCode = compiled.css && compiled.css.code ? compiled.css.code : null;
  const css = {
    present: !!cssCode,
    hash: cssCode ? extractScopeHash(cssCode) : null,
    code: cssCode ? maskScopeHash(cssCode) : null,
  };

  return {
    slug,
    backend,
    oracleVersion: SVELTE_ORACLE_VERSION,
    imports: extractImports(code),
    exportDefault: extractExportDefault(code),
    helperSequence,
    helperSet,
    helperCounts,
    // The ordered delegated event-type set (the module `$.delegate([...])`
    // declaration) — client backend only (the server does no event delegation).
    delegatedEvents: backend === "client" ? extractDelegatedEvents(code) : [],
    templates: backend === "client" ? extractTemplates(code) : [],
    // The FULL normalized official module (client backend only) — the
    // argument/offset/identifier-precise oracle the emitted-JS topology gate
    // compares Verter's normalized output against (cosmetic whitespace collapsed
    // OUTSIDE literals; literal/template TEXT preserved byte-exact). The server
    // backend omits it (the client gate is the only consumer today).
    clientModule: backend === "client" ? normalizeModuleForComparison(code) : null,
    css,
  };
}

// ---------------------------------------------------------------------------
// Golden file IO
// ---------------------------------------------------------------------------

/** The golden path for a fixture/backend pair, rooted under `goldensDir`. */
function goldenPathFor(goldensDir, slug, backend) {
  const rel = slug.replace(/\.svelte$/, "");
  return join(goldensDir, `${rel}.${backend}.json`);
}

/** Deterministic, stable JSON serialization (trailing newline). */
function serializeGolden(obj) {
  return JSON.stringify(obj, null, 2) + "\n";
}

/**
 * Compile + normalize every fixture/backend pair into `{ path -> content }`,
 * with each path rooted under `goldensDir` (defaults to the committed tree).
 */
function buildAllGoldens(compiler, goldensDir = GOLDENS_DIR) {
  const fixtures = discoverFixtures(FIXTURES_DIR);
  if (fixtures.length === 0) {
    throw new Error(`no .svelte fixtures found under ${FIXTURES_DIR}`);
  }
  const result = new Map();
  for (const abs of fixtures) {
    const slug = fixtureSlug(abs);
    const source = readFileSync(abs, "utf8");
    const name = componentNameFor(slug);
    for (const backend of BACKENDS) {
      let compiled;
      try {
        compiled = compiler.compile(source, {
          generate: backend,
          filename: slug,
          name,
        });
      } catch (err) {
        throw new Error(`svelte compile failed for ${slug} (${backend}): ${err.message}`);
      }
      const golden = normalize(slug, backend, compiled);
      result.set(goldenPathFor(goldensDir, slug, backend), serializeGolden(golden));
    }
  }
  return result;
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

function writeMode(compiler) {
  const goldens = buildAllGoldens(compiler);
  // Clean rewrite: drop this generator's goldens, then write fresh. Idempotent.
  // The top-level `generated/` subtree is PRESERVED (it is owned by the
  // differential-corpus generator), so only the hand-vendored top-level entries
  // are removed.
  assertSafeDestructiveDir(GOLDENS_DIR, { role: "write" });
  if (existsSync(GOLDENS_DIR)) {
    for (const e of readdirSync(GOLDENS_DIR, { withFileTypes: true })) {
      if (e.name === GENERATED_SUBDIR) continue;
      rmSync(join(GOLDENS_DIR, e.name), { recursive: true, force: true });
    }
  }
  for (const [path, content] of [...goldens].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  console.log(
    `gen-svelte-goldens: wrote ${goldens.size} golden(s) from svelte@${SVELTE_ORACLE_VERSION} ` +
      `into ${relative(REPO_ROOT, GOLDENS_DIR)}`,
  );
}

function checkMode(compiler, goldensDir = GOLDENS_DIR) {
  const fresh = buildAllGoldens(compiler, goldensDir);
  const drift = [];

  // Detect drifted / missing goldens.
  for (const [path, content] of fresh) {
    const rel = relative(REPO_ROOT, path);
    if (!existsSync(path)) {
      drift.push(`MISSING golden: ${rel}`);
      continue;
    }
    const committed = readFileSync(path, "utf8");
    if (committed !== content) {
      drift.push(`DRIFTED golden (on-disk != regenerated): ${rel}`);
    }
  }

  // Detect stale goldens with no corresponding fixture/backend. The top-level
  // `generated/` subtree is owned by the differential-corpus generator and is
  // skipped here (its own `--check` validates it).
  const expected = new Set([...fresh.keys()]);
  const collectCommitted = (dir) => {
    if (!existsSync(dir)) return;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        if (dir === goldensDir && e.name === GENERATED_SUBDIR) continue;
        collectCommitted(p);
      } else if (e.isFile() && e.name.endsWith(".json") && !expected.has(p)) {
        drift.push(`STALE golden (no fixture/backend): ${relative(REPO_ROOT, p)}`);
      }
    }
  };
  collectCommitted(goldensDir);

  if (drift.length > 0) {
    console.error(
      `gen-svelte-goldens --check: the Svelte goldens are out of sync with ` +
        `the pinned svelte@${SVELTE_ORACLE_VERSION} compiler.\n` +
        drift.map((d) => `  - ${d}`).join("\n") +
        `\n\nRegenerate with \`node scripts/gen-svelte-goldens.mjs\` and review the diff ` +
        `as the oracle delta. Do NOT hand-edit the goldens.`,
    );
    process.exit(1);
  }
  console.log(
    `gen-svelte-goldens --check: ${fresh.size} golden(s) in sync with svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/**
 * Emit fresh normalized output into an arbitrary directory WITHOUT touching
 * the committed goldens. The feature-gated Rust oracle harness consumes this
 * to drive the normalized topology diff against the committed goldens.
 */
function emitMode(compiler, emitDir) {
  const goldens = buildAllGoldens(compiler);
  assertSafeDestructiveDir(emitDir, { role: "emit" });
  rmSync(emitDir, { recursive: true, force: true });
  // Stamp the generator-owned marker FIRST so a later re-emit into the same
  // dir is recognised as a prior emit tree (and so accepted) by the safety
  // guard above, while no other tool's directory ever carries it.
  mkdirSync(emitDir, { recursive: true });
  writeFileSync(join(emitDir, EMIT_SENTINEL), "");
  for (const [path, content] of goldens) {
    // Re-root the committed path under emitDir, preserving the relative layout.
    const rel = relative(GOLDENS_DIR, path);
    const target = join(emitDir, rel);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, content);
  }
  console.log(
    `gen-svelte-goldens --emit-dir: wrote ${goldens.size} normalized output(s) ` +
      `into ${emitDir}`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const emitArg = process.argv.find((a) => a.startsWith("--emit-dir="));
  const goldensDirArg = process.argv.find((a) => a.startsWith("--goldens-dir="));
  const compiler = loadPinnedCompiler();
  if (emitArg) {
    const rawEmit = emitArg.slice("--emit-dir=".length).trim();
    if (rawEmit === "") {
      throw new Error(
        "refusing to run: `--emit-dir=` requires an explicit isolated output " +
          "directory. An empty value resolves to the current working directory " +
          "(from the repo root that would target the checkout for deletion).",
      );
    }
    emitMode(compiler, resolve(rawEmit));
  } else if (check) {
    // `--goldens-dir=<dir>` checks a COPY of the goldens (the drift
    // discrimination self-test); without it, the committed tree is checked.
    const goldensDir = goldensDirArg
      ? resolve(goldensDirArg.slice("--goldens-dir=".length))
      : GOLDENS_DIR;
    checkMode(compiler, goldensDir);
  } else {
    writeMode(compiler);
  }
}

main();
