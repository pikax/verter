#!/usr/bin/env node
// Executes the public Vue- and Svelte-pinned Vite and Rollup entries, including
// any virtual-script load that carries the compiled product, and prints the
// result as JSON for a Rust-side comparison against the in-process host route.
//
// The plugin is loaded from its BUILT entry (`dist/index.mjs`). A committed
// source/dist fingerprint makes that prerequisite fail closed when the ignored
// dist is absent or was not built from the current production sources.
//
// Usage: node scripts/probe-bundler-route.mjs
// Exit codes: 0 = probed; 2 = the built plugin is missing, stale, or unloadable.

import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.join(here, "..");
const sourceRoot = path.join(packageRoot, "src");
const distRoot = path.join(packageRoot, "dist");
const entry = path.join(distRoot, "index.mjs");
const freshnessRecordPath = path.join(here, "probe-bundler-route.freshness.json");

async function filesUnder(root) {
  const files = [];
  async function visit(directory) {
    for (const item of await readdir(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, item.name);
      if (item.isDirectory()) await visit(absolute);
      else if (item.isFile()) files.push(absolute);
    }
  }
  await visit(root);
  return files;
}

async function treeHash(root, files) {
  const digest = createHash("sha256");
  for (const absolute of files.sort()) {
    const relative = path.relative(root, absolute).split(path.sep).join("/");
    digest.update(relative);
    digest.update("\0");
    digest.update(await readFile(absolute));
    digest.update("\0");
  }
  return digest.digest("hex");
}

async function currentFreshness() {
  const sourceFiles = (await filesUnder(sourceRoot)).filter((absolute) => {
    const relative = path.relative(sourceRoot, absolute).split(path.sep).join("/");
    return (
      !relative.includes("/__fixtures__/") &&
      !relative.startsWith("__fixtures__/") &&
      !relative.includes("/__tests__/") &&
      !relative.startsWith("__tests__/") &&
      !relative.endsWith(".spec.ts")
    );
  });
  const buildInputs = [
    ...sourceFiles,
    path.join(packageRoot, "package.json"),
    path.join(packageRoot, "tsconfig.json"),
    path.join(packageRoot, "tsdown.config.mts"),
  ];
  const distFiles = await filesUnder(distRoot);
  return {
    sourceSha256: await treeHash(packageRoot, buildInputs),
    distSha256: await treeHash(distRoot, distFiles),
  };
}

let freshness;
let module_;
try {
  const expected = JSON.parse(await readFile(freshnessRecordPath, "utf8"));
  freshness = await currentFreshness();
  if (
    freshness.sourceSha256 !== expected.sourceSha256 ||
    freshness.distSha256 !== expected.distSha256
  ) {
    throw new Error(
      `built plugin freshness mismatch (expected source ${expected.sourceSha256} / dist ${expected.distSha256}, ` +
        `observed source ${freshness.sourceSha256} / dist ${freshness.distSha256})`,
    );
  }
  module_ = await import(pathToFileURL(entry).href);
} catch (error) {
  process.stdout.write(
    JSON.stringify({ loaded: false, fresh: false, error: String(error?.message ?? error) }),
  );
  process.exit(2);
}

const SUPPORTED_SVELTE =
  '<script>\n  let count = $state(0);\n</script>\n\n<div class="root">{count}</div>\n\n<style>\n  .root { color: red; }\n</style>\n';
const VUE_SFC =
  "<script setup>\nconst props = defineProps({ label: { type: String, required: true } });\n</script>\n\n<template>\n  <button>{{ label }}</button>\n</template>\n";

/** The minimal bundler context the transform and load hooks consult. */
function bundlerContext(errors) {
  return {
    error(problem) {
      errors.push(String(problem?.message ?? problem));
      throw new Error(String(problem?.message ?? problem));
    },
    warn() {},
    addWatchFile() {},
    emitFile() {},
    async resolve() {
      return null;
    },
  };
}

function hook(plugin, name) {
  const candidate = plugin[name];
  const callable = typeof candidate === "function" ? candidate : candidate?.handler;
  if (typeof callable !== "function") throw new TypeError(`the public plugin has no ${name} hook`);
  return callable;
}

const results = {
  loaded: true,
  fresh: true,
  freshness,
  exports: Object.keys(module_).sort(),
  // The `default` spelling is an ALIAS, and the identity is MEASURED here
  // rather than asserted from source: without this the export could only be
  // classified out of scope on a claim.
  defaultIsVerterVue: module_.default === module_.VerterVue,
  cases: {},
};

/** The resolved-config shape the Vite-only hooks read. */
function viteConfigStub() {
  return { root: process.cwd(), command: "serve", build: { ssr: false } };
}

/**
 * Drive one already-constructed Vite-shaped plugin over one carrier: the
 * include decision for both carrier extensions, the transform, and the
 * virtual-script request the published wrapper points at.
 */
async function driveViteEntry({
  publicFactory,
  plugin,
  id,
  otherId,
  source,
  queryMarker,
  context,
}) {
  const include = hook(plugin, "transformInclude").call(context, id);
  const oppositeInclude = hook(plugin, "transformInclude").call(context, otherId);
  const wrapper = await hook(plugin, "transform").call(context, source, id);
  const quotedRequests =
    typeof wrapper?.code === "string"
      ? [...wrapper.code.matchAll(/["']([^"']+\?(?:vue|verter)&type=script[^"']*)["']/g)]
      : [];
  const scriptRequest = quotedRequests
    .map((match) => match[1])
    .find((request) => request.startsWith(`${id}?${queryMarker}&type=script`));
  if (!scriptRequest) {
    throw new Error(`the transformed wrapper published no ${queryMarker} script request`);
  }
  const resolvedScriptId = await hook(plugin, "resolveId").call(context, scriptRequest);
  const loadedScript = await hook(plugin, "load").call(context, resolvedScriptId ?? scriptRequest);

  return {
    outcome: "transformed",
    publicFactory,
    id,
    transformInclude: include,
    oppositeId: otherId,
    oppositeTransformInclude: oppositeInclude,
    // The `*HasMap` booleans are DIAGNOSTICS. They are this script's opinion of
    // the artifact, so an acceptance assertion resting on one asserts the
    // probe rather than the product; the raw `*Map` values beside them are what
    // the Rust side validates.
    wrapperHasMap: wrapper?.map !== null && wrapper?.map !== undefined,
    wrapperMap: wrapper?.map ?? null,
    scriptRequest,
    resolvedScriptId: resolvedScriptId ?? null,
    loadedScriptOutcome:
      loadedScript === null || loadedScript === undefined ? "missing" : "published",
    loadedScriptCode: loadedScript?.code ?? null,
    loadedScriptHasMap: loadedScript?.map !== null && loadedScript?.map !== undefined,
    loadedScriptMap: loadedScript?.map ?? null,
  };
}

for (const { label, publicFactory, entryObject, id, otherId, source, queryMarker } of [
  {
    label: "vuePublicEntry",
    publicFactory: "VerterVue.vite",
    entryObject: module_.VerterVue,
    id: "/probe/Plug.vue",
    otherId: "/probe/Plug.svelte",
    source: VUE_SFC,
    queryMarker: "vue",
  },
  {
    label: "sveltePublicEntry",
    publicFactory: "VerterSvelte.vite",
    entryObject: module_.VerterSvelte,
    id: "/probe/Plug.svelte",
    otherId: "/probe/Plug.vue",
    source: SUPPORTED_SVELTE,
    queryMarker: "verter",
  },
]) {
  const errors = [];
  const context = bundlerContext(errors);
  let plugin;
  try {
    if (typeof entryObject?.vite !== "function") {
      throw new TypeError(`the built plugin exports no public ${publicFactory} factory`);
    }
    plugin = entryObject.vite({});
    if (typeof plugin.configResolved === "function") {
      await plugin.configResolved(viteConfigStub());
    }

    results.cases[label] = await driveViteEntry({
      publicFactory,
      plugin,
      id,
      otherId,
      source,
      queryMarker,
      context,
    });
  } catch (error) {
    results.cases[label] = {
      outcome: "error",
      publicFactory,
      id,
      message: String(error?.message ?? error),
      errors,
    };
  } finally {
    if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call(context);
  }
}

for (const { label, publicFactory, entryObject, id, source, queryMarker } of [
  {
    label: "vueRollupEntry",
    publicFactory: "VerterVue.rollup",
    entryObject: module_.VerterVue,
    id: "/probe/Plug.vue",
    source: VUE_SFC,
    queryMarker: "vue",
  },
  {
    label: "svelteRollupEntry",
    publicFactory: "VerterSvelte.rollup",
    entryObject: module_.VerterSvelte,
    id: "/probe/Plug.svelte",
    source: SUPPORTED_SVELTE,
    queryMarker: "verter",
  },
]) {
  const errors = [];
  const context = bundlerContext(errors);
  let plugin;
  try {
    if (typeof entryObject?.rollup !== "function") {
      throw new TypeError(`the built plugin exports no public ${publicFactory} factory`);
    }
    plugin = entryObject.rollup({});

    const include = hook(plugin, "transformInclude").call(context, id);
    const transformed = await hook(plugin, "transform").call(context, source, id);
    const quotedRequests =
      typeof transformed?.code === "string"
        ? [...transformed.code.matchAll(/["']([^"']+\?(?:vue|verter)&type=script[^"']*)["']/g)]
        : [];
    const scriptRequest = quotedRequests
      .map((match) => match[1])
      .find((request) => request.startsWith(`${id}?${queryMarker}&type=script`));
    const loadedScript = scriptRequest
      ? await hook(plugin, "load").call(
          context,
          (await hook(plugin, "resolveId").call(context, scriptRequest)) ?? scriptRequest,
        )
      : null;

    results.cases[label] = {
      outcome: "transformed",
      publicFactory,
      id,
      transformInclude: include,
      publicTransformIsInline:
        typeof transformed?.code === "string" && transformed.code.length > 0 && !scriptRequest,
      publicTransformHasMap: transformed?.map !== null && transformed?.map !== undefined,
      publicTransformMap: transformed?.map ?? null,
      scriptRequest: scriptRequest ?? null,
      loadedScriptOutcome: scriptRequest
        ? loadedScript === null || loadedScript === undefined
          ? "missing"
          : "published"
        : "not-applicable",
      loadedScriptHasMap: scriptRequest
        ? loadedScript?.map !== null && loadedScript?.map !== undefined
        : null,
      loadedScriptMap: scriptRequest ? (loadedScript?.map ?? null) : null,
    };
  } catch (error) {
    results.cases[label] = {
      outcome: "error",
      publicFactory,
      id,
      message: String(error?.message ?? error),
      errors,
    };
  } finally {
    if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call(context);
  }
}

// ── EVERY enumerated export, driven uniformly ──────────────────────────────
//
// The case list below IS the export list: one generic driver, invoked once per
// name in `results.exports`, reading the value back out of `module_` itself.
// There is deliberately NO per-export body. A case therefore cannot be cloned
// from a sibling's — the only way a record exists is that this function read
// that name and called what it returned — and each record carries the
// observations that tell the spellings apart: what KIND of thing the export is,
// and which carriers it accepts.

/** The two carriers every drivable export is offered. */
const CARRIERS = [
  { key: "vue", id: "/probe/Plug.vue", source: VUE_SFC, queryMarker: "vue" },
  { key: "svelte", id: "/probe/Plug.svelte", source: SUPPORTED_SVELTE, queryMarker: "verter" },
];

/** Drive one carrier through an already-flattened Vite-shaped plugin. */
async function driveCarrier(plugin, carrier, context) {
  const include = hook(plugin, "transformInclude").call(context, carrier.id);
  if (include !== true) return { transformInclude: include === true };

  const wrapper = await hook(plugin, "transform").call(context, carrier.source, carrier.id);
  const quoted =
    typeof wrapper?.code === "string"
      ? [...wrapper.code.matchAll(/["']([^"']+\?(?:vue|verter)&type=script[^"']*)["']/g)]
      : [];
  const scriptRequest = quoted
    .map((match) => match[1])
    .find((request) => request.startsWith(`${carrier.id}?${carrier.queryMarker}&type=script`));
  if (!scriptRequest) {
    return {
      transformInclude: true,
      scriptRequest: null,
      transformedCode: wrapper?.code ?? null,
      loadedScriptOutcome: "not-applicable",
      loadedScriptCode: null,
    };
  }
  const resolved = await hook(plugin, "resolveId").call(context, scriptRequest);
  const loaded = await hook(plugin, "load").call(context, resolved ?? scriptRequest);
  return {
    transformInclude: true,
    scriptRequest,
    transformedCode: wrapper?.code ?? null,
    loadedScriptOutcome: loaded === null || loaded === undefined ? "missing" : "published",
    loadedScriptCode: loaded?.code ?? null,
    loadedScriptMap: loaded?.map ?? null,
  };
}

/**
 * What can be READ off an export's value, with no interpretation applied.
 *
 * This script records evidence, NOT a classification: a `kind` string here
 * would be this script's opinion, and a case copied from a sibling carries
 * whatever opinion was copied with it. The consumer derives the classification
 * from these readings instead, and cross-checks them against what DRIVING the
 * same value returned (`pluginKeys` below), so a copied case has to contradict
 * itself.
 *
 * The trust floor, stated rather than papered over: none of this proves this
 * script READ HONESTLY. A probe can print any `typeof` it likes. Cross-checking
 * two recorded observations catches a case copied from a sibling, because the
 * copy carries the sibling's drive result; it cannot catch a probe that forges
 * both. Closing that needs the observation moved in-process — it is the floor
 * of every out-of-process probe, not something a further assertion can reach.
 */
function readEvidence(value) {
  const valueType = typeof value;
  if (valueType === "function") {
    return { valueType, functionLength: value.length, functionName: value.name };
  }
  if (value !== null && valueType === "object") {
    const ownKeys = Object.keys(value).sort();
    return { valueType, ownKeys, ownKeyTypes: ownKeys.map((key) => typeof value[key]) };
  }
  return { valueType };
}

/**
 * Drive ONE enumerated export by name.
 *
 * How the value is invoked follows from the value itself: an unplugin object
 * exposes a `.vite` factory, while a raw unplugin factory is a bare function a
 * consumer calls with its own bundler meta (and whose Vite-only hooks stay
 * nested under a `vite` sub-object, since `createUnplugin` is what flattens
 * them). Neither branch writes that decision into the record — the record
 * carries the evidence it was made from, plus what the invocation returned.
 */
async function driveExport(exportName, enumerated) {
  const value = module_[exportName];
  const evidence = readEvidence(value);
  // An ALIAS is recorded by object identity against an export already
  // enumerated — a measurement, not a claim. The consumer additionally
  // requires the two spellings' evidence to agree, which identity implies.
  const aliasOf = enumerated
    .slice(0, enumerated.indexOf(exportName))
    .find((earlier) => module_[earlier] === value);
  if (aliasOf !== undefined) {
    return { exportName, evidence, aliasOf };
  }

  const errors = [];
  const context = bundlerContext(errors);
  let plugin = null;
  try {
    if (value !== null && typeof value === "object" && typeof value.vite === "function") {
      plugin = value.vite({});
      if (typeof plugin.configResolved === "function") {
        await plugin.configResolved(viteConfigStub());
      }
    } else if (typeof value === "function") {
      plugin = value({}, { framework: "vite" });
      if (typeof plugin?.vite?.configResolved === "function") {
        await plugin.vite.configResolved.call(context, viteConfigStub());
      }
    } else {
      // Not drivable. No reason string is recorded: the evidence above is what
      // the consumer states the reason from.
      return { exportName, evidence };
    }

    // What the INVOCATION returned, read off the plugin object itself. This
    // is a second, independent observation of the same value: `createUnplugin`
    // flattens an adapter's Vite-only hooks onto the plugin it returns, so a
    // wrapped entry's plugin and a raw factory's differ here even though both
    // are driven through the same hooks below.
    const pluginKeys = Object.keys(plugin).sort();

    const carriers = {};
    for (const carrier of CARRIERS) {
      carriers[carrier.key] = await driveCarrier(plugin, carrier, context);
    }
    return { exportName, evidence, pluginKeys, carriers };
  } catch (error) {
    return {
      exportName,
      evidence,
      outcome: "error",
      message: String(error?.message ?? error),
      errors,
    };
  } finally {
    if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call(context);
  }
}

results.exportCases = {};
for (const exportName of results.exports) {
  results.exportCases[exportName] = await driveExport(exportName, results.exports);
}

process.stdout.write(JSON.stringify(results));
