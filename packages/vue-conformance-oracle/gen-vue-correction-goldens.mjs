#!/usr/bin/env node
/**
 * Raw Vue golden generator for the route-correction corpus
 * (`crates/verter_vue_conformance/corrections`).
 *
 * Compiles every `cases/<id>.vue` with an explicitly supplied oracle install
 * and writes `goldens/<vue-version>/<mode>/<id>.js`, plus two standalone
 * artifacts with their compile options recorded beside them:
 *
 * - `script/force-js-catch-parameter.js`: `compileScript` output for a typed
 *   catch binding, erased with TypeScript `transpileModule`.
 * - `server/server-slot-handler-one-statement.js`: the server render of a
 *   compiler regression fixture.
 *
 * `probes.json` is rewritten in place. Its human-authored fields (bucket,
 * targets, checks) and the golden modes of each probe are kept as committed;
 * only `sourceSha256`, `vueVersion` and each golden's path, hash and
 * diagnostics are regenerated.
 *
 * Invocation: `filename: "App.vue"`, `id: "test"`, development mode. A
 * separate template receives the script's binding metadata; server templates
 * use `ssr: true` with the SFC's css variables; inline output is
 * `compileScript({ inlineTemplate: true })`. TypeScript setup blocks enable
 * the TypeScript expression plugin. Raw Vue TypeScript output is kept.
 *
 * Deterministic: sorted artifacts, LF text, no timestamps or absolute paths.
 *
 * Usage:
 *   node gen-vue-correction-goldens.mjs <oracle-install-dir>          # write
 *   node gen-vue-correction-goldens.mjs <oracle-install-dir> --check  # verify
 */

import { createHash } from "node:crypto";
import { promises as fs, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PACKAGE_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(PACKAGE_DIR, "..", "..");
const CORRECTIONS_ROOT = path.join(REPO_ROOT, "crates", "verter_vue_conformance", "corrections");
const CASES_ROOT = path.join(CORRECTIONS_ROOT, "cases");
const GOLDENS_ROOT = path.join(CORRECTIONS_ROOT, "goldens");
const PROBES_PATH = path.join(CORRECTIONS_ROOT, "probes.json");

/** The script golden's erasure contract is pinned to this TypeScript. */
const TYPESCRIPT_VERSION = "5.9.3";
/** Compiler packages that must resolve at the same version as compiler-sfc. */
const VUE_PACKAGES = [
  "@vue/compiler-core",
  "@vue/compiler-dom",
  "@vue/compiler-ssr",
  "@vue/shared",
];
const FILENAME = "App.vue";
const DEFAULT_ID = "test";
const MODES = new Set(["client", "client-inline", "server"]);

/** A typed catch binding in a TypeScript setup block. */
const SCRIPT_SOURCE =
  "<script setup lang=\"ts\">function load() { try { JSON.parse('') } catch (err: unknown) { report(err) } }\n" +
  "function report(err: unknown) {}</script>\n" +
  '<template><button @click="load()">x</button></template>\n';
const SCRIPT_STEM = "script/force-js-catch-parameter";
const TRANSPILE_OPTIONS = { target: "ESNext", module: "ESNext" };

const ONE_STATEMENT_SOURCE =
  "crates/verter_compiler/tests/cases/sfc_regression_fixtures/server_slot_handler_one_statement.vue";
const ONE_STATEMENT_STEM = "server/server-slot-handler-one-statement";

// ---------------------------------------------------------------------------
// Command line and oracle toolchain.
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const CHECK_MODE = args.includes("--check");
const positional = args.filter((arg) => !arg.startsWith("--"));
if (positional.length !== 1 || args.some((arg) => arg.startsWith("--") && arg !== "--check")) {
  console.error("usage: node gen-vue-correction-goldens.mjs <oracle-install-dir> [--check]");
  process.exit(2);
}

function loadOracle(installDir) {
  const oracle = createRequire(path.resolve(installDir, "package.json"));
  const version = (pkg, from = oracle) => {
    try {
      return from(`${pkg}/package.json`).version;
    } catch {
      throw new Error(`${pkg} is not installed in the oracle directory ${installDir}`);
    }
  };
  const vueVersion = version("@vue/compiler-sfc");
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(vueVersion)) {
    throw new Error(`unexpected @vue/compiler-sfc version: ${vueVersion}`);
  }
  // Resolve the compiler's own dependencies from compiler-sfc, so a hoisted
  // copy at another version cannot slip into the build.
  const sfcRequire = createRequire(oracle.resolve("@vue/compiler-sfc/package.json"));
  for (const pkg of VUE_PACKAGES) {
    const found = version(pkg, sfcRequire);
    if (found !== vueVersion) {
      throw new Error(`${pkg} is ${found}; it must match @vue/compiler-sfc ${vueVersion}`);
    }
  }
  const typescriptVersion = version("typescript");
  if (typescriptVersion !== TYPESCRIPT_VERSION) {
    throw new Error(`typescript is ${typescriptVersion}; it must be ${TYPESCRIPT_VERSION}`);
  }
  return {
    sfc: oracle("@vue/compiler-sfc"),
    ts: oracle("typescript"),
    vueVersion,
    typescriptVersion,
  };
}

// ---------------------------------------------------------------------------
// Small deterministic utilities.
// ---------------------------------------------------------------------------

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function readText(absolute) {
  return readFileSync(absolute, "utf8").replace(/\r\n/g, "\n");
}

function toJson(value) {
  return JSON.stringify(value, null, 2) + "\n";
}

function withTrailingNewline(text) {
  return text.endsWith("\n") ? text : text + "\n";
}

/** Repo-relative POSIX path. */
function toPosixRelative(absolute, root = REPO_ROOT) {
  return path.relative(root, absolute).split(path.sep).join("/");
}

function diagnostic(kind, error) {
  if (typeof error === "string") return { kind, message: error };
  const entry = { kind, message: String(error?.message ?? error) };
  if (typeof error?.code === "number" || typeof error?.code === "string") {
    entry.code = error.code;
  }
  const start = error?.loc?.start;
  if (start && Number.isInteger(start.line) && Number.isInteger(start.column)) {
    entry.loc = { line: start.line, column: start.column };
  }
  return entry;
}

// ---------------------------------------------------------------------------
// Compilation.
// ---------------------------------------------------------------------------

/**
 * Parse one SFC and derive its per-SFC options from the descriptor. A
 * `<style scoped>` SFC takes the scope id Verter derives for `App.vue`: the
 * first eight hex digits of SHA-256 over the component name. Diagnostics are
 * parse and compile errors; compiler warnings are advisory and not recorded.
 */
function prepare(sfc, source) {
  const { descriptor, errors } = sfc.parse(source, { filename: FILENAME });
  const script = descriptor.scriptSetup ?? descriptor.script;
  const scoped = descriptor.styles.some((style) => style.scoped);
  const id = scoped ? sha256(path.basename(FILENAME, ".vue")).slice(0, 8) : DEFAULT_ID;
  const bindings = script
    ? sfc.compileScript(descriptor, { id, isProd: false }).bindings
    : undefined;
  return {
    sfc,
    descriptor,
    id,
    scoped,
    isTS: script?.lang === "ts",
    bindings,
    parseDiagnostics: errors.map((error) => diagnostic("parse-error", error)),
  };
}

/** The separate-template options, in the order the metadata records them. */
function templateOptions(prepared, ssr) {
  const { descriptor, id, scoped, isTS, bindings } = prepared;
  const compilerOptions = {};
  if (bindings) compilerOptions.bindingMetadata = bindings;
  if (isTS) compilerOptions.expressionPlugins = ["typescript"];
  return {
    filename: FILENAME,
    id,
    ...(scoped ? { scoped: true } : {}),
    ...(ssr ? { ssr: true, ssrCssVars: descriptor.cssVars } : {}),
    compilerOptions,
  };
}

function compileSeparate(prepared, ssr) {
  const options = templateOptions(prepared, ssr);
  const result = prepared.sfc.compileTemplate({
    ...options,
    source: prepared.descriptor.template?.content ?? "",
  });
  return {
    options,
    code: result.code,
    diagnostics: [
      ...prepared.parseDiagnostics,
      ...result.errors.map((error) => diagnostic("error", error)),
    ],
  };
}

/** `compileScript` throws on inline template errors, so none are returned. */
function compileInline(prepared) {
  const result = prepared.sfc.compileScript(prepared.descriptor, {
    id: prepared.id,
    isProd: false,
    inlineTemplate: true,
  });
  return { code: result.content, diagnostics: [...prepared.parseDiagnostics] };
}

function compileMode(prepared, mode) {
  if (mode === "client-inline") return compileInline(prepared);
  return compileSeparate(prepared, mode === "server");
}

// ---------------------------------------------------------------------------
// Artifacts.
// ---------------------------------------------------------------------------

async function caseIds() {
  const entries = await fs.readdir(CASES_ROOT, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".vue"))
    .map((entry) => entry.name.slice(0, -".vue".length))
    .sort();
}

/**
 * Regenerate every artifact. Returns a map from corrections-relative POSIX
 * path to its bytes.
 */
async function buildArtifacts(oracle) {
  const { sfc, ts, vueVersion, typescriptVersion } = oracle;
  const versionRoot = `goldens/${vueVersion}`;
  /** @type {Map<string, string>} */
  const files = new Map();

  const committed = JSON.parse(readText(PROBES_PATH));
  const ids = await caseIds();
  const listed = committed.map((probe) => probe.id);
  const unlisted = ids.filter((id) => !listed.includes(id));
  const missing = listed.filter((id) => !ids.includes(id));
  if (unlisted.length || missing.length || new Set(listed).size !== listed.length) {
    throw new Error(
      `probes.json and cases/ disagree: unlisted cases [${unlisted}], ` +
        `probes without a case [${missing}], or a duplicate probe id`,
    );
  }

  const probes = committed.map((probe) => {
    const source = readText(path.join(CASES_ROOT, `${probe.id}.vue`));
    const prepared = prepare(sfc, source);
    const goldens = probe.goldens.map(({ mode }) => {
      if (!MODES.has(mode)) throw new Error(`${probe.id}: unknown golden mode ${mode}`);
      const { code, diagnostics } = compileMode(prepared, mode);
      const text = withTrailingNewline(code);
      const rel = `${versionRoot}/${mode}/${probe.id}.js`;
      files.set(rel, text);
      return { mode, path: rel, sha256: sha256(text), diagnostics };
    });
    const regenerated = { sourceSha256: sha256(source), vueVersion, goldens };
    return Object.fromEntries(
      Object.keys(probe).map((key) => [key, key in regenerated ? regenerated[key] : probe[key]]),
    );
  });
  files.set("probes.json", toJson(probes));

  // The script golden: Vue's script output, erased by TypeScript.
  {
    const prepared = prepare(sfc, SCRIPT_SOURCE);
    const compileScriptOptions = { id: prepared.id, isProd: false };
    const script = sfc.compileScript(prepared.descriptor, compileScriptOptions);
    const erased = ts.transpileModule(script.content, { compilerOptions: TRANSPILE_OPTIONS });
    const code = erased.outputText;
    const diagnostics = [
      ...prepared.parseDiagnostics,
      ...(erased.diagnostics ?? []).map((error) => ({
        kind: "typescript",
        message: ts.flattenDiagnosticMessageText(error.messageText, "\n"),
        code: error.code,
      })),
    ];
    files.set(`${versionRoot}/${SCRIPT_STEM}.js`, code);
    files.set(
      `${versionRoot}/${SCRIPT_STEM}.meta.json`,
      toJson({
        vueVersion,
        typescriptVersion,
        sourceSha256: sha256(SCRIPT_SOURCE),
        codeSha256: sha256(code),
        options: { compileScript: compileScriptOptions, transpileModule: TRANSPILE_OPTIONS },
        diagnostics,
      }),
    );
  }

  // The one-statement server golden keeps compileTemplate's code verbatim.
  {
    const source = readText(path.join(REPO_ROOT, ...ONE_STATEMENT_SOURCE.split("/")));
    const { options, code, diagnostics } = compileSeparate(prepare(sfc, source), true);
    files.set(`${versionRoot}/${ONE_STATEMENT_STEM}.js`, code);
    files.set(
      `${versionRoot}/${ONE_STATEMENT_STEM}.meta.json`,
      toJson({
        vueVersion,
        sourcePath: ONE_STATEMENT_SOURCE,
        sourceSha256: sha256(source),
        codeSha256: sha256(code),
        options,
        diagnostics,
      }),
    );
  }

  return new Map([...files].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

// ---------------------------------------------------------------------------
// Modes: write and --check.
// ---------------------------------------------------------------------------

/** Every committed file under `goldens/`, plus `probes.json`. */
async function collectCommitted() {
  /** @type {Map<string, string>} */
  const committed = new Map();
  async function walk(dir) {
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(abs);
      else if (entry.isFile()) {
        committed.set(toPosixRelative(abs, CORRECTIONS_ROOT), await fs.readFile(abs, "utf8"));
      }
    }
  }
  await walk(GOLDENS_ROOT);
  try {
    committed.set("probes.json", await fs.readFile(PROBES_PATH, "utf8"));
  } catch {
    // reported as missing
  }
  return committed;
}

function correctionsPath(rel) {
  const abs = path.resolve(CORRECTIONS_ROOT, ...rel.split("/"));
  if (!abs.startsWith(CORRECTIONS_ROOT + path.sep)) {
    throw new Error(`refusing to write outside corrections/: ${rel}`);
  }
  return abs;
}

/** Replace `goldens/<version>/` and `probes.json`; other versions are kept. */
async function writeMode(files, vueVersion) {
  await fs.rm(correctionsPath(`goldens/${vueVersion}`), { recursive: true, force: true });
  for (const [rel, bytes] of files) {
    const abs = correctionsPath(rel);
    await fs.mkdir(path.dirname(abs), { recursive: true });
    await fs.writeFile(abs, bytes, "utf8");
  }
  console.log(
    `generated ${files.size} correction artifacts with @vue/compiler-sfc ${vueVersion} ` +
      `-> ${toPosixRelative(path.join(GOLDENS_ROOT, vueVersion))}`,
  );
}

/** Fail on missing, drifted or stale artifacts anywhere under `goldens/`. */
async function checkMode(files, vueVersion) {
  const committed = await collectCommitted();
  const missing = [];
  const drifted = [];
  const stale = [];
  for (const [rel, bytes] of files) {
    if (!committed.has(rel)) missing.push(rel);
    else if (committed.get(rel) !== bytes) drifted.push(rel);
  }
  for (const rel of committed.keys()) {
    if (!files.has(rel)) stale.push(rel);
  }
  for (const rel of missing) console.error(`MISSING (not committed): ${rel}`);
  for (const rel of drifted) console.error(`DRIFTED (bytes differ): ${rel}`);
  for (const rel of stale.sort()) console.error(`STALE (not regenerated): ${rel}`);
  if (missing.length || drifted.length || stale.length) {
    throw new Error(
      `correction goldens check failed: ${missing.length} missing, ${drifted.length} drifted, ` +
        `${stale.length} stale`,
    );
  }
  console.log(
    `correction goldens check OK: ${files.size} committed artifacts match a fresh run ` +
      `with @vue/compiler-sfc ${vueVersion}`,
  );
}

async function main() {
  const oracle = loadOracle(positional[0]);
  console.log(
    `correction oracle: @vue/compiler-sfc@${oracle.vueVersion}, typescript@${oracle.typescriptVersion}`,
  );
  const files = await buildArtifacts(oracle);
  if (CHECK_MODE) await checkMode(files, oracle.vueVersion);
  else await writeMode(files, oracle.vueVersion);
}

main().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
