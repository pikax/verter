/**
 * Capture — and verify — the WASM-produced carrier fixtures the in-context
 * LanguageService guards consume.
 *
 * Runs the REAL WASM host (packages/wasm/wasm — the same binary the playground
 * ships) over a small set of committed carrier sources and snapshots the three
 * generated surfaces per source:
 *
 *   - IDE carrier      — `compileRequest(id, { vue|svelte: {...} })`
 *                        an `ideCompanion` product                 → `Comp.vue.tsx`
 *   - declaration      — `getPublicApi(id, "declaration")` → `Comp.d.vue.ts`
 *   - API carrier      — `getPublicApi(id)`                → `Comp.vue.verter.ts`
 *
 * The snapshot is committed at `src/editor/__fixtures__/wasm-carriers.json` so
 * the guards stay hermetic (no live WASM host load per test). A hermetic
 * snapshot of live output only stays truthful while something compares the two,
 * so ONE renderer serves both modes:
 *
 *   node scripts/capture-wasm-carrier-fixtures.mjs           # render + write
 *   node scripts/capture-wasm-carrier-fixtures.mjs --check   # render + compare
 *
 * `--check` writes nothing and exits non-zero on any difference, so a
 * compiler-output change that is not accompanied by a regenerated fixture
 * fails instead of leaving every consuming guard green against stale bytes.
 * It renders from the artifact present on disk; whoever invokes it is
 * responsible for that artifact being current (CI runs it in the same job that
 * just built it). A missing artifact is a hard failure, never a skip — a check
 * that can pass without reaching the host proves nothing.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { resolve, dirname, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const thisDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(thisDir, "../../..");
const wasmJs = resolve(thisDir, "../../wasm/wasm/verter_wasm.js");
const wasmBin = resolve(thisDir, "../../wasm/wasm/verter_wasm_bg.wasm");
const outFile = resolve(thisDir, "../src/editor/__fixtures__/wasm-carriers.json");

const USAGE = `usage: node scripts/capture-wasm-carrier-fixtures.mjs [--check]

  (no flag)  render from the built WASM artifact and write the fixture
  --check    render from the built WASM artifact and compare it against the
             committed fixture; writes nothing, exits 1 on any difference`;

const argv = process.argv.slice(2);
const unknownArgs = argv.filter((arg) => arg !== "--check");
if (unknownArgs.length > 0) {
  console.error(`unknown argument(s): ${unknownArgs.join(" ")}\n\n${USAGE}`);
  process.exit(2);
}
const checkMode = argv.includes("--check");

const rel = (p) => relative(repoRoot, p);

/** The committed fixture sources. Key = fixture id in the JSON. */
const FIXTURES = [
  {
    key: "compVue",
    filename: "Comp.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
defineProps<{ count: number }>()
</script>
<template><div>{{ count }}</div></template>`,
  },
  {
    key: "compVueEdited",
    filename: "Comp.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
defineProps<{ count: string, label: string }>()
</script>
<template><div>{{ count }}{{ label }}</div></template>`,
  },
  {
    key: "astralVue",
    filename: "Astral.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
const msg = "ok"
</script>
<template><div>🎉🎉{{ msg }}</div></template>`,
  },
  {
    // The cross-host semantic-parity probe: a runtime-object `defineExpose`
    // whose exposed member's type is INFERRED from the setup binding. The API
    // surface renders `count: typeof count` (the setup body is emitted, so the
    // inferred `number` survives); the declaration surface can only render
    // `count: unknown` (the body is omitted, so the binding is not in scope).
    // Every host that can choose its import target must therefore choose the
    // API surface, or the same code types differently per host.
    key: "exposeVue",
    filename: "Expose.vue",
    fileKind: "vue",
    source: `<script setup lang="ts">
const count = 41 + 1
defineExpose({ count })
</script>
<template><div>{{ count }}</div></template>`,
  },
  {
    key: "compSvelte",
    filename: "Comp.svelte",
    fileKind: "svelte",
    source: `<script lang="ts">
  let { count }: { count: number } = $props();
</script>
<div>{count}</div>`,
  },
];

const FIXTURE_COMMENT =
  "Committed snapshot of REAL WASM-host carrier output (getIde / getPublicApi). " +
  "Captured by scripts/capture-wasm-carrier-fixtures.mjs so guards run hermetically. " +
  "Regenerate with: node scripts/capture-wasm-carrier-fixtures.mjs — " +
  "verify against the built artifact with: node scripts/capture-wasm-carrier-fixtures.mjs --check";

function requireBuiltArtifact() {
  const missing = [wasmJs, wasmBin].filter((path) => !existsSync(path));
  if (missing.length === 0) return;
  console.error(
    `WASM artifact missing — cannot ${checkMode ? "verify" : "capture"} carrier fixtures:\n` +
      `${missing.map((path) => `  ${rel(path)}`).join("\n")}\n` +
      `Build it first: pnpm --filter @verter/wasm build\n` +
      `That is the publication lane (wasm-bindgen + wasm-opt) and the one CI ` +
      `compares against. The root \`pnpm run build:wasm\` is the developer ` +
      `lane (bindgen only, no wasm-opt); nothing here proves the two lanes ` +
      `render identical carrier bytes, so a fixture captured from one and ` +
      `checked against the other can report a difference that is only the ` +
      `optimizer's.`,
  );
  process.exit(1);
}

function surface(response) {
  if (!response || typeof response.code !== "string") return null;
  return {
    code: response.code,
    sourceMap: typeof response.sourceMap === "string" ? response.sourceMap : null,
    destructuredBlock: response.destructuredBlock ?? null,
  };
}

function publicApiSurface(result) {
  if (result.error) {
    throw Object.assign(
      new Error(`public API projection failed: ${result.error.code}/${result.error.detailCode}`),
      result.error,
    );
  }
  return surface(result.value);
}

/**
 * The single requested product: the IDE surface, with source maps on and
 * every axis the legacy `{ sourceMap: true, target: "ide", forceJs: true }`
 * profile implied left at its (false) default. The typed host-request
 * route's equivalence tests pin these axis values verbatim, but against a
 * `{ target: "full", sourceMap: true, isProduction: false }` legacy demand
 * with `forceJs` at its identity — this tool's `forceJs: true` +
 * `target: "ide"` pairing is covered only indirectly, through the shared
 * IDE response projection and this script's own fixture freshness rail.
 */
function ideRequest(fileKind) {
  const identity = { isProduction: false, forceJs: true };
  const products = [
    {
      ideCompanion: {
        wantSourceMap: true,
        embedAmbientTypes: false,
        conditionalRootNarrowing: false,
        strictSlots: false,
        ideChunkBoundaries: false,
      },
    },
  ];
  if (fileKind === "svelte") return { svelte: { identity, products, options: {} } };
  return {
    vue: {
      identity,
      products,
      options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
    },
  };
}

/**
 * The IDE row of a compile response, taken by kind tag — never by position.
 * The response carries one row per requested product, tagged and in request
 * order, so a capture that requested exactly one IDE product must find
 * exactly one `ideCompanion` row. Anything else is a wire break inside this
 * tool, so it fails the capture loudly instead of snapshotting the
 * silent-null `ide: null` shape the fixture must never carry.
 */
function ideProductRow(response) {
  const products = Array.isArray(response?.products) ? response.products : [];
  const kinds = products.map((product) => product?.kind ?? "<untagged>");
  if (products.length !== 1 || kinds[0] !== "ideCompanion") {
    throw new Error(`expected exactly one ideCompanion product row, got: [${kinds.join(", ")}]`);
  }
  return products[0];
}

function capture(wasmModule, { filename, fileKind, source }) {
  // A fresh host per fixture keeps every entry independent of capture order.
  const host = new wasmModule.VerterHost({
    devMode: true,
    compileErrorPolicy: "devServeLastKnownGood",
    maxProfilesPerFile: 8,
  });
  // Registration carries source only, no compile demand — the compile demand
  // is stated on the typed request below, by canonical id, never by copying
  // the source into it.
  host.upsert({ inputId: filename, source, fileKind, aliases: [] });

  let ide = null;
  let ideUnavailable = null;
  let response;
  try {
    response = host.compileRequest(filename, ideRequest(fileKind));
  } catch (err) {
    // An IDE compile failure is recorded explicitly (`ideUnavailable`) so the
    // fixture stays honest about what the host produced — a guard asserting
    // IDE parity fails on the recorded gap instead of a silent null.
    ideUnavailable = String(err?.message ?? err).split("\n")[0];
  }
  // Row selection runs OUTSIDE the failure catch above: a malformed (or
  // missing) product list is this tool's wire break, not a host compile
  // failure, and must fail the capture rather than ride into the fixture
  // as a silent `ide: null`.
  if (ideUnavailable === null) ide = surface(ideProductRow(response));
  const decl = publicApiSurface(host.getPublicApi(filename, "declaration"));
  const api = publicApiSurface(host.getPublicApi(filename));
  // No explicit `host.free()`: if an IDE compile trapped, the host borrow is
  // poisoned and `free` would throw; the short-lived capture process reclaims
  // everything on exit.
  return { filename, source, ide, ideUnavailable, decl, api };
}

/**
 * The single rendering path. Both writing and comparison call this, so the two
 * modes can never disagree about formatting, key order, or what a surface is.
 * Insertion order is fixed by `FIXTURES` and the literal field order above, so
 * repeated renders over the same artifact produce identical bytes.
 */
async function render() {
  const wasmModule = await import(pathToFileURL(wasmJs).href);
  await wasmModule.default({ module_or_path: readFileSync(wasmBin) });
  const snapshot = { $comment: FIXTURE_COMMENT };
  for (const fixture of FIXTURES) {
    snapshot[fixture.key] = capture(wasmModule, fixture);
  }
  return { snapshot, text: `${JSON.stringify(snapshot, null, 2)}\n` };
}

function summarize(key, entry) {
  const ide = entry.ide
    ? `${entry.ide.code.length} chars`
    : `UNAVAILABLE (${entry.ideUnavailable})`;
  return (
    `${key}: ide=${ide}` +
    ` decl=${entry.decl ? `${entry.decl.code.length} chars` : "MISSING"}` +
    ` api=${entry.api ? `${entry.api.code.length} chars` : "MISSING"}`
  );
}

const ABSENT = Symbol("absent");
const MAX_REPORTED_DIFFS = 20;

function collectDiffs(committed, rendered, path, out) {
  if (out.length >= MAX_REPORTED_DIFFS) return;
  if (committed === rendered) return;
  const comparable =
    committed !== null &&
    rendered !== null &&
    typeof committed === "object" &&
    typeof rendered === "object" &&
    Array.isArray(committed) === Array.isArray(rendered);
  if (!comparable) {
    out.push({ path, committed, rendered });
    return;
  }
  const keys = [...new Set([...Object.keys(committed), ...Object.keys(rendered)])];
  for (const key of keys) {
    if (out.length >= MAX_REPORTED_DIFFS) return;
    const childPath = path ? `${path}.${key}` : key;
    const left = key in committed ? committed[key] : ABSENT;
    const right = key in rendered ? rendered[key] : ABSENT;
    if (left === ABSENT || right === ABSENT) {
      out.push({ path: childPath, committed: left, rendered: right });
      continue;
    }
    collectDiffs(left, right, childPath, out);
  }
}

function describeValue(value) {
  if (value === ABSENT) return "<key absent>";
  if (typeof value === "string") return `string (${value.length} chars)`;
  return JSON.stringify(value) ?? String(value);
}

function describeDiff({ path, committed, rendered }) {
  const head = `  ${path}: committed ${describeValue(committed)} vs rendered ${describeValue(rendered)}`;
  if (typeof committed !== "string" || typeof rendered !== "string") return head;
  let i = 0;
  while (i < committed.length && i < rendered.length && committed[i] === rendered[i]) i += 1;
  const window = (text) => JSON.stringify(text.slice(Math.max(0, i - 40), i + 60));
  return (
    `${head}\n` +
    `    first difference at index ${i}\n` +
    `      committed: ${window(committed)}\n` +
    `      rendered:  ${window(rendered)}`
  );
}

async function main() {
  requireBuiltArtifact();

  const { snapshot, text } = await render();
  for (const fixture of FIXTURES) {
    console.log(summarize(fixture.key, snapshot[fixture.key]));
  }

  if (!checkMode) {
    mkdirSync(dirname(outFile), { recursive: true });
    writeFileSync(outFile, text);
    console.log(`wrote ${rel(outFile)}`);
    return;
  }

  if (!existsSync(outFile)) {
    console.error(
      `committed fixture missing: ${rel(outFile)}\n` +
        `Regenerate it: pnpm --filter @verter/playground run capture:wasm-fixtures`,
    );
    process.exit(1);
  }

  const committedText = readFileSync(outFile, "utf8");
  if (committedText === text) {
    console.log(
      `OK: ${rel(outFile)} is byte-identical to what the built WASM artifact renders ` +
        `(${text.length} chars).`,
    );
    return;
  }

  let committedSnapshot;
  try {
    committedSnapshot = JSON.parse(committedText);
  } catch (err) {
    console.error(
      `committed fixture ${rel(outFile)} is not valid JSON (${err.message}); ` +
        `it differs from the rendered snapshot.`,
    );
    process.exit(1);
  }

  const diffs = [];
  collectDiffs(committedSnapshot, snapshot, "", diffs);
  console.error(
    `STALE FIXTURE: ${rel(outFile)} does not match what the built WASM artifact renders.\n` +
      `  committed: ${committedText.length} chars\n` +
      `  rendered:  ${text.length} chars`,
  );
  if (diffs.length === 0) {
    // Same parsed value, different bytes: formatting or key order drifted.
    console.error(
      "  the parsed snapshots are equal, so the committed bytes differ only in " +
        "formatting or key order — they were not produced by this renderer.",
    );
  } else {
    // The walk stops AT the cap, so a run that collected exactly the cap may or
    // may not have more — never report that count as exact.
    console.error(
      `differences (${diffs.length >= MAX_REPORTED_DIFFS ? `first ${MAX_REPORTED_DIFFS}, possibly more` : diffs.length}):`,
    );
    for (const diff of diffs) console.error(describeDiff(diff));
  }
  console.error(
    `\nRegenerate from the built artifact and commit the result:\n` +
      `  pnpm --filter @verter/playground run capture:wasm-fixtures`,
  );
  process.exit(1);
}

await main();
