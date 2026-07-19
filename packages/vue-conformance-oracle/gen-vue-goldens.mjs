#!/usr/bin/env node
/**
 * Generator — official Vue 3.6 RC conformance goldens (oracle).
 *
 * Runs the PINNED official Vue RC compiler (`vue` / `@vue/compiler-dom` /
 * `@vue/compiler-sfc` / `@vue/compiler-vapor`, all exactly
 * `VUE_ORACLE_VERSION` from `./vue-golden-lib.mjs`) over every committed
 * corpus SFC under `crates/verter_vue_conformance/corpus/cases/` and writes
 * VENDORED, COMMITTED goldens: the official emitted render/component module
 * per backend (`vdom` + `vapor`), its source map, and a per-cell metadata
 * JSON (schema, package versions, source/options/code/map SHA-256, backend,
 * disposition, interleaved diagnostics, helper inventory, generator
 * version). It then rewrites `corpus/manifest.json` (case-id → SFC →
 * per-backend golden/map/meta paths + disposition).
 *
 * The goldens are the release ORACLE the `verter_vue_conformance` Rust suite
 * compares Verter output against (structurally, not byte-wise — cosmetic JS
 * formatting and private local names are waived). They are NEVER hand-edited:
 * this script is the only writer, and `node gen-vue-goldens.mjs --check`
 * regenerates everything in-memory and fails on any missing / drifted /
 * stale committed artifact.
 *
 * Guarantees:
 *   - PINNED: all four Vue packages AND esbuild must resolve to the exact
 *     versions declared in `./vue-golden-lib.mjs`; the generator refuses to
 *     run on any drift (no ranges, no dist-tags).
 *   - HERMETIC: compilation may only read inside `corpus/`. The `compileScript`
 *     type-resolution `fs` is replaced with a guard that denies (throws on)
 *     any read outside the corpus root (symlinks resolved); shared support
 *     files are vendored under `corpus/support/`.
 *   - DETERMINISTIC: case order is sorted, options are fixed and hashed, no
 *     timestamps, no absolute paths (all recorded paths are corpus-relative
 *     POSIX). Re-running with the same pins reproduces identical bytes.
 *
 * TypeScript cells: `compileScript` keeps TS syntax in its emitted module
 * (the official SFC loaders strip types downstream). For `lang="ts"` cells
 * the emitted module is type-stripped with the pinned esbuild
 * (`{ loader: "ts" }` only — no format conversion, so PURE annotations and
 * the official export shape survive) and the compiler's source map is
 * chained through the strip. Every stripped cell records the post-process in
 * its metadata. All other cells vendor the raw compiler bytes untouched.
 *
 * Usage:
 *   node gen-vue-goldens.mjs           # clean regenerate (rewrites goldens + manifest)
 *   node gen-vue-goldens.mjs --check   # verify committed artifacts match a fresh run
 */

import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { promises as fs, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as compilerDom from "@vue/compiler-dom";
import * as compilerVapor from "@vue/compiler-vapor";
import { compileScript, compileTemplate, parse } from "@vue/compiler-sfc";
import { transform } from "esbuild";

import {
  ESBUILD_VERSION,
  GENERATOR_VERSION,
  MANIFEST_SCHEMA_VERSION,
  META_SCHEMA_VERSION,
  ORACLE_PACKAGES,
  VUE_ORACLE_VERSION,
} from "./vue-golden-lib.mjs";

const PACKAGE_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(PACKAGE_DIR, "..", "..");
const CORPUS_ROOT = path.join(REPO_ROOT, "crates", "verter_vue_conformance", "corpus");
const CASES_ROOT = path.join(CORPUS_ROOT, "cases");
const GOLDENS_ROOT = path.join(CORPUS_ROOT, "goldens", VUE_ORACLE_VERSION);
const MANIFEST_PATH = path.join(CORPUS_ROOT, "manifest.json");
const BACKENDS = ["vdom", "vapor"];
const CHECK_MODE = process.argv.includes("--check");

// ---------------------------------------------------------------------------
// Pin assertion — refuse to generate with anything but the exact RC toolchain.
// ---------------------------------------------------------------------------

const require = createRequire(import.meta.url);

function resolvedVersion(pkg) {
  const manifestPath = require.resolve(`${pkg}/package.json`, {
    paths: [PACKAGE_DIR],
  });
  return JSON.parse(readFileSync(manifestPath, "utf8")).version;
}

function assertPinnedToolchain() {
  const versions = {};
  for (const pkg of ORACLE_PACKAGES) {
    const version = resolvedVersion(pkg);
    if (version !== VUE_ORACLE_VERSION) {
      throw new Error(
        `oracle pin violation: ${pkg} resolved to ${version}, expected exactly ${VUE_ORACLE_VERSION} ` +
          `(pin authority: packages/vue-conformance-oracle/vue-golden-lib.mjs)`,
      );
    }
    versions[pkg] = version;
  }
  const esbuild = resolvedVersion("esbuild");
  if (esbuild !== ESBUILD_VERSION) {
    throw new Error(
      `oracle pin violation: esbuild resolved to ${esbuild}, expected exactly ${ESBUILD_VERSION}`,
    );
  }
  versions.esbuild = esbuild;
  return versions;
}

// ---------------------------------------------------------------------------
// Hermetic filesystem — compilation reads stay inside corpus/ (symlinks
// resolved). Any outside read throws; the compile fails loudly.
// ---------------------------------------------------------------------------

const CORPUS_REAL = realpathSync(CORPUS_ROOT);

function insideCorpus(realPath) {
  return realPath === CORPUS_REAL || realPath.startsWith(CORPUS_REAL + path.sep);
}

function assertCorpusPath(candidate, what) {
  const real = realpathSync(candidate);
  if (!insideCorpus(real)) {
    throw new Error(`hermetic boundary: denied ${what} outside corpus/: ${candidate}`);
  }
  return real;
}

/**
 * Containment check for paths that may not exist yet (write targets). The
 * corpus tree is generator-owned (only dirs/files this script creates, no
 * symlinks), so a lexical resolve against the real corpus root is sound.
 */
function assertCorpusWritePath(candidate, what) {
  const abs = path.resolve(candidate);
  if (abs !== CORPUS_REAL && !abs.startsWith(CORPUS_REAL + path.sep)) {
    throw new Error(`hermetic boundary: denied ${what} outside corpus/: ${candidate}`);
  }
  return abs;
}

/** `fs` adapter handed to `compileScript` for imported-type resolution. */
const hermeticCompileFs = {
  fileExists(candidate) {
    try {
      return insideCorpus(realpathSync(resolveCompileCandidate(candidate)));
    } catch {
      return false;
    }
  },
  readFile(candidate) {
    const real = realpathSync(resolveCompileCandidate(candidate));
    if (!insideCorpus(real)) {
      throw new Error(`hermetic boundary: denied read outside corpus/: ${candidate}`);
    }
    return readFileSync(real, "utf8");
  },
};

/**
 * `compileScript` is invoked with the corpus-relative POSIX filename
 * `cases/<id>.vue`; its imported-type resolver joins import specifiers onto
 * that filename lexically, so relative candidates arrive corpus-root-relative
 * (e.g. `support/types.ts`) — never cwd-relative. Resolve them against the
 * corpus root; absolute candidates are taken as-is. `realpathSync` then
 * settles symlink escapes before the containment check.
 */
function resolveCompileCandidate(candidate) {
  return path.isAbsolute(candidate) ? candidate : path.join(CORPUS_ROOT, candidate);
}

// ---------------------------------------------------------------------------
// Small deterministic utilities.
// ---------------------------------------------------------------------------

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`).join(",")}}`;
}

/** Repo-relative POSIX path (the only path shape recorded in artifacts). */
function toPosixRelative(absolute) {
  const rel = path.relative(REPO_ROOT, absolute);
  return rel.split(path.sep).join("/");
}

function withTrailingNewline(text) {
  return text.endsWith("\n") ? text : text + "\n";
}

function scrubPaths(message) {
  return String(message).split(REPO_ROOT).join("<repo>");
}

// ---------------------------------------------------------------------------
// Compilation.
// ---------------------------------------------------------------------------

function pushDiagnostic(diagnostics, kind, error) {
  const entry = { kind, message: scrubPaths(error?.message ?? String(error)) };
  const code = error?.code;
  if (typeof code === "number" || typeof code === "string") entry.code = code;
  const start = error?.loc?.start;
  if (start && Number.isInteger(start.line) && Number.isInteger(start.column)) {
    entry.loc = { line: start.line, column: start.column };
  }
  diagnostics.push(entry);
}

const ERROR_KINDS = new Set(["parse-error", "thrown", "returned-error"]);

/**
 * Compile one SFC source for one backend. Returns
 * `{ code, map, diagnostics, postprocess }` — `code`/`map` are `null` when the
 * official compiler rejects the cell.
 */
function compileCell({ caseId, source, backend }) {
  const vapor = backend === "vapor";
  const compiler = vapor ? compilerVapor : compilerDom;
  const filename = `cases/${caseId}.vue`;
  const diagnostics = [];
  const onError = (error) => pushDiagnostic(diagnostics, "error", error);
  const onWarn = (error) => pushDiagnostic(diagnostics, "warning", error);

  const { descriptor, errors: parseErrors } = parse(source, { filename });
  for (const error of parseErrors) {
    pushDiagnostic(diagnostics, "parse-error", error);
  }

  let code = null;
  let map = null;
  let pipeline = null;
  let postprocess = null;

  try {
    if (descriptor.scriptSetup) {
      pipeline = "compileScript:inlineTemplate";
      const result = compileScript(descriptor, {
        id: caseId,
        vapor,
        inlineTemplate: true,
        fs: hermeticCompileFs,
        templateOptions: {
          compiler,
          vapor,
          sourceMap: true,
          compilerOptions: { onError, onWarn },
        },
      });
      code = result.content;
      map = result.map ?? null;
    } else {
      pipeline = "compileTemplate";
      const result = compileTemplate({
        source: descriptor.template?.content ?? "",
        filename,
        id: caseId,
        compiler,
        vapor,
        sourceMap: true,
        compilerOptions: { onError, onWarn },
      });
      code = result.code;
      map = result.map ?? null;
      for (const error of result.errors ?? []) {
        pushDiagnostic(diagnostics, "returned-error", error);
      }
    }
  } catch (error) {
    pushDiagnostic(diagnostics, "thrown", error);
  }

  const rejected = code === null || diagnostics.some((d) => ERROR_KINDS.has(d.kind));

  // The official SFC pipeline strips TS types after compileScript; mirror it
  // for `lang="ts"` cells so the vendored golden is plain JS. The compiler's
  // source map is chained through the strip (esbuild consumes the input
  // sourceMappingURL), keeping the vendored map anchored to the SFC.
  if (!rejected && descriptor.scriptSetup?.lang === "ts") {
    const inputMap = map
      ? `\n//# sourceMappingURL=data:application/json;base64,${Buffer.from(
          JSON.stringify(map),
        ).toString("base64")}`
      : "";
    return transform(code + inputMap, {
      loader: "ts",
      sourcemap: true,
      sourcefile: filename,
    }).then((stripped) => {
      postprocess = {
        tool: "esbuild",
        version: ESBUILD_VERSION,
        options: { loader: "ts", sourcemap: true, sourcefile: filename },
        reason:
          "strip TypeScript types from compileScript output (official SFC-loader pipeline parity)",
      };
      return {
        code: stripped.code,
        map: stripped.map ? JSON.parse(stripped.map) : null,
        diagnostics,
        pipeline,
        postprocess,
      };
    });
  }

  return Promise.resolve({
    code: rejected ? null : code,
    map: rejected ? null : map,
    diagnostics,
    pipeline,
    postprocess,
  });
}

/** Sorted unique `{ imported, alias }` rows imported from `vue` in `code`. */
function helperInventory(code) {
  const rows = new Map();
  const importRe = /import\s*\{([^}]*)\}\s*from\s*["']vue["']/g;
  let match;
  while ((match = importRe.exec(code)) !== null) {
    for (const specifier of match[1].split(",")) {
      const trimmed = specifier.trim();
      if (!trimmed) continue;
      const aliasMatch = /^(\S+)\s+as\s+(\S+)$/.exec(trimmed);
      const imported = aliasMatch ? aliasMatch[1] : trimmed;
      const alias = aliasMatch ? aliasMatch[2] : imported;
      rows.set(`${imported}${alias}`, { imported, alias });
    }
  }
  return [...rows.values()].sort((a, b) =>
    a.imported === b.imported
      ? a.alias.localeCompare(b.alias)
      : a.imported.localeCompare(b.imported),
  );
}

// ---------------------------------------------------------------------------
// Artifact assembly — one in-memory map of relpath → bytes for both modes.
// ---------------------------------------------------------------------------

async function buildArtifacts(versions) {
  /** @type {Map<string, string>} relpath (repo-relative POSIX) → file bytes */
  const files = new Map();
  const manifestCases = [];
  const totals = {
    vdom: { compiled: 0, rejected: 0 },
    vapor: { compiled: 0, rejected: 0 },
  };

  const caseIds = await discoverCases();
  const sourceShas = new Map();
  for (const caseId of caseIds) {
    const sfcAbs = assertCorpusPath(
      path.join(CASES_ROOT, ...caseId.split("/")) + ".vue",
      "case read",
    );
    const source = await fs.readFile(sfcAbs, "utf8");
    sourceShas.set(caseId, { source, sha: sha256(source) });
  }

  for (const caseId of caseIds) {
    const { source, sha: sourceSha } = sourceShas.get(caseId);
    const backends = {};

    for (const backend of BACKENDS) {
      const { code, map, diagnostics, pipeline, postprocess } = await compileCell({
        caseId,
        source,
        backend,
      });
      const disposition = code === null ? "rejected" : "compiled";
      totals[backend][disposition] += 1;

      const base = `crates/verter_vue_conformance/corpus/goldens/${VUE_ORACLE_VERSION}/${backend}/${caseId}`;
      const optionsSummary = {
        backend,
        vapor: backend === "vapor",
        filename: `cases/${caseId}.vue`,
        id: caseId,
        pipeline,
        sourceMap: true,
        postprocess,
      };
      const optionsSha = sha256(stableStringify(optionsSummary));

      let goldenRel = null;
      let mapRel = null;
      const artifacts = { code: null, map: null };
      if (disposition === "compiled") {
        goldenRel = `${base}.js`;
        mapRel = `${base}.map.json`;
        const goldenBytes = withTrailingNewline(code);
        const mapBytes = withTrailingNewline(JSON.stringify(map, null, 2));
        files.set(goldenRel, goldenBytes);
        files.set(mapRel, mapBytes);
        artifacts.code = {
          path: goldenRel.replace("crates/verter_vue_conformance/corpus/", ""),
          sha256: sha256(goldenBytes),
          bytes: Buffer.byteLength(goldenBytes, "utf8"),
        };
        artifacts.map = {
          path: mapRel.replace("crates/verter_vue_conformance/corpus/", ""),
          sha256: sha256(mapBytes),
        };
      }

      const metaRel = `${base}.meta.json`;
      const meta = {
        schema: META_SCHEMA_VERSION,
        caseId,
        backend,
        generator: {
          name: "@verter/vue-conformance-oracle",
          version: GENERATOR_VERSION,
        },
        versions,
        source: { path: `cases/${caseId}.vue`, sha256: sourceSha },
        options: { sha256: optionsSha, summary: optionsSummary },
        artifacts,
        disposition,
        diagnostics,
        helpers: disposition === "compiled" ? helperInventory(code) : [],
      };
      files.set(metaRel, withTrailingNewline(JSON.stringify(meta, null, 2)));

      backends[backend] = {
        disposition,
        golden: artifacts.code?.path ?? null,
        map: artifacts.map?.path ?? null,
        meta: metaRel.replace("crates/verter_vue_conformance/corpus/", ""),
      };
    }

    manifestCases.push({ id: caseId, sfc: `cases/${caseId}.vue`, backends });
  }

  const manifest = {
    schema: MANIFEST_SCHEMA_VERSION,
    generator: {
      name: "@verter/vue-conformance-oracle",
      version: GENERATOR_VERSION,
    },
    vueVersion: VUE_ORACLE_VERSION,
    packages: versions,
    cases: manifestCases,
  };
  files.set(
    "crates/verter_vue_conformance/corpus/manifest.json",
    withTrailingNewline(JSON.stringify(manifest, null, 2)),
  );

  return { files, totals, caseCount: caseIds.length };
}

async function discoverCases() {
  const ids = [];
  async function walk(dir, prefix) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        await walk(path.join(dir, entry.name), rel);
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        ids.push(rel.slice(0, -".vue".length));
      }
    }
  }
  await walk(CASES_ROOT, "");
  ids.sort();
  if (ids.length === 0) {
    throw new Error(`no corpus cases found under ${toPosixRelative(CASES_ROOT)}`);
  }
  return ids;
}

// ---------------------------------------------------------------------------
// Modes: clean regenerate (default) and --check (fresh-run vs committed bytes).
// ---------------------------------------------------------------------------

async function writeMode(artifacts) {
  assertCorpusWritePath(GOLDENS_ROOT, "goldens write root");
  await fs.rm(GOLDENS_ROOT, { recursive: true, force: true });
  for (const [rel, bytes] of [...artifacts.files.entries()].sort()) {
    const abs = assertCorpusWritePath(path.join(REPO_ROOT, ...rel.split("/")), "write");
    await fs.mkdir(path.dirname(abs), { recursive: true });
    await fs.writeFile(abs, bytes, "utf8");
  }
}

async function collectCommitted() {
  /** @type {Map<string, string>} */
  const committed = new Map();
  async function walk(dir) {
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return; // goldens tree absent — everything will report as missing
    }
    for (const entry of entries) {
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(abs);
      else if (entry.isFile()) {
        committed.set(toPosixRelative(abs), await fs.readFile(abs, "utf8"));
      }
    }
  }
  await walk(GOLDENS_ROOT);
  try {
    committed.set(
      "crates/verter_vue_conformance/corpus/manifest.json",
      await fs.readFile(MANIFEST_PATH, "utf8"),
    );
  } catch {
    // manifest absent — reported as missing below
  }
  return committed;
}

async function checkMode(artifacts) {
  const committed = await collectCommitted();
  const missing = [];
  const drifted = [];
  const stale = [];
  for (const [rel, bytes] of artifacts.files) {
    if (!committed.has(rel)) missing.push(rel);
    else if (committed.get(rel) !== bytes) drifted.push(rel);
  }
  for (const rel of committed.keys()) {
    if (!artifacts.files.has(rel)) stale.push(rel);
  }
  if (missing.length || drifted.length || stale.length) {
    for (const rel of missing.sort()) console.error(`MISSING (not committed): ${rel}`);
    for (const rel of drifted.sort()) console.error(`DRIFTED (bytes differ): ${rel}`);
    for (const rel of stale.sort()) console.error(`STALE (not regenerated): ${rel}`);
    throw new Error(
      `goldens check failed: ${missing.length} missing, ${drifted.length} drifted, ${stale.length} stale — ` +
        `regenerate with \`pnpm gen:vue-goldens\` and commit the result`,
    );
  }
  console.log(`goldens check OK: ${artifacts.files.size} committed artifacts match a fresh run`);
}

async function main() {
  const versions = assertPinnedToolchain();
  console.log(
    `vue conformance oracle: ${ORACLE_PACKAGES.map((p) => `${p}@${versions[p]}`).join(", ")}, esbuild@${versions.esbuild}`,
  );
  const artifacts = await buildArtifacts(versions);
  const { vdom, vapor } = artifacts.totals;
  if (CHECK_MODE) {
    await checkMode(artifacts);
  } else {
    await writeMode(artifacts);
    console.log(
      `generated ${artifacts.caseCount} cases ` +
        `(vdom: ${vdom.compiled} compiled / ${vdom.rejected} rejected, ` +
        `vapor: ${vapor.compiled} compiled / ${vapor.rejected} rejected) ` +
        `-> ${toPosixRelative(GOLDENS_ROOT)}`,
    );
  }
}

main().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
