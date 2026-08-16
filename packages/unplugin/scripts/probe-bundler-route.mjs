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
  cases: {},
};

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
      await plugin.configResolved({
        root: process.cwd(),
        command: "serve",
        build: { ssr: false },
      });
    }

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
    const loadedScript = await hook(plugin, "load").call(
      context,
      resolvedScriptId ?? scriptRequest,
    );

    results.cases[label] = {
      outcome: "transformed",
      publicFactory,
      id,
      transformInclude: include,
      oppositeId: otherId,
      oppositeTransformInclude: oppositeInclude,
      wrapperHasMap: wrapper?.map !== null && wrapper?.map !== undefined,
      scriptRequest,
      resolvedScriptId: resolvedScriptId ?? null,
      loadedScriptOutcome:
        loadedScript === null || loadedScript === undefined ? "missing" : "published",
      loadedScriptCode: loadedScript?.code ?? null,
      loadedScriptHasMap: loadedScript?.map !== null && loadedScript?.map !== undefined,
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

process.stdout.write(JSON.stringify(results));
