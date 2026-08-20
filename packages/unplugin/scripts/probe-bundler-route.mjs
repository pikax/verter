#!/usr/bin/env node
// Drive the public Vue/Svelte Vite and Rollup entries (including any
// virtual-script load that carries the compiled product) and print JSON
// for Rust-side comparison against the in-process host.
//
// Loads the built entry (`dist/index.mjs`). A committed source/dist
// fingerprint fails closed when dist is missing or stale.
//
// Exit: 0 every lane observed, 1 record printed but a lane errored
// (`erroredCases`), 2 plugin missing/stale/unloadable.

import { createHash } from "node:crypto";
import { readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  allocateRecompileFixture,
  collectErroredCaseLabels,
  probeExitCode,
} from "./probe-bundler-route-isolation.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.join(here, "..");
const repoRoot = path.join(packageRoot, "..", "..");
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
/**
 * The CSS handed to the non-Vite style transform.
 *
 * It carries a `v-bind()` payload on purpose: an UNSCOPED request returns its
 * input unchanged, so an unscoped product is indistinguishable from "the lane
 * never ran". The `v-bind()` rewrite names the componentId of the profile the
 * lane read out of its cache, which is what ties the product to THIS carrier.
 */
const NON_VITE_STYLE_SOURCE = ".box { color: v-bind(primary); }\n";
/**
 * Where on-disk fixtures are allocated.
 *
 * It is inside the repository on purpose — the plugin resolves
 * `vue/compiler-sfc` through `createRequire(join(root, "package.json"))`, so a
 * fixture in the OS temp directory would not reach the workspace's copy. The
 * directory itself is stable and ignored; each invocation gets its own
 * `mkdtemp` child under it and removes only that child.
 */
const FIXTURE_PARENT = path.join(repoRoot, ".verter-probe-fixtures");
/** The Parent/Child pair the pre-compile + cross-file lane is driven over. */
const RECOMPILE_CHILD_VUE =
  "<script setup>\ndefineProps({ msg: String })\n</script>\n\n<template><div>{{ msg }}</div></template>\n";
const RECOMPILE_PARENT_VUE =
  '<script setup>\nimport Child from "./Child.vue"\n</script>\n\n<template><Child msg="hello" /></template>\n';

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
 * The style sub-requests a transformed wrapper published, in the order the
 * wrapper listed them.
 *
 * Both import spellings the wrapper emits are matched: a bare side-effect
 * import for a plain block, and a default import for a CSS-module block.
 */
const STYLE_REQUEST_PATTERN =
  /import(?:\s+[A-Za-z_$][\w$]*\s+from)?\s+["']([^"']+\?(?:vue|verter)&type=style[^"']*)["']/g;

function styleRequestsIn(code) {
  if (typeof code !== "string") return [];
  return [...code.matchAll(STYLE_REQUEST_PATTERN)].map((match) => match[1]);
}

/** The `index` / `lang` the WRAPPER wrote into a style request. */
function styleRequestFacts(request) {
  const [, queryString = ""] = request.split("?", 2);
  const params = new URLSearchParams(queryString);
  let lang = params.get("lang") ?? null;
  if (!lang) {
    for (const key of params.keys()) {
      if (key.startsWith("lang.")) {
        lang = key.slice(5);
        break;
      }
    }
  }
  return {
    index: params.has("index") ? Number.parseInt(params.get("index"), 10) : null,
    lang,
  };
}

/** Resolve then load one virtual request, recording the published artifact. */
async function loadRequest(plugin, request, context) {
  const resolved = await hook(plugin, "resolveId").call(context, request);
  const loaded = await hook(plugin, "load").call(context, resolved ?? request);
  return {
    request,
    resolvedId: resolved ?? null,
    outcome: loaded === null || loaded === undefined ? "missing" : "published",
    code: loaded?.code ?? null,
    // A DIAGNOSTIC beside the raw artifact, like `wrapperHasMap` above; the
    // Rust side validates `map` itself.
    hasMap: loaded?.map !== null && loaded?.map !== undefined,
    map: loaded?.map ?? null,
  };
}

/** Load every style sub-request a wrapper published. */
async function loadStyleRequests(plugin, requests, context) {
  const loaded = [];
  for (const request of requests) {
    const facts = styleRequestFacts(request);
    loaded.push({ ...(await loadRequest(plugin, request, context)), ...facts });
  }
  return loaded;
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
  templateRequest,
  unregisteredTemplateRequest,
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

  // The STYLE sub-requests the same wrapper published, each loaded. A carrier
  // with no `<style>` publishes NONE, which is what makes the count a
  // measurement rather than a formality.
  const styleRequests = styleRequestsIn(wrapper?.code);
  const loadedStyles = await loadStyleRequests(plugin, styleRequests, context);

  // A virtual request the wrapper does NOT point at, served straight from the
  // host rather than from a transform-populated cache, plus a negative control
  // for a carrier this plugin never transformed.
  const loadedTemplate = templateRequest
    ? await loadRequest(plugin, templateRequest, context)
    : null;
  const unregisteredTemplate = unregisteredTemplateRequest
    ? await loadRequest(plugin, unregisteredTemplateRequest, context)
    : null;

  return {
    styleRequests,
    loadedStyles,
    loadedTemplate,
    unregisteredTemplate,
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

for (const {
  label,
  publicFactory,
  entryObject,
  id,
  otherId,
  source,
  queryMarker,
  templateRequest,
  unregisteredTemplateRequest,
} of [
  {
    label: "vuePublicEntry",
    publicFactory: "VerterVue.vite",
    entryObject: module_.VerterVue,
    id: "/probe/Plug.vue",
    otherId: "/probe/Plug.svelte",
    source: VUE_SFC,
    queryMarker: "vue",
    // Never cached by the transform, so this request falls through to the
    // host-backed branch of `load` rather than to a transform-populated map.
    templateRequest: "/probe/Plug.vue?vue&type=template",
    unregisteredTemplateRequest: "/probe/NotRegistered.vue?vue&type=template",
  },
  {
    label: "sveltePublicEntry",
    publicFactory: "VerterSvelte.vite",
    entryObject: module_.VerterSvelte,
    id: "/probe/Plug.svelte",
    otherId: "/probe/Plug.vue",
    source: SUPPORTED_SVELTE,
    queryMarker: "verter",
    templateRequest: null,
    unregisteredTemplateRequest: null,
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
      templateRequest,
      unregisteredTemplateRequest,
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
      // The inline product ITSELF, not just the boolean that says one exists:
      // a boolean cannot be compared against the host route's own bytes.
      publicTransformCode: transformed?.code ?? null,
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

// the non-Vite CSS scoping lane
//
// A Rollup-shaped plugin (no resolved Vite config) routes a style sub-request
// through the native `processStyle` rather than through `compileStyleAsync`.
// Its include gate additionally requires a NON-`css` lang there, so the request
// carries `lang.scss`; it carries `&scoped` because an unscoped request returns
// its input byte-for-byte and would prove nothing.
{
  const label = "vueRollupStyleScoping";
  const errors = [];
  const context = bundlerContext(errors);
  const id = "/probe/Plug.vue";
  const styleId = "/probe/Plug.vue?vue&type=style&index=0&scoped&lang.scss";
  const unregisteredStyleId = "/probe/Unregistered.vue?vue&type=style&index=0&scoped&lang.scss";
  let plugin;
  try {
    plugin = module_.VerterVue.rollup({});
    // The carrier transform first: it is what puts this carrier's compile
    // profile — and therefore its componentId — into the plugin's cache.
    const carrierInclude = hook(plugin, "transformInclude").call(context, id);
    await hook(plugin, "transform").call(context, VUE_SFC, id);

    const styleInclude = hook(plugin, "transformInclude").call(context, styleId);
    const scoped = await hook(plugin, "transform").call(context, NON_VITE_STYLE_SOURCE, styleId);
    // NEGATIVE CONTROL: the same request for a carrier this plugin never
    // transformed has no cached profile, so the lane returns its input.
    const unscoped = await hook(plugin, "transform").call(
      context,
      NON_VITE_STYLE_SOURCE,
      unregisteredStyleId,
    );

    results.cases[label] = {
      outcome: "transformed",
      publicFactory: "VerterVue.rollup",
      id,
      styleId,
      carrierTransformInclude: carrierInclude,
      styleTransformInclude: styleInclude,
      styleSource: NON_VITE_STYLE_SOURCE,
      scopedCode: scoped?.code ?? null,
      scopedMap: scoped?.map ?? null,
      unregisteredId: unregisteredStyleId,
      unregisteredCode: unscoped?.code ?? null,
    };
  } catch (error) {
    results.cases[label] = {
      outcome: "error",
      publicFactory: "VerterVue.rollup",
      id,
      message: String(error?.message ?? error),
      errors,
    };
  } finally {
    if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call(context);
  }
}

// the pre-compile + cross-file recompile lane
//
// `buildStart` is the only entry to it, and it needs real files on disk: a
// production-shaped resolved config, `preCompile`, `crossFileOptimize`, and a
// parent passing a LITERAL prop to a child (the shape the cross-file optimizer
// records constness for). The fixture lives inside the repository so the
// plugin's `createRequire(join(root, "package.json"))` still reaches the
// workspace's own `vue/compiler-sfc`.
//
// The directory is allocated PER INVOCATION under a stable repository-local
// parent, and only that invocation's directory is removed. A fixed path is a
// shared mutable resource: two probes running at once — the ordinary case when
// the consuming suite is not forced onto one thread — would delete each other's
// files mid-build and record an `ENOENT` recompile lane.
//
// ATTRIBUTING THE RECOMPILE WRITE. The pre-compiled and the recompiled module
// are byte-identical (the runtime compile path passes no constness overrides),
// so the published products cannot tell the two apart, and the products alone
// say nothing about the cross-file block. What DOES separate them: an
// observation of `host.getVirtualFile` taken WHILE `buildStart` runs. The hook
// reaches that call at two places — the cross-file recompile block, and the
// compiled-style read the SVELTE pre-compile branch performs — and this lane's
// fixture is Vue-only, so only the recompile block can fire. The two are
// distinguishable regardless: the style read asks for a
// `?verter&type=style&index=…` request, the recompile asks for a BARE
// canonical, and both are recorded so the consumer can tell them apart.
//
// The observation is taken at the NATIVE MODULE BOUNDARY, on the very
// `@verter/native` the plugin's own `createRequire(dist/index.mjs)` resolves;
// the wrapper delegates and hands back the real value, so the lane under
// observation is the shipped code path, unmodified. The wrapper is installed
// only around this lane group and removed immediately after, so no other lane
// in this probe runs against a patched prototype.
//
// One run SUBSTITUTES a marked value for what that one call returns. Whether
// the published module carries the marker is the write itself: the recompiled
// value either reached the cache the load hook serves from, or it did not.
const RECOMPILE_RETURN_MARKER = "\n/* verter-probe: recompile-return */\n";
const nativeRequire = createRequire(entry);
results.nativeEntry = nativeRequire.resolve("@verter/native").split(path.sep).join("/");
{
  const native = nativeRequire("@verter/native");
  const publishedVirtualFile = native.VerterHost.prototype.getVirtualFile;
  if (typeof publishedVirtualFile !== "function") {
    throw new TypeError("the native host exposes no getVirtualFile to observe");
  }

  // "off" while nothing is being observed, so a stray read outside a
  // `buildStart` cannot be recorded as one.
  let phase = "off";
  let reads = [];
  native.VerterHost.prototype.getVirtualFile = function observedGetVirtualFile(...args) {
    const published = publishedVirtualFile.apply(this, args);
    if (phase === "off") return published;
    reads.push({
      rawId: args[0]?.rawId ?? null,
      codeLength: typeof published?.code === "string" ? published.code.length : null,
    });
    if (phase === "substitute") {
      return { ...published, code: `${published.code}${RECOMPILE_RETURN_MARKER}` };
    }
    return published;
  };

  /**
   * Drive `buildStart` once over a fresh two-file fixture, recording what the
   * hook published and every `getVirtualFile` it reached while it ran.
   */
  async function driveRecompileLane(label, { crossFileOptimize, substitute }) {
    const errors = [];
    const context = bundlerContext(errors);
    let fixtureRoot = null;
    let plugin;
    try {
      fixtureRoot = await allocateRecompileFixture(FIXTURE_PARENT);
      const parentId = path.join(fixtureRoot, "Parent.vue").split(path.sep).join("/");
      const childId = path.join(fixtureRoot, "Child.vue").split(path.sep).join("/");
      await writeFile(path.join(fixtureRoot, "Child.vue"), RECOMPILE_CHILD_VUE);
      await writeFile(path.join(fixtureRoot, "Parent.vue"), RECOMPILE_PARENT_VUE);

      plugin = module_.VerterVue.vite({ preCompile: true, crossFileOptimize });
      await plugin.configResolved({
        root: fixtureRoot,
        command: "build",
        build: { ssr: false },
      });

      reads = [];
      phase = substitute ? "substitute" : "observe";
      try {
        await hook(plugin, "buildStart").call(context);
      } finally {
        phase = "off";
      }
      const buildStartVirtualFileCalls = reads;

      // What `buildStart` POPULATED, read back through the plugin's own load
      // hook: a script sub-request is served from the cache the pre-compile
      // loop filled, so a `buildStart` that did nothing publishes nothing here.
      // The observation is disarmed by now, so a cache MISS here would be
      // served with the host's true bytes and could not forge the marker.
      const parentScript = await loadRequest(
        plugin,
        `${parentId}?vue&type=script&lang.js`,
        context,
      );
      const childScript = await loadRequest(plugin, `${childId}?vue&type=script&lang.js`, context);

      results.cases[label] = {
        outcome: "buildStarted",
        publicFactory: "VerterVue.vite",
        crossFileOptimize,
        fixtureRoot: fixtureRoot.split(path.sep).join("/"),
        parentId,
        childId,
        parentSource: RECOMPILE_PARENT_VUE,
        childSource: RECOMPILE_CHILD_VUE,
        parentScript,
        childScript,
        buildStartVirtualFileCalls,
        recompileReturnMarker: substitute ? RECOMPILE_RETURN_MARKER : null,
        errors,
      };
    } catch (error) {
      results.cases[label] = {
        outcome: "error",
        publicFactory: "VerterVue.vite",
        crossFileOptimize,
        message: String(error?.message ?? error),
        stack: String(error?.stack ?? ""),
        errors,
      };
    } finally {
      if (typeof plugin?.closeBundle === "function") await plugin.closeBundle.call(context);
      // ONLY this invocation's directory. The parent is left in place: it is a
      // stable, ignored location, not this run's property.
      if (fixtureRoot !== null) await rm(fixtureRoot, { recursive: true, force: true });
    }
  }

  try {
    // The lane itself.
    await driveRecompileLane("vueRecompileLane", {
      crossFileOptimize: true,
      substitute: false,
    });
    // The NEGATIVE CONTROL: the same drive with the cross-file pass off. It
    // still publishes both modules, so an empty observation is an absent
    // recompile rather than an absent lane.
    await driveRecompileLane("vueRecompileLaneWithoutCrossFile", {
      crossFileOptimize: false,
      substitute: false,
    });
    // The WRITE: the one read `buildStart` takes comes back marked.
    await driveRecompileLane("vueRecompileWriteAttribution", {
      crossFileOptimize: true,
      substitute: true,
    });
  } finally {
    native.VerterHost.prototype.getVirtualFile = publishedVirtualFile;
  }
}

// EVERY enumerated export, driven uniformly
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

// EVERY lane above is one this probe is required to drive: none of them has a
// legitimate `error` outcome. So an errored lane is a failed RUN, not a datum,
// and the record says so at the top level rather than only in the case body —
// otherwise a lane that never touched its subject reads as a success to
// anything checking the exit status or the `loaded`/`fresh` flags. The
// consumer additionally asserts this field, so a run whose exit status is
// swallowed still cannot pass.
// Both maps count: `exportCases` drives one lane per enumerated export through
// the same hooks, and its driver has its own `outcome: "error"` arm. Scanning
// only `cases` would leave that whole family able to error while the process
// still exited 0 — the exact shape this field exists to make impossible.
results.erroredCases = collectErroredCaseLabels(results.cases, results.exportCases);

process.stdout.write(JSON.stringify(results));
process.exitCode = probeExitCode(results.erroredCases);
