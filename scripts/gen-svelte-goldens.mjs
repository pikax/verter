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

import { createRequire } from "node:module";
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
 * Resolve the pinned svelte compiler. Pins the EXACT version directory under
 * pnpm so a different installed `svelte` cannot silently satisfy the oracle.
 * Throws a clear error (rather than resolving a floating `svelte`) when the
 * pinned version is not installed.
 */
function loadPinnedCompiler() {
  const require = createRequire(join(REPO_ROOT, "noop.js"));
  // The pnpm content-addressed path for the pinned version. Pinning the exact
  // version directory forbids a floating-version fallback.
  const pinnedDir = join(
    REPO_ROOT,
    "node_modules/.pnpm",
    `svelte@${SVELTE_ORACLE_VERSION}`,
    "node_modules/svelte",
  );
  const pkgPath = join(pinnedDir, "package.json");
  if (!existsSync(pkgPath)) {
    throw new Error(
      `pinned svelte@${SVELTE_ORACLE_VERSION} not installed at ${pinnedDir}. ` +
        `Run \`pnpm install\` (svelte is a pinned devDependency).`,
    );
  }
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  if (pkg.version !== SVELTE_ORACLE_VERSION) {
    throw new Error(
      `installed svelte version ${pkg.version} != pinned SVELTE_ORACLE_VERSION ` +
        `${SVELTE_ORACLE_VERSION}. Re-pin the oracle and regenerate the goldens.`,
    );
  }
  const compilerPath = join(pinnedDir, "compiler/index.js");
  return require(compilerPath);
}

// ---------------------------------------------------------------------------
// Corpus discovery
// ---------------------------------------------------------------------------

/** Recursively collect `.svelte` fixtures, sorted lexicographically. */
function discoverFixtures(dir) {
  const out = [];
  const walk = (d) => {
    const entries = readdirSync(d, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    );
    for (const e of entries) {
      const p = join(d, e.name);
      if (e.isDirectory()) {
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

const SCOPE_HASH_RE = /svelte-[0-9a-z]+/g;
const SCOPE_HASH_PLACEHOLDER = "svelte-<scoped>";

/** First `svelte-<hash>` token in the source, or `null`. Topology, not bytes. */
function extractScopeHash(text) {
  const m = text.match(/svelte-[0-9a-z]+/);
  return m ? m[0] : null;
}

function maskScopeHash(text) {
  return text.replace(SCOPE_HASH_RE, SCOPE_HASH_PLACEHOLDER);
}

/**
 * Mask the NON-CODE regions of a JS module — string literals, the TEXT spans of
 * template literals, line comments, block comments, and regex literals — by
 * overwriting their contents with spaces (newlines preserved so line structure
 * is unchanged). The masked-out characters can no longer match the `$.<helper>`
 * member-access scan below, so a literal `$.<ident>` authored in MARKUP (which
 * the compiler emits into a template-literal TEXT span — `$.from_html(`…`)` on
 * the client, `$$payload.out += `…`` on the server) cannot pollute the helper
 * topology with a phantom call.
 *
 * Template-literal `${…}` INTERPOLATIONS are deliberately NOT masked: they are
 * real code, and the SSR backend renders genuine helper calls (`$.escape`,
 * `$.attr`, `$.get`, …) inside them. Masking the interpolations would DROP real
 * topology. The scanner tracks template-literal nesting (a template inside an
 * interpolation inside a template) via a brace-depth stack so it resumes
 * template TEXT masking exactly when an interpolation closes.
 *
 * This is a single-pass character scanner, not a full JS parse — it preserves
 * the helper FAMILY/sequence topology (the oracle bar) while excluding non-code
 * bytes, and stays var-rename / whitespace stable.
 */
function maskNonCodeRegions(code) {
  const out = Array.from(code);
  const n = code.length;
  // Stack of template-literal frames currently open. Each frame records the
  // interpolation `{`-nesting depth at which the template TEXT resumes; while
  // `interpDepth > 0` we are in CODE inside a `${…}` and must scan normally.
  const tmplStack = [];
  // Previous significant (non-whitespace, non-comment) character — used to
  // decide whether a `/` begins a regex literal or is a division operator.
  let prevSignificant = "";
  let i = 0;

  const inTemplateText = () =>
    tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth === 0;

  const maskChar = (idx) => {
    if (code[idx] !== "\n" && code[idx] !== "\r") out[idx] = " ";
  };

  while (i < n) {
    // ----- inside template-literal TEXT (not an interpolation) -----
    if (inTemplateText()) {
      const ch = code[i];
      if (ch === "\\") {
        // Escaped char inside template text — mask both bytes.
        maskChar(i);
        if (i + 1 < n) maskChar(i + 1);
        i += 2;
        continue;
      }
      if (ch === "`") {
        // Close this template literal — the backtick itself is code.
        tmplStack.pop();
        prevSignificant = "`";
        i += 1;
        continue;
      }
      if (ch === "$" && i + 1 < n && code[i + 1] === "{") {
        // Open an interpolation — `${` is code; contents scanned as code.
        tmplStack[tmplStack.length - 1].interpDepth = 1;
        prevSignificant = "{";
        i += 2;
        continue;
      }
      // Plain template TEXT byte — mask it.
      maskChar(i);
      i += 1;
      continue;
    }

    // ----- CODE (top level, or inside a `${…}` interpolation) -----
    const ch = code[i];
    const next = i + 1 < n ? code[i + 1] : "";

    // Line comment.
    if (ch === "/" && next === "/") {
      maskChar(i);
      maskChar(i + 1);
      i += 2;
      while (i < n && code[i] !== "\n") {
        maskChar(i);
        i += 1;
      }
      continue;
    }
    // Block comment.
    if (ch === "/" && next === "*") {
      maskChar(i);
      maskChar(i + 1);
      i += 2;
      while (i < n && !(code[i] === "*" && i + 1 < n && code[i + 1] === "/")) {
        maskChar(i);
        i += 1;
      }
      if (i < n) {
        maskChar(i); // '*'
        maskChar(i + 1); // '/'
        i += 2;
      }
      continue;
    }
    // Single- / double-quoted string literal.
    if (ch === "'" || ch === '"') {
      const quote = ch;
      prevSignificant = quote;
      i += 1; // opening quote is code
      while (i < n && code[i] !== quote) {
        if (code[i] === "\\") {
          maskChar(i);
          if (i + 1 < n) maskChar(i + 1);
          i += 2;
          continue;
        }
        maskChar(i);
        i += 1;
      }
      if (i < n) i += 1; // closing quote is code
      continue;
    }
    // Template-literal open.
    if (ch === "`") {
      tmplStack.push({ interpDepth: 0 });
      prevSignificant = "`";
      i += 1;
      continue;
    }
    // Brace tracking for the innermost open interpolation, so a nested `{…}`
    // inside `${…}` does not prematurely close the interpolation.
    if (tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth > 0) {
      const frame = tmplStack[tmplStack.length - 1];
      if (ch === "{") {
        frame.interpDepth += 1;
        prevSignificant = "{";
        i += 1;
        continue;
      }
      if (ch === "}") {
        frame.interpDepth -= 1; // closing the `${…}` returns to template TEXT
        prevSignificant = "}";
        i += 1;
        continue;
      }
    }
    // Regex literal: only when a `/` in expression position (the previous
    // significant token cannot end an expression). Generated svelte output is
    // unlikely to embed `$.` in a regex, but masking it keeps the scan honest.
    if (ch === "/" && regexAllowedAfter(prevSignificant)) {
      i += 1; // opening slash is code
      let inClass = false;
      while (i < n) {
        const rc = code[i];
        if (rc === "\\") {
          maskChar(i);
          if (i + 1 < n) maskChar(i + 1);
          i += 2;
          continue;
        }
        if (rc === "[") inClass = true;
        else if (rc === "]") inClass = false;
        else if (rc === "/" && !inClass) {
          i += 1; // closing slash is code
          break;
        }
        if (rc === "\n") break; // unterminated — bail (treat as not-a-regex)
        maskChar(i);
        i += 1;
      }
      // Skip regex flags (code, harmless to the `$.` scan).
      while (i < n && /[a-z]/i.test(code[i])) i += 1;
      prevSignificant = "/";
      continue;
    }

    if (!/\s/.test(ch)) prevSignificant = ch;
    i += 1;
  }

  return out.join("");
}

/**
 * True when a `/` appearing after `prev` (the previous significant character)
 * begins a regex literal rather than a division operator. A regex may start
 * when the prior token cannot terminate an expression — i.e. `prev` is empty
 * (start of input) or one of the expression-position delimiters/operators.
 */
function regexAllowedAfter(prev) {
  if (prev === "") return true;
  return "([{,;:=&|!?+-*%^~<>".includes(prev);
}

/**
 * Extract the ORDERED `$.<helper>` reference sequence from the module body.
 * The runtime namespace is imported as `$` (`import * as $ from …`), so every
 * helper call/reference is `$.<ident>`. The scan runs over the CODE-only view
 * of the module (`maskNonCodeRegions`): string literals, comments, and the TEXT
 * spans of template literals are masked, so a literal `$.<ident>` authored in
 * markup cannot pollute the topology, while real helper calls inside template
 * `${…}` interpolations (the SSR `$.escape`/`$.attr`/… calls) are preserved.
 * This is deliberately NOT a full JS parse — it captures the helper FAMILY
 * topology, which is the oracle bar, and is var-rename / whitespace stable.
 */
function extractHelperSequence(code) {
  const masked = maskNonCodeRegions(code);
  const seq = [];
  const re = /\$\.([A-Za-z_][A-Za-z0-9_]*)/g;
  let m;
  while ((m = re.exec(masked)) !== null) {
    seq.push(m[1]);
  }
  return seq;
}

/**
 * Extract the import topology: bare side-effect imports, the runtime namespace
 * import, and named/default imports — as `{ source, kind, names }` rows, sorted
 * deterministically. Var-rename stable: a renamed local default import is
 * captured by its imported binding shape, not byte position.
 */
function extractImports(code) {
  const rows = [];
  const lines = code.split("\n");
  for (const raw of lines) {
    const line = raw.trim();
    if (!line.startsWith("import ")) continue;
    // Bare side-effect import: `import 'x';`
    let m = line.match(/^import\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      rows.push({ source: m[1], kind: "sideEffect", names: [] });
      continue;
    }
    // Namespace import: `import * as $ from 'x';`
    m = line.match(/^import\s+\*\s+as\s+([A-Za-z_$][\w$]*)\s+from\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      rows.push({ source: m[2], kind: "namespace", names: [m[1]] });
      continue;
    }
    // Default + (optional) named: `import D from 'x';` / `import { a, b } from 'x';`
    m = line.match(/^import\s+(.+?)\s+from\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      const clause = m[1].trim();
      const source = m[2];
      const names = [];
      let kind = "named";
      const braceIdx = clause.indexOf("{");
      if (braceIdx === 0) {
        kind = "named";
      } else if (braceIdx > 0) {
        kind = "defaultAndNamed";
        names.push(`default:${clause.slice(0, braceIdx).replace(/,$/, "").trim()}`);
      } else {
        kind = "default";
        names.push(`default:${clause}`);
      }
      const braceMatch = clause.match(/\{([^}]*)\}/);
      if (braceMatch) {
        for (const part of braceMatch[1].split(",")) {
          const t = part.trim();
          if (t) names.push(t);
        }
      }
      rows.push({ source, kind, names });
      continue;
    }
  }
  // Deterministic multi-field tuple sort over (source, kind, names): a stable
  // total order with no synthetic delimiter that could collide with field
  // bytes (and no non-printable separator).
  const cmp = (x, y) => (x < y ? -1 : x > y ? 1 : 0);
  rows.sort(
    (a, b) =>
      cmp(a.source, b.source) || cmp(a.kind, b.kind) || cmp(a.names.join(","), b.names.join(",")),
  );
  return rows;
}

/**
 * Extract the default-exported component function shape: name + ordered param
 * identifier list. The body is intentionally not captured (the helper sequence
 * already pins the body topology).
 */
function extractExportDefault(code) {
  const m = code.match(/export\s+default\s+function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)/);
  if (!m) return null;
  const name = m[1];
  const params = m[2]
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    // strip default-value noise — keep the param identifier only.
    .map((p) => p.split("=")[0].trim());
  return { name, params };
}

/**
 * Extract the template skeletons: every `$.from_html` / `$.from_svg` /
 * `$.from_mathml` first-argument template literal + the optional trailing
 * fragment flag. Scope hashes masked. Captures the DOM skeleton topology.
 */
function extractTemplates(code) {
  const out = [];
  const re =
    /\$\.(from_html|from_svg|from_mathml|from_tree)\(`((?:\\.|[^`\\])*)`(?:\s*,\s*([^)]+))?\)/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    out.push({
      factory: m[1],
      html: maskScopeHash(m[2]),
      flag: m[3] !== undefined ? m[3].trim() : null,
    });
  }
  return out;
}

/** Normalize one compiled fixture to its topology golden object. */
function normalize(slug, backend, compiled) {
  const code = compiled.js.code;
  const helperSequence = extractHelperSequence(code);
  const helperSet = [...new Set(helperSequence)].sort();
  const helperCounts = {};
  for (const h of helperSequence) helperCounts[h] = (helperCounts[h] || 0) + 1;

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
    templates: backend === "client" ? extractTemplates(code) : [],
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
  // Clean rewrite: drop the goldens tree, then write fresh. Idempotent.
  assertSafeDestructiveDir(GOLDENS_DIR, { role: "write" });
  rmSync(GOLDENS_DIR, { recursive: true, force: true });
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

  // Detect stale goldens with no corresponding fixture/backend.
  const expected = new Set([...fresh.keys()]);
  const collectCommitted = (dir) => {
    if (!existsSync(dir)) return;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) collectCommitted(p);
      else if (e.isFile() && e.name.endsWith(".json") && !expected.has(p)) {
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
