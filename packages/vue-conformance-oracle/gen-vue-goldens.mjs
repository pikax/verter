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
 * TOPOLOGY: the goldens cover BOTH official emission topologies. The default
 * (`non-inline`) cells use the official NON-inline emission — the same
 * `_sfc_main`-shaped module with a SEPARATE render function that Verter
 * ships at runtime (`verter_session::assemble_vue_main_module`). Script-setup
 * cells: `compileScript({ inlineTemplate: false })` for the component object
 * + `compileTemplate({ compilerOptions: { bindingMetadata } })` for the
 * separate render fn (identical invocation shape for VDOM via compiler-dom
 * and Vapor via compiler-vapor — vapor's render is a separate exported
 * function just like VDOM's), assembled as
 * `[render import line][script component object][function render][_sfc_main.render
 * = render][export default _sfc_main]`. Template-only cells get the
 * bundler-equivalent `const _sfc_main = {}` + attach wrapper.
 *
 * The `inline` cells (VDOM script-setup cases only — Vapor inline is
 * deferred) use the official PRODUCTION topology: a single
 * `compileScript({ inlineTemplate: true, vapor: false })` call whose content
 * already carries the render closure inside `setup()` (official does NOT
 * split compileTemplate for inline), with `export default` rebound to
 * `const _sfc_main =` and a trailing `export default _sfc_main` — the same
 * module shape Verter's inline assembly ships. Inline goldens live under
 * `goldens/<ver>/vdom-inline/…`; each cell's `.meta.json` records
 * `topology` and `inlineTemplate`. The vendored inline `.map.json` is the
 * whole-module `compileScript` map (already SFC-absolute; no re-anchor).
 * The vendored non-inline `.map.json` is the render-fn (compileTemplate)
 * map; the script block's own map is not vendored.
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
 * TypeScript cells: `compileScript` keeps TS syntax in its emitted script
 * (the official SFC loaders strip types downstream). For `lang="ts"` cells
 * the emitted script is type-stripped with the pinned esbuild
 * (`{ loader: "ts" }` only — no format conversion, so PURE annotations and
 * the official export shape survive). Every stripped cell records the
 * post-process in its metadata. All other cells vendor the raw compiler
 * bytes untouched.
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
 * Source-map VLQ codec + SFC re-anchoring. `compileTemplate` emits original
 * positions RELATIVE to the template block content (the bundler re-anchors
 * them later); Verter's template maps are SFC-absolute. The oracle vendors
 * SFC-absolute maps so both sides share one coordinate basis — the same
 * re-anchoring `@vitejs/plugin-vue` applies (`loc.start.line - 1`, the
 * template tag's line; columns preserved).
 */
const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function decodeVlq(segment, pos) {
  let shift = 0;
  let value = 0;
  for (;;) {
    const digit = B64.indexOf(segment[pos.value++]);
    if (digit < 0) throw new Error(`invalid base64 in mappings segment ${segment}`);
    value |= (digit & 31) << shift;
    if ((digit & 32) === 0) break;
    shift += 5;
  }
  const negative = value & 1;
  value >>= 1;
  return negative ? -value : value;
}

function encodeVlq(out, value) {
  let vlq = value < 0 ? (-value << 1) | 1 : value << 1;
  for (;;) {
    let digit = vlq & 31;
    vlq >>>= 5;
    if (vlq > 0) digit |= 32;
    out.value += B64[digit];
    if (vlq === 0) break;
  }
}

function reAnchorMapLines(map, lineOffset) {
  if (!map || typeof map.mappings !== "string" || lineOffset === 0) return map;
  // Decode to absolute rows (fields: genCol, srcIdx, srcLine, srcCol, nameIdx).
  const rows = [];
  const prev = [0, 0, 0, 0, 0];
  for (const line of map.mappings.split(";")) {
    prev[0] = 0;
    const rowLine = [];
    if (line) {
      for (const segment of line.split(",")) {
        const pos = { value: 0 };
        const fields = [];
        while (pos.value < segment.length && fields.length < 5) {
          const delta = decodeVlq(segment, pos);
          prev[fields.length] += delta;
          fields.push(prev[fields.length]);
        }
        if (fields.length >= 4) fields[2] += lineOffset; // original line
        rowLine.push(fields);
      }
    }
    rows.push(rowLine);
  }
  // Re-encode (field 0 resets per line; fields 1..4 run across lines).
  const encPrev = [0, 0, 0, 0, 0];
  const mappings = rows
    .map((rowLine) => {
      encPrev[0] = 0;
      return rowLine
        .map((fields) => {
          const out = { value: "" };
          for (let i = 0; i < fields.length; i += 1) {
            encodeVlq(out, fields[i] - encPrev[i]);
            encPrev[i] = fields[i];
          }
          return out.value;
        })
        .join(",");
    })
    .join(";");
  return { ...map, mappings };
}

function splitRenderImport(code) {
  const match = code.match(/^import \{[^}]*\} from ["']vue["'];?[ \t]*\n/);
  if (!match) {
    return { importLine: null, body: code };
  }
  return {
    importLine: match[0].trimEnd(),
    body: code.slice(match[0].length).replace(/^\n+/, ""),
  };
}

/**
 * Assemble the official INLINE module in the shape a bundler host (and
 * Verter's inline `assemble_vue_main_module`) ships: the single
 * `compileScript({ inlineTemplate: true })` content (which already carries
 * helper imports, module-scope hoists, and the render closure inside
 * `setup()`) with `export default` rebound to `const _sfc_main =` and a
 * trailing default export.
 */
function assembleInline({ scriptCode }) {
  return scriptCode.replace("export default", "const _sfc_main =") + "\nexport default _sfc_main";
}

/**
 * Compile one SFC source for the INLINE topology (VDOM script-setup only).
 * Official production shape: ONE `compileScript({ inlineTemplate: true })`
 * call — no separate compileTemplate, no bindingMetadata split.
 */
async function compileInlineCell({ caseId, source }) {
  const filename = `cases/${caseId}.vue`;
  const diagnostics = [];

  const { descriptor, errors: parseErrors } = parse(source, { filename });
  for (const error of parseErrors) {
    pushDiagnostic(diagnostics, "parse-error", error);
  }

  let code = null;
  let map = null;
  let pipeline = null;
  let postprocess = null;

  try {
    pipeline = "compileScript:inlineTemplate";
    const script = compileScript(descriptor, {
      id: caseId,
      vapor: false,
      inlineTemplate: true,
      fs: hermeticCompileFs,
    });
    let scriptCode = script.content;
    // Mirror the official SFC-loader TS strip for `lang="ts"` cells (same
    // post-process the non-inline cells get).
    if (descriptor.scriptSetup.lang === "ts") {
      const stripped = await transform(scriptCode, { loader: "ts" });
      scriptCode = stripped.code;
      postprocess = {
        tool: "esbuild",
        version: ESBUILD_VERSION,
        options: { loader: "ts" },
        reason:
          "strip TypeScript types from compileScript output (official SFC-loader pipeline parity)",
      };
    }
    code = assembleInline({ scriptCode });
    // The compileScript map is the whole-module map and already SFC-absolute
    // (no template-relative re-anchor needed).
    map = script.map ?? null;
  } catch (error) {
    pushDiagnostic(diagnostics, "thrown", error);
  }

  const rejected = code === null || diagnostics.some((d) => ERROR_KINDS.has(d.kind));

  return {
    code: rejected ? null : code,
    map: rejected ? null : map,
    diagnostics,
    pipeline,
    postprocess,
  };
}

/**
 * Assemble the official NON-inline module in the shape a bundler host (and
 * Verter's `assemble_vue_main_module`) ships: the template helper import
 * line first, then the component object, then the separate render function,
 * then the attach + default export. This is the apples-to-apples counterpart
 * of Verter's shipped runtime Main — the official `inlineTemplate: true`
 * topology (setup returns the render closure) is a DIFFERENT, behaviorally
 * equivalent shape that is not what Verter emits (tracked as a future
 * feature in `docs/arch/next/vue-inline-template-runtime.md`).
 *
 * - `renderCode` is a full `compileTemplate` module; its import line is
 *   hoisted to the top and its `export function render` becomes the
 *   attachable `function render`.
 * - `scriptCode` (when present) has its `export default` rebound to
 *   `const _sfc_main =`.
 */
function assembleNonInline({ importLine, body, scriptCode }) {
  const renderBody = body.replace("export function render(", "function render(");
  const parts = [];
  if (importLine) parts.push(importLine);
  if (scriptCode != null) {
    parts.push(scriptCode.replace("export default", "const _sfc_main ="));
  } else {
    parts.push("const _sfc_main = {}");
  }
  parts.push(renderBody, "_sfc_main.render = render", "export default _sfc_main");
  return parts.join("\n");
}

/**
 * Compile one SFC source for one backend. Returns
 * `{ code, map, diagnostics, postprocess }` — `code`/`map` are `null` when the
 * official compiler rejects the cell.
 */
async function compileCell({ caseId, source, backend }) {
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
      pipeline = "compileScript:nonInline+compileTemplate";
      // The render is compiled separately with the script's bindings — the
      // official non-inline invocation, identical in shape for VDOM
      // (compiler-dom) and Vapor (compiler-vapor; vapor's render is a
      // separate exported function just like VDOM's).
      const script = compileScript(descriptor, {
        id: caseId,
        vapor,
        inlineTemplate: false,
        fs: hermeticCompileFs,
      });
      const render = compileTemplate({
        source: descriptor.template?.content ?? "",
        filename,
        id: caseId,
        compiler,
        vapor,
        sourceMap: true,
        compilerOptions: {
          bindingMetadata: script.bindings,
          onError,
          onWarn,
        },
      });
      for (const error of render.errors ?? []) {
        pushDiagnostic(diagnostics, "returned-error", error);
      }
      let scriptCode = script.content;
      // The official SFC pipeline strips TS types after compileScript;
      // mirror it for `lang="ts"` cells so the vendored golden is plain JS
      // (the render fn is plain JS already; only the script needs the strip).
      if (descriptor.scriptSetup.lang === "ts") {
        const stripped = await transform(scriptCode, { loader: "ts" });
        scriptCode = stripped.code;
        postprocess = {
          tool: "esbuild",
          version: ESBUILD_VERSION,
          options: { loader: "ts" },
          reason:
            "strip TypeScript types from compileScript output (official SFC-loader pipeline parity)",
        };
      }
      const { importLine, body } = splitRenderImport(render.code);
      code = assembleNonInline({ importLine, body, scriptCode });
      // compileTemplate original positions are template-content-relative;
      // re-anchor them SFC-absolute (the same offset the bundler applies).
      map = reAnchorMapLines(render.map ?? null, (descriptor.template?.loc.start.line ?? 1) - 1);
    } else {
      pipeline = "compileTemplate";
      const render = compileTemplate({
        source: descriptor.template?.content ?? "",
        filename,
        id: caseId,
        compiler,
        vapor,
        sourceMap: true,
        compilerOptions: { onError, onWarn },
      });
      for (const error of render.errors ?? []) {
        pushDiagnostic(diagnostics, "returned-error", error);
      }
      const { importLine, body } = splitRenderImport(render.code);
      code = assembleNonInline({ importLine, body, scriptCode: null });
      // compileTemplate original positions are template-content-relative;
      // re-anchor them SFC-absolute (the same offset the bundler applies).
      map = reAnchorMapLines(render.map ?? null, (descriptor.template?.loc.start.line ?? 1) - 1);
    }
  } catch (error) {
    pushDiagnostic(diagnostics, "thrown", error);
  }

  const rejected = code === null || diagnostics.some((d) => ERROR_KINDS.has(d.kind));

  return {
    code: rejected ? null : code,
    map: rejected ? null : map,
    diagnostics,
    pipeline,
    postprocess,
  };
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
    vdomInline: { compiled: 0, rejected: 0 },
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
    const inlineBackends = {};

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
        topology: "non-inline",
        inlineTemplate: false,
        bindingMetadata: pipeline === "compileScript:nonInline+compileTemplate",
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
        topology: "non-inline",
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

    // Inline topology cells (official production shape): VDOM + script-setup
    // cases only — Vapor inline is deferred, and template-only SFCs have no
    // `setup()` to inline into (their production shape IS the non-inline
    // shape, so an inline cell would be a byte-for-byte duplicate).
    const { descriptor: topologyProbe } = parse(source, { filename: `cases/${caseId}.vue` });
    if (topologyProbe.scriptSetup) {
      const { code, map, diagnostics, pipeline, postprocess } = await compileInlineCell({
        caseId,
        source,
      });
      const disposition = code === null ? "rejected" : "compiled";
      totals.vdomInline[disposition] += 1;

      const base = `crates/verter_vue_conformance/corpus/goldens/${VUE_ORACLE_VERSION}/vdom-inline/${caseId}`;
      const optionsSummary = {
        backend: "vdom",
        vapor: false,
        filename: `cases/${caseId}.vue`,
        id: caseId,
        pipeline,
        topology: "inline",
        inlineTemplate: true,
        bindingMetadata: false,
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
        backend: "vdom",
        topology: "inline",
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

      inlineBackends.vdom = {
        disposition,
        golden: artifacts.code?.path ?? null,
        map: artifacts.map?.path ?? null,
        meta: metaRel.replace("crates/verter_vue_conformance/corpus/", ""),
      };
    }

    const entry = { id: caseId, sfc: `cases/${caseId}.vue`, backends };
    if (Object.keys(inlineBackends).length > 0) {
      entry.inlineBackends = inlineBackends;
    }
    manifestCases.push(entry);
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
  const { vdom, vapor, vdomInline } = artifacts.totals;
  if (CHECK_MODE) {
    await checkMode(artifacts);
  } else {
    await writeMode(artifacts);
    console.log(
      `generated ${artifacts.caseCount} cases ` +
        `(vdom: ${vdom.compiled} compiled / ${vdom.rejected} rejected, ` +
        `vapor: ${vapor.compiled} compiled / ${vapor.rejected} rejected, ` +
        `vdom-inline: ${vdomInline.compiled} compiled / ${vdomInline.rejected} rejected) ` +
        `-> ${toPosixRelative(GOLDENS_ROOT)}`,
    );
  }
}

main().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
