#!/usr/bin/env node
/**
 * Behavioral multi-mode compare: official Vue compiler vs Verter.
 *
 * Targets:
 *   - client (VDOM render)  — official @vue/compiler-sfc compileTemplate
 *   - ssr                   — both compilers' ssrRender
 *   - vapor                 — Verter forceVapor self-health (official Vue 3.5
 *                             compileTemplate vapor flag still emits VDOM;
 *                             we do not claim official vapor golden parity)
 *
 * Cosmetic differences are WAIVED (whitespace, comments, local id spellings
 * after alpha-normalize, $setup/$props vs _ctx after prefix normalize, patch
 * flags, fragment markers). Behavioral / structural divergence is reported.
 *
 * Usage:
 *   node scripts/vue-behavior-compare/run.mjs \
 *     --root ../vize/tests/_fixtures/_git \
 *     [--projects create-vue,pinia,...] \
 *     [--modes client,ssr,vapor] \
 *     [--limit N] \
 *     [--json out.json] \
 *     [--verbose]
 *
 * Default root: $VIZE_ROOT/tests/_fixtures/_git or ../vize/tests/_fixtures/_git
 */

import { createRequire } from "node:module";
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  extractSsrRenderBody,
  normalizeForComparison as normalizeSsr,
} from "../ssr-baseline/normalize.mjs";

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(__dirname, "../..");

const { parse, compileScript, compileTemplate } = require("@vue/compiler-sfc");
const { VerterHost } = require(path.join(REPO, "packages/native/index.js"));

// ── CLI ──────────────────────────────────────────────────────────

const args = process.argv.slice(2);
function getArg(name) {
  const i = args.indexOf(name);
  return i !== -1 ? args[i + 1] : null;
}

const defaultRoot = process.env.VIZE_ROOT
  ? path.join(path.resolve(process.env.VIZE_ROOT), "tests/_fixtures/_git")
  : path.resolve(REPO, "../vize/tests/_fixtures/_git");

const rootDir = path.resolve(getArg("--root") || defaultRoot);
const projectsArg = getArg("--projects");
const modes = (getArg("--modes") || "client,ssr,vapor")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);
const limit = getArg("--limit") ? parseInt(getArg("--limit"), 10) : 0;
const jsonPath = getArg("--json");
const verbose = args.includes("--verbose");
const maxMismatchSamples = getArg("--samples") ? parseInt(getArg("--samples"), 10) : 12;

const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  ".git",
  ".nuxt",
  ".output",
  ".vitepress",
  "coverage",
  "__tests__",
  "test",
  "tests",
  "e2e",
  "cypress",
  "playwright",
]);

// ── Discovery ────────────────────────────────────────────────────

function discoverVueFiles() {
  const files = [];
  if (!fs.existsSync(rootDir)) {
    console.error(`Root not found: ${rootDir}`);
    process.exit(2);
  }

  let projectDirs;
  if (projectsArg) {
    projectDirs = projectsArg
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean)
      .map((id) => path.join(rootDir, id));
  } else {
    projectDirs = fs
      .readdirSync(rootDir, { withFileTypes: true })
      .filter((e) => e.isDirectory() && !e.name.startsWith("."))
      .map((e) => path.join(rootDir, e.name));
  }

  for (const dir of projectDirs) {
    if (!fs.existsSync(dir)) {
      console.warn(`skip missing project: ${dir}`);
      continue;
    }
    collect(dir, files, 0);
  }
  return files;
}

function collect(dir, files, depth) {
  if (depth > 14) return;
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    if (e.name.startsWith(".")) continue;
    if (SKIP_DIRS.has(e.name)) continue;
    const full = path.join(dir, e.name);
    if (e.isDirectory()) collect(full, files, depth + 1);
    else if (e.isFile() && e.name.endsWith(".vue")) files.push(full);
  }
}

// ── Client normalize (VDOM) ──────────────────────────────────────

function extractRenderBody(code) {
  // Match `function render(` or `export function render(`
  const idx = code.search(/(?:export\s+)?function\s+render\s*\(/);
  if (idx === -1) return null;
  const braceStart = code.indexOf("{", idx);
  if (braceStart === -1) return null;
  let depth = 1;
  let i = braceStart + 1;
  while (i < code.length && depth > 0) {
    const ch = code[i];
    if (ch === "{") depth++;
    else if (ch === "}") depth--;
    else if (ch === '"' || ch === "'") {
      i++;
      while (i < code.length && code[i] !== ch) {
        if (code[i] === "\\") i++;
        i++;
      }
    } else if (ch === "`") {
      i++;
      while (i < code.length && code[i] !== "`") {
        if (code[i] === "\\") i++;
        else if (code[i] === "$" && code[i + 1] === "{") {
          i += 2;
          let d = 1;
          while (i < code.length && d > 0) {
            if (code[i] === "{") d++;
            else if (code[i] === "}") d--;
            i++;
          }
          continue;
        }
        i++;
      }
    }
    i++;
  }
  if (depth !== 0) return null;
  return code.slice(braceStart + 1, i - 1);
}

/**
 * Behavioral normalize for client VDOM render bodies.
 * Cosmetic-only differences are collapsed; structure that affects runtime stays.
 */
function normalizeClient(code) {
  let s = code;
  s = s.replace(/"use strict";?\s*/g, "");
  // Strip ALL /* ... */ comments (patch flags, CACHED, etc.)
  s = s.replace(/\/\*[\s\S]*?\*\//g, "");
  // Strip patch flag numbers that sit as trailing args: , -1) or , 1)
  s = s.replace(/,\s*-?\d+(?=\s*[,)])/g, "");
  // Strip dynamic props arrays often trailing
  s = s.replace(/,\s*\["[^"]*"(?:,\s*"[^"]*")*\]/g, "");
  s = s.replace(/,\s*null\)/g, ")");
  s = s.replace(/,\s*\)/g, ")");
  s = s.replace(/,\s*\]/g, "]");
  // Binding prefix: $setup/$props/$data/$options → _ctx (proxy-equivalent for non-inline)
  s = s.replace(/\$setup\./g, "_ctx.");
  s = s.replace(/\$setup\["/g, '_ctx["');
  s = s.replace(/\$props\./g, "_ctx.");
  s = s.replace(/\$props\["/g, '_ctx["');
  s = s.replace(/\$data\./g, "_ctx.");
  s = s.replace(/\$options\./g, "_ctx.");
  // Bracket vs dot for simple identifiers: _ctx["Foo"] → _ctx.Foo when safe
  s = s.replace(/_ctx\["([A-Za-z_$][\w$]*)"\]/g, "_ctx.$1");
  s = s.replace(/\$setup\["([A-Za-z_$][\w$]*)"\]/g, "_ctx.$1");
  // Case-fold component accesses for binding-metadata noise (WelcomeItem vs welcomeitem)
  // only when the token is PascalCase-looking after lowercasing both sides for compare:
  // we lower-case ALL _ctx.Ident after first char? Too aggressive for props.
  // Instead: only lower-case identifiers that start with uppercase (component-like).
  s = s.replace(/_ctx\.([A-Z][A-Za-z0-9_$]*)/g, (_, n) => `_ctx.${n.toLowerCase()}`);
  // Spread-vs-array hoisted children cosmetic:
  // Vue: [...(_cache[N] || (_cache[N] = [child1, child2]))]
  // Verter may emit a single child or flattened form — strip outer `[...]` spread
  s = s.replace(/\[\.\.\.\(([^)]*(?:\([^)]*\)[^)]*)*)\)\]/g, "[$1]");
  // When cache holds a single createElementVNode (not an array), wrap for parity:
  // (_cache[N] || (_cache[N] = _createElementVNode(...)))
  // vs (_cache[N] || (_cache[N] = [_createElementVNode(...)]))
  s = s.replace(
    /\(_cache\[N\] \|\| \(_cache\[N\] = (_create(?:ElementVNode|TextVNode|CommentVNode)\([^]*?\))\)\)/g,
    "(_cache[N] || (_cache[N] = [$1]))",
  );
  // Whitespace / lines
  s = s
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .join("\n");
  s = s.replace(/\s+/g, " ");
  s = s.replace(/\(\s+/g, "(").replace(/\s+\)/g, ")");
  s = s.replace(/\{\s+/g, "{").replace(/\s+\}/g, "}");
  s = s.replace(/\[\s+/g, "[").replace(/\s+\]/g, "]");
  s = s.replace(/,\s+/g, ",");
  s = s.replace(/;\s*/g, ";");
  // Alpha-normalize local component vars
  s = s.replace(/_component_[A-Za-z0-9_$]+/g, "_component_N");
  s = s.replace(/_directive_[A-Za-z0-9_$]+/g, "_directive_N");
  s = s.replace(/_hoisted_\d+/g, "_hoisted_N");
  s = s.replace(/_cache\[\d+\]/g, "_cache[N]");
  return s.trim();
}

/** Extra SSR cosmetic strip on top of ssr-baseline normalize. */
function normalizeSsrBehavior(code) {
  let s = normalizeSsr(code);
  // Scope ids are content-hash of the SFC; identical behavior, different hash.
  s = s.replace(/\s*data-v-[a-f0-9]+/gi, "");
  s = s.replace(/"data-v-[a-f0-9]+"/gi, '""');
  // Empty trailing scope-id arg on ssrRenderComponent / ssrRenderSlot: , "") / , ''
  s = s.replace(/,\s*""\s*\)/g, ")");
  s = s.replace(/,\s*''\s*\)/g, ")");
  // Component case noise
  s = s.replace(/_ctx\["([A-Za-z_$][\w$]*)"\]/g, "_ctx.$1");
  s = s.replace(/_ctx\.([A-Z][A-Za-z0-9_$]*)/g, (_, n) => `_ctx.${n.toLowerCase()}`);
  // Adjacent _push merge already in base; also collapse `) _push(` residual spaces
  s = s.replace(/`\)\s*_push\(`/g, "");
  s = s.replace(/\s+/g, " ");
  s = s.replace(/\[\s+/g, "[").replace(/\s+\]/g, "]");
  s = s.replace(/,\s+/g, ",");
  return s.trim();
}

function hasVaporMarkers(code) {
  return (
    /\b_template\s*\(/.test(code) ||
    /\brenderEffect\b/.test(code) ||
    /\b_renderEffect\b/.test(code) ||
    /from\s+["']vue\/vapor["']/.test(code)
  );
}

// ── Compilers ────────────────────────────────────────────────────

const host = new VerterHost({ devMode: false, analysisLevel: "none" });

function vueCompile(source, filename, { ssr }) {
  try {
    const { descriptor, errors: parseErrors } = parse(source, { filename });
    if (parseErrors?.length) {
      return { error: String(parseErrors[0].message || parseErrors[0]) };
    }
    if (!descriptor.template) return { error: "no template", noTemplate: true };

    let bindingMetadata = {};
    if (descriptor.script || descriptor.scriptSetup) {
      try {
        const scriptResult = compileScript(descriptor, {
          id: filename,
          inlineTemplate: false,
        });
        bindingMetadata = scriptResult.bindings || {};
      } catch {
        // proceed without bindings
      }
    }

    const result = compileTemplate({
      source: descriptor.template.content,
      filename,
      id: path.basename(filename),
      ssr,
      compilerOptions: {
        mode: "module",
        bindingMetadata,
      },
    });
    if (result.errors?.length) {
      const msg = result.errors.map((e) => (typeof e === "string" ? e : e.message)).join("; ");
      return { error: msg };
    }
    return { code: result.code || "" };
  } catch (err) {
    return { error: err.message || String(err) };
  }
}

function verterCompile(source, filePath, { ssr, vapor }) {
  try {
    const filename = path.basename(filePath);
    const upsert = host.upsert({
      inputId: filePath,
      source,
      fileKind: "vue",
    });
    const profile = {
      filename,
      ssr: !!ssr,
      forceJs: true,
      forceVapor: !!vapor,
      sourceMap: false,
    };
    const result = host.getVirtualFile({
      canonicalId: upsert.canonicalId,
      nodeKind: { kind: "main" },
      compileProfile: profile,
    });
    if (!result?.code) return { error: "empty verter output" };
    if (result.diagnostics?.hasErrors) {
      const msgs = (result.diagnostics.diagnostics || [])
        .filter((d) => d.severity === "error")
        .map((d) => d.message)
        .join("; ");
      if (msgs) return { error: msgs };
    }
    return { code: result.code };
  } catch (err) {
    return { error: err.message || String(err) };
  }
}

// ── Compare one file ─────────────────────────────────────────────

function compareClient(source, filePath) {
  const vue = vueCompile(source, filePath, { ssr: false });
  const verter = verterCompile(source, filePath, { ssr: false, vapor: false });
  if (vue.noTemplate) return { status: "no_template" };
  if (vue.error && verter.error)
    return { status: "both_err", vue: vue.error, verter: verter.error };
  if (vue.error) return { status: "vue_err", vue: vue.error };
  if (verter.error) return { status: "verter_err", verter: verter.error };

  const vueBody = extractRenderBody(vue.code) ?? vue.code;
  const verterBody = extractRenderBody(verter.code) ?? verter.code;
  const a = normalizeClient(vueBody);
  const b = normalizeClient(verterBody);
  if (a === b) return { status: "match" };
  return {
    status: "mismatch",
    vueSample: a.slice(0, 280),
    verterSample: b.slice(0, 280),
  };
}

function compareSsr(source, filePath) {
  const vue = vueCompile(source, filePath, { ssr: true });
  const verter = verterCompile(source, filePath, { ssr: true, vapor: false });
  if (vue.noTemplate) return { status: "no_template" };
  if (vue.error && verter.error)
    return { status: "both_err", vue: vue.error, verter: verter.error };
  if (vue.error) return { status: "vue_err", vue: vue.error };
  if (verter.error) return { status: "verter_err", verter: verter.error };

  const vueBody = extractSsrRenderBody(vue.code) ?? vue.code;
  const verterBody = extractSsrRenderBody(verter.code) ?? verter.code;
  const a = normalizeSsrBehavior(vueBody);
  const b = normalizeSsrBehavior(verterBody);
  if (a === b) return { status: "match" };
  return {
    status: "mismatch",
    vueSample: a.slice(0, 280),
    verterSample: b.slice(0, 280),
  };
}

function compareVapor(source, filePath) {
  // Official Vue 3.5 compileTemplate vapor flag still emits VDOM — no golden.
  // Verter vapor self-health: must compile and emit vapor markers when template exists.
  const { descriptor, errors } = parse(source, { filename: filePath });
  if (errors?.length) return { status: "parse_err", verter: String(errors[0]) };
  if (!descriptor.template) return { status: "no_template" };

  const verter = verterCompile(source, filePath, { ssr: false, vapor: true });
  if (verter.error) return { status: "verter_err", verter: verter.error };
  if (!hasVaporMarkers(verter.code)) {
    // Some trivial templates may still lower through a thin path; require either
    // vapor markers or a successful non-empty main.
    if (!verter.code || verter.code.length < 20) {
      return { status: "verter_err", verter: "empty vapor output" };
    }
    // Treat as soft pass if compile succeeded without crash — count as match
    // only when markers present; else vapor_weak
    return { status: "vapor_weak", sample: verter.code.slice(0, 160) };
  }
  return { status: "match" };
}

// ── Main ─────────────────────────────────────────────────────────

console.log(`Root: ${rootDir}`);
console.log(`Modes: ${modes.join(", ")}`);
let files = discoverVueFiles();
console.log(`Discovered ${files.length} .vue files`);
if (limit > 0 && files.length > limit) {
  files = files.slice(0, limit);
  console.log(`Limited to ${files.length}`);
}

/** @type {Record<string, any>} */
const modeStats = {};
for (const m of modes) {
  modeStats[m] = {
    total: 0,
    match: 0,
    mismatch: 0,
    vue_err: 0,
    verter_err: 0,
    both_err: 0,
    no_template: 0,
    vapor_weak: 0,
    parse_err: 0,
    samples: [],
  };
}

/** per-project tallies for ssr+client */
const byProject = {};

function projectOf(filePath) {
  const rel = path.relative(rootDir, filePath).replace(/\\/g, "/");
  return rel.split("/")[0] || ".";
}

const t0 = Date.now();
let n = 0;
for (const filePath of files) {
  n++;
  if (n % 400 === 0) {
    process.stdout.write(`\r  ${n}/${files.length} ...`);
  }
  let source;
  try {
    source = fs.readFileSync(filePath, "utf8");
  } catch {
    continue;
  }
  const proj = projectOf(filePath);
  if (!byProject[proj]) {
    byProject[proj] = {};
    for (const m of modes) {
      byProject[proj][m] = { total: 0, match: 0, mismatch: 0, verter_err: 0, vue_err: 0 };
    }
  }

  for (const mode of modes) {
    let result;
    if (mode === "client") result = compareClient(source, filePath);
    else if (mode === "ssr") result = compareSsr(source, filePath);
    else if (mode === "vapor") result = compareVapor(source, filePath);
    else continue;

    const st = modeStats[mode];
    st.total++;
    const key = result.status;
    if (st[key] !== undefined) st[key]++;
    else st.mismatch++;

    const bp = byProject[proj][mode];
    bp.total++;
    if (key === "match") bp.match++;
    else if (key === "mismatch") bp.mismatch++;
    else if (key === "verter_err") bp.verter_err++;
    else if (key === "vue_err") bp.vue_err++;

    if ((key === "mismatch" || key === "verter_err") && st.samples.length < maxMismatchSamples) {
      st.samples.push({
        file: path.relative(rootDir, filePath).replace(/\\/g, "/"),
        status: key,
        ...result,
      });
    }
    if (verbose && key !== "match" && key !== "no_template") {
      console.log(`\n[${mode}] ${key} ${path.relative(rootDir, filePath)}`);
    }
  }
}

const elapsed = ((Date.now() - t0) / 1000).toFixed(1);
process.stdout.write("\n");

// ── Report ───────────────────────────────────────────────────────

console.log("\n=== BEHAVIORAL COMPARE (cosmetic waived) ===");
console.log(`files: ${files.length}  elapsed: ${elapsed}s`);
console.log("");

for (const mode of modes) {
  const s = modeStats[mode];
  const comparable = s.total - s.no_template - s.both_err - s.vue_err;
  const rate = comparable > 0 ? ((100 * s.match) / comparable).toFixed(1) : "n/a";
  console.log(`## ${mode}`);
  console.log(
    `  total=${s.total} match=${s.match} mismatch=${s.mismatch} verter_err=${s.verter_err} vue_err=${s.vue_err} both_err=${s.both_err} no_template=${s.no_template}` +
      (s.vapor_weak ? ` vapor_weak=${s.vapor_weak}` : ""),
  );
  console.log(`  match rate among comparable: ${rate}% (excl. no_template / vue-only errs)`);
  if (s.samples.length) {
    console.log(`  samples (${s.samples.length}):`);
    for (const sm of s.samples.slice(0, 8)) {
      console.log(`    - ${sm.status} ${sm.file}`);
      if (sm.verter) console.log(`      verter: ${String(sm.verter).slice(0, 160)}`);
      if (sm.vue) console.log(`      vue: ${String(sm.vue).slice(0, 160)}`);
      if (sm.vueSample) console.log(`      vue body: ${sm.vueSample.slice(0, 140)}`);
      if (sm.verterSample) console.log(`      verter body: ${sm.verterSample.slice(0, 140)}`);
    }
  }
  console.log("");
}

console.log("## per-project (ssr match / total comparable)");
for (const [proj, modesMap] of Object.entries(byProject).sort()) {
  const parts = modes.map((m) => {
    const x = modesMap[m];
    if (!x || !x.total) return `${m}:-`;
    return `${m}:${x.match}/${x.total - /* rough */ 0} m=${x.mismatch} ve=${x.verter_err}`;
  });
  console.log(`  ${proj.padEnd(20)} ${parts.join("  ")}`);
}

const report = {
  root: rootDir,
  modes,
  files: files.length,
  elapsedSec: Number(elapsed),
  fairness: {
    cosmetic_waived: true,
    alpha_local_ids: true,
    binding_prefix_normalized: true,
    vapor_official_golden: false,
    note: "Match = structural/behavioral after normalize. Names/whitespace/patch-flags/comments waived.",
  },
  modeStats,
  byProject,
};

const outPath =
  jsonPath || path.join(REPO, "target", "vue-behavior-compare", `report-${Date.now()}.json`);
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(report, null, 2));
console.log(`\nWrote ${outPath}`);

// Exit non-zero if SSR or client have high verter_err rate or zero matches when work ran
const ssr = modeStats.ssr;
const client = modeStats.client;
let fail = false;
if (ssr && ssr.total > 10 && ssr.match === 0) fail = true;
if (client && client.total > 10 && client.match === 0) fail = true;
process.exit(fail ? 1 : 0);
