#!/usr/bin/env node
/**
 * Generator — Svelte helper-topology reference goldens.
 *
 * Runs the PINNED official `svelte@5.56.3` compiler over the vendored
 * `.svelte` corpus and writes committed, NORMALIZED golden files. The goldens
 * are the pinned Svelte reference the drift gate and the native runtime
 * codegen conformance gates compare against. Cosmetic JS CARRIER formatting
 * is WAIVED (whitespace/indentation outside literals, local variable-name
 * noise) — but the goldens DO pin bytes where bytes are the contract: the
 * CSS payload bytes (`css.code`, the injected `$$css` literal inside
 * `clientModule`), the scope-hash TOPOLOGY (masked value, pinned placement),
 * the helper-call argument row, the static-HTML skeleton template strings,
 * and every other literal preserved by `normalizeModuleForComparison`.
 *
 * This mirrors the `scripts/gen-corpus-audit-tests.mjs` and
 * `scripts/generate-svelte-bind-contract.mjs` patterns: one idempotent
 * command rewrites every committed golden, and a `--check` mode re-runs the
 * compiler, re-normalizes, and asserts the committed goldens equal the fresh
 * normalized output (non-zero exit on drift). The script is the ONLY way the
 * goldens change — goldens are never hand-edited.
 *
 * ## What is normalized (carrier formatting waived; contractual bytes pinned)
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
 *                               scope hash (the SAME hash must reach both
 *                               backends and the template), and the scoped CSS
 *                               code BYTES with only the hash masked — the
 *                               `css.code` payload is byte-contractual.
 *
 * Whitespace, indentation, and local variable-name noise (`text`, `text_1`,
 * `node`, `var fragment`, …) in the JS CARRIER are intentionally NOT pinned —
 * cosmetic carrier formatting the conformance bar waives. Helper FAMILIES and
 * their argument rows, the import set, the export shape, the static-HTML
 * template skeleton strings, the scope-hash topology, the per-backend
 * decisions, and the CSS payload bytes (external `css.code` and the injected
 * `$$css` literal) ARE pinned.
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
 *     node scripts/gen-svelte-goldens.mjs --conformance
 *                                                    # rewrite the CONFORMANCE-corpus
 *                                                    # goldens (crates/
 *                                                    # verter_svelte_conformance/
 *                                                    # corpus/goldens/) from the
 *                                                    # canonical Rust emit-plan.
 *     node scripts/gen-svelte-goldens.mjs --conformance --check
 *                                                    # assert the committed
 *                                                    # conformance goldens equal
 *                                                    # the fresh emit-plan +
 *                                                    # pinned-compiler output
 *                                                    # (non-zero exit on drift).
 *
 * ## Conformance-corpus mode (`--conformance`)
 *
 * The `verter_svelte_conformance` crate owns the canonical CSS-scoping
 * coverage plan (a typed Rust manifest). This script NEVER re-derives that
 * matrix: `--conformance` spawns the Rust CLI
 * (`cargo run --quiet -p verter_svelte_conformance -- emit-plan`), parses the
 * plan JSON from stdout, compiles every case's `source` with the SAME pinned
 * compiler on both backends, and writes DISPOSITION-AWARE goldens to
 * `crates/verter_svelte_conformance/corpus/goldens/<slug>.<backend>.json`:
 *
 *   - `supported` / `refused:*` — the official compiler COMPILES the case;
 *     the golden is the same normalized topology schema the oracle corpus
 *     uses (a `refused` case's Verter-side refusal is asserted by the Rust
 *     differential suite, not here — the golden pins the OFFICIAL output).
 *   - `oracle-rejected:*` — the official compiler REJECTS the case; the
 *     golden captures the official diagnostic
 *     (`{ rejected: true, diagnostic: { code, message } }`). A case declared
 *     oracle-rejected that COMPILES is a hard error, and vice versa.
 *
 * The two corpora stay separate: the oracle-corpus modes above never touch
 * the conformance corpus, and `--conformance` never touches the oracle
 * corpus (nor its `generated/` subtree).
 *
 * ## Hermeticity
 *
 * `svelte` is a pinned devDependency, so this script (and the `--check` guard)
 * MAY read it from `node_modules`. The DEFAULT canonical Rust run checks the
 * COMMITTED goldens only — the live-compiler oracle harness is feature-gated
 * (`svelte-oracle`) and excluded from the default workspace test set.
 *
 * The pinned version's single source of truth is `SVELTE_ORACLE_VERSION` in
 * `scripts/svelte-golden-lib.mjs` (imported here); the Rust guard
 * `svelte_lockfile_matches_oracle_pin` asserts the resolved `pnpm-lock.yaml`
 * version equals it, and the hermetic guard
 * `committed_svelte_goldens_match_oracle_pin` asserts every committed golden's
 * `oracleVersion` equals it (so a `svelte` bump that leaves STALE goldens
 * fails). A version bump is a reviewed delta: re-pin in the lib, bump the
 * lockfile, run this script (which restamps every golden), review the diff.
 */

import { spawnSync } from "node:child_process";
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
  extractTemplates,
  helperCountsOf,
  helperSequenceOf,
  loadPinnedCompiler as loadPinnedCompilerFrom,
  normalizeCss,
  normalizeModuleForComparison,
  // The oracle pin — `svelte-golden-lib.mjs` is the SOLE JS authority for
  // `SVELTE_ORACLE_VERSION`. The Rust guard `svelte_lockfile_matches_oracle_pin`
  // parses the pin from the lib and asserts the resolved `pnpm-lock.yaml`
  // `svelte@<version>` equals it; the hermetic guard
  // `committed_svelte_goldens_match_oracle_pin` asserts every committed
  // golden's `oracleVersion` equals it. A `svelte` bump is therefore a
  // reviewed delta: re-pin in the lib, bump the lockfile, regenerate.
  SVELTE_ORACLE_VERSION,
} from "./svelte-golden-lib.mjs";

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
// Conformance corpus (`--conformance`) — the CSS-manifest goldens owned by the
// `verter_svelte_conformance` crate. The crate's Rust CLI owns the fixture
// corpus (`corpus/fixtures/` + the review artifacts); THIS script owns exactly
// the `corpus/goldens/` subtree, generated from the emit-plan wire.
// ---------------------------------------------------------------------------
const CONFORMANCE_CORPUS_ROOT = resolve(REPO_ROOT, "crates/verter_svelte_conformance/corpus");
const CONFORMANCE_GOLDENS_DIR = join(CONFORMANCE_CORPUS_ROOT, "goldens");

// The canonical-plan command: the typed Rust manifest is the sole matrix
// authority; this script only consumes its wire (it never re-derives cases).
const EMIT_PLAN_COMMAND = [
  "cargo",
  "run",
  "--quiet",
  "-p",
  "verter_svelte_conformance",
  "--",
  "emit-plan",
];

// The emit-plan wire schema this consumer understands. A bump on the Rust side
// is a reviewed delta of this façade, never a silent reinterpretation.
const CONFORMANCE_PLAN_SCHEMA_VERSION = 1;

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
 *   - in CONFORMANCE-WRITE mode, any target other than EXACTLY
 *     `CONFORMANCE_GOLDENS_DIR` — the conformance mode owns that one subtree
 *     of the conformance crate's corpus and writes nowhere else;
 *   - an existing path that is not a directory, or — for an EMIT target — an
 *     existing non-empty directory that does NOT carry the generator-owned
 *     `EMIT_SENTINEL` marker at its root. An existing emit target is therefore
 *     accepted only when it is empty or a tree the generator itself produced;
 *     a directory whose leaves merely happen to all be `.json` is NOT accepted.
 *
 * @param {string} dir          resolved absolute path
 * @param {{ role: "write" | "emit" | "conformance-write" }} opts
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

  if (role === "conformance-write") {
    // The conformance mode owns EXACTLY the conformance crate's committed
    // goldens subtree — never the crate's fixtures (Rust-CLI-owned), never the
    // oracle corpus, never anything else.
    if (target !== CONFORMANCE_GOLDENS_DIR) {
      throw new Error(
        `refusing destructive rmSync on ${target} in conformance mode: it owns ` +
          `exactly the conformance goldens directory ${CONFORMANCE_GOLDENS_DIR} ` +
          `and writes nowhere else.`,
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

/**
 * Per-fixture COMPILE OPTIONS — the few fixtures whose surface is a compile
 * option rather than in-source syntax. Keyed by fixture slug; spread into the
 * `compile()` options for BOTH backends. The Rust emitted-JS gate mirrors this
 * map (`compile_options_for` in `svelte_client_emit_topology.rs`) so Verter
 * compiles the fixture under the same options the golden was generated with.
 */
const FIXTURE_COMPILE_OPTIONS = {
  // The `customElement: true` compile option (no `<svelte:options>` value) —
  // the create-without-define custom-element form.
  "options/custom_element_option_true.svelte": { customElement: true },
  // NO filename: the default css-hash input falls back to the css TEXT
  // (`filename === '(unknown)' ? css : filename ?? css`) — the golden's
  // css.hash pins the fallback, discriminating it from the filename input.
  "css/scope_hash_fallback_no_filename.svelte": { filename: undefined },
};

// ---------------------------------------------------------------------------
// Normalization (carrier formatting waived; contractual bytes pinned)
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

  // The shared css normalization (svelte-golden-lib `normalizeCss`):
  // presence is `compiled.css !== null` — an existing-but-empty `<style>`
  // body is a REAL artifact (`{present: true, hash: null, code: ""}`), only
  // an absent style block / injected mode normalizes absent.
  const css = normalizeCss(compiled);

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
          ...(FIXTURE_COMPILE_OPTIONS[slug] ?? {}),
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

// ---------------------------------------------------------------------------
// Conformance-corpus mode (`--conformance` / `--conformance --check`)
// ---------------------------------------------------------------------------

/**
 * Obtain the canonical conformance plan by spawning the Rust CLI
 * (`EMIT_PLAN_COMMAND`, cwd = repo root) and parsing its stdout JSON. The
 * typed Rust manifest is the sole matrix authority — this consumer validates
 * the wire (schema version, case shape) and never re-derives cases.
 */
function loadConformancePlan() {
  const [command, ...args] = EMIT_PLAN_COMMAND;
  const spawned = spawnSync(command, args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    // The plan carries every rendered fixture source; keep ample headroom.
    maxBuffer: 256 * 1024 * 1024,
  });
  if (spawned.error) {
    throw new Error(`failed to spawn \`${EMIT_PLAN_COMMAND.join(" ")}\`: ${spawned.error.message}`);
  }
  if (spawned.status !== 0) {
    throw new Error(
      `\`${EMIT_PLAN_COMMAND.join(" ")}\` exited with status ${spawned.status}:\n` +
        `${spawned.stderr ?? ""}`,
    );
  }
  let plan;
  try {
    plan = JSON.parse(spawned.stdout);
  } catch (err) {
    throw new Error(`emit-plan stdout is not valid JSON: ${err.message}`);
  }
  if (plan.schemaVersion !== CONFORMANCE_PLAN_SCHEMA_VERSION) {
    throw new Error(
      `emit-plan schemaVersion ${plan.schemaVersion} is not the supported ` +
        `${CONFORMANCE_PLAN_SCHEMA_VERSION} — the wire changed; review this ` +
        `consumer against the new plan schema before regenerating goldens.`,
    );
  }
  if (typeof plan.manifestHash !== "string" || plan.manifestHash === "") {
    throw new Error("emit-plan is missing its manifestHash");
  }
  if (!Array.isArray(plan.cases) || plan.cases.length === 0) {
    throw new Error("emit-plan carries no cases");
  }
  return plan;
}

/**
 * Defense in depth (mirrors the Rust CLI's slug validation): a conformance
 * slug must be a single portable path component, so a hostile/corrupt plan
 * can never traverse outside the goldens directory.
 */
function validateConformanceSlug(slug) {
  if (typeof slug !== "string" || !/^[a-z0-9-]+$/.test(slug)) {
    throw new Error(`conformance slug ${JSON.stringify(slug)} is not a portable path component`);
  }
}

/**
 * Normalize the official compiler's rejection into a STABLE diagnostic:
 * the machine-readable `code` plus the first line of the message. The
 * official message appends a `https://svelte.dev/e/<code>` docs-link line
 * that restates the code — the captured `code` field already pins that
 * identity, so only the human-readable first line is kept. Position/frame
 * data is deliberately dropped (filename- and layout-dependent).
 */
function normalizeOracleRejection(err) {
  const code = typeof err?.code === "string" && err.code !== "" ? err.code : null;
  if (code === null) {
    throw new Error(
      `the official compiler rejected the case WITHOUT a structured CompileError ` +
        `code — refusing to golden an unclassified failure: ${err?.message ?? err}`,
    );
  }
  const message = String(err.message).split("\n", 1)[0].trim();
  return { code, message };
}

/**
 * Compile every plan case on both of its backends and build the
 * DISPOSITION-AWARE golden map `{ absolute path -> serialized content }`:
 *
 *   - `supported` / `refused:*` — the official compiler MUST compile the
 *     case; the golden is the same normalized topology schema the oracle
 *     corpus uses (`normalize`). A `refused` case is refused by VERTER, not
 *     by the official compiler — its Verter-side refusal is asserted by the
 *     Rust differential suite; the golden pins the official output.
 *   - `oracle-rejected:*` — the official compiler MUST reject the case; the
 *     golden captures the normalized official diagnostic.
 *
 * A disposition/oracle disagreement in either direction is a hard error —
 * never a silently skipped or empty golden.
 */
function buildConformanceGoldens(compiler, plan) {
  const result = new Map();
  for (const planCase of plan.cases) {
    validateConformanceSlug(planCase.slug);
    if (typeof planCase.source !== "string" || typeof planCase.disposition !== "string") {
      throw new Error(`malformed emit-plan case ${planCase.slug}`);
    }
    if (
      !Array.isArray(planCase.backends) ||
      planCase.backends.length === 0 ||
      !planCase.backends.every((b) => BACKENDS.includes(b))
    ) {
      throw new Error(
        `emit-plan case ${planCase.slug} carries an unknown backend set ` +
          `${JSON.stringify(planCase.backends)} (known: ${BACKENDS.join(", ")})`,
      );
    }
    const oracleRejected = planCase.disposition.startsWith("oracle-rejected:");
    const name = componentNameFor(planCase.slug);
    for (const backend of planCase.backends) {
      let compiled = null;
      let rejection = null;
      try {
        compiled = compiler.compile(planCase.source, {
          generate: backend,
          // The fixture-relative filename (`corpus/fixtures/<slug>.svelte`
          // basename) — the css scope-hash input the Rust differential must
          // reproduce when it compiles the committed fixture.
          filename: `${planCase.slug}.svelte`,
          name,
          ...(planCase.compileOptions ?? {}),
        });
      } catch (err) {
        rejection = err;
      }
      let golden;
      if (oracleRejected) {
        if (rejection === null) {
          throw new Error(
            `conformance case ${planCase.slug} (${backend}) is declared ` +
              `${planCase.disposition} but the official compiler COMPILED it — ` +
              `the manifest disposition and the pinned oracle disagree.`,
          );
        }
        golden = {
          slug: planCase.slug,
          backend,
          oracleVersion: SVELTE_ORACLE_VERSION,
          rejected: true,
          diagnostic: normalizeOracleRejection(rejection),
        };
      } else {
        if (rejection !== null) {
          throw new Error(
            `svelte compile failed for conformance case ${planCase.slug} ` +
              `(${backend}, disposition ${planCase.disposition}): ${rejection.message}`,
          );
        }
        golden = normalize(planCase.slug, backend, compiled);
      }
      result.set(
        join(CONFORMANCE_GOLDENS_DIR, `${planCase.slug}.${backend}.json`),
        serializeGolden(golden),
      );
    }
  }
  return result;
}

/** Clean-rewrite the conformance goldens subtree (and ONLY that subtree). */
function conformanceWriteMode(compiler) {
  const plan = loadConformancePlan();
  const goldens = buildConformanceGoldens(compiler, plan);
  assertSafeDestructiveDir(CONFORMANCE_GOLDENS_DIR, { role: "conformance-write" });
  rmSync(CONFORMANCE_GOLDENS_DIR, { recursive: true, force: true });
  mkdirSync(CONFORMANCE_GOLDENS_DIR, { recursive: true });
  for (const [path, content] of [...goldens].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    writeFileSync(path, content);
  }
  console.log(
    `gen-svelte-goldens --conformance: wrote ${goldens.size} golden(s) for ` +
      `${plan.cases.length} case(s) (manifest ${plan.manifestHash}, ` +
      `svelte@${SVELTE_ORACLE_VERSION}) into ` +
      `${relative(REPO_ROOT, CONFORMANCE_GOLDENS_DIR)}`,
  );
}

/**
 * Re-run the plan + pinned compiler and assert every committed conformance
 * golden equals the fresh output (line-ending-normalized, so a CRLF checkout
 * does not false-drift), with no missing and no orphan goldens vs the plan.
 * Non-zero exit + a drift report on any mismatch.
 */
function conformanceCheckMode(compiler) {
  const plan = loadConformancePlan();
  const fresh = buildConformanceGoldens(compiler, plan);
  const normalizeEol = (text) => text.replace(/\r\n/g, "\n");
  const drift = [];

  for (const [path, content] of fresh) {
    const rel = relative(REPO_ROOT, path);
    if (!existsSync(path)) {
      drift.push(`MISSING golden: ${rel}`);
      continue;
    }
    const committed = readFileSync(path, "utf8");
    if (normalizeEol(committed) !== normalizeEol(content)) {
      drift.push(`DRIFTED golden (on-disk != regenerated): ${rel}`);
    }
  }

  // Orphan scan: the conformance goldens subtree is FULLY owned by this mode,
  // so every on-disk entry must be an expected `<slug>.<backend>.json`.
  const expected = new Set([...fresh.keys()]);
  const collectCommitted = (dir) => {
    if (!existsSync(dir)) return;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        collectCommitted(p);
      } else if (!expected.has(p)) {
        drift.push(`STALE golden (no plan case/backend): ${relative(REPO_ROOT, p)}`);
      }
    }
  };
  collectCommitted(CONFORMANCE_GOLDENS_DIR);

  if (drift.length > 0) {
    console.error(
      `gen-svelte-goldens --conformance --check: the conformance goldens are out ` +
        `of sync with the emit-plan (manifest ${plan.manifestHash}) + the pinned ` +
        `svelte@${SVELTE_ORACLE_VERSION} compiler.\n` +
        drift.map((d) => `  - ${d}`).join("\n") +
        `\n\nRegenerate with \`node scripts/gen-svelte-goldens.mjs --conformance\` and ` +
        `review the diff as the oracle delta. Do NOT hand-edit the goldens.`,
    );
    process.exit(1);
  }
  console.log(
    `gen-svelte-goldens --conformance --check: ${fresh.size} golden(s) in sync ` +
      `with the emit-plan (manifest ${plan.manifestHash}) and svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const conformance = process.argv.includes("--conformance");
  const emitArg = process.argv.find((a) => a.startsWith("--emit-dir="));
  const goldensDirArg = process.argv.find((a) => a.startsWith("--goldens-dir="));
  const compiler = loadPinnedCompiler();
  if (conformance) {
    if (emitArg || goldensDirArg) {
      throw new Error(
        "--conformance supports only --check; --emit-dir/--goldens-dir belong " +
          "to the oracle-corpus modes.",
      );
    }
    if (check) {
      conformanceCheckMode(compiler);
    } else {
      conformanceWriteMode(compiler);
    }
    return;
  }
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
