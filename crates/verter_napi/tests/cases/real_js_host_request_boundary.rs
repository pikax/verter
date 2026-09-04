//! The host compile request driven from real JS values.
//!
//! The rule under repair lives in `FromNapiValue`, and its input is a live
//! V8 object graph. Three of the distinctions that decide the outcome
//! cannot be modelled in Rust at all: a property STATED as `undefined`
//! versus one that is absent, a key that lives on the prototype rather
//! than on the object, and an array whose length is DECLARED rather than
//! held. A `serde_json::Value` can express none of them, and a Rust model
//! of a JS graph only expresses what its author thought to model.
//!
//! So this suite drives the real boundary: a test-only Node addon whose
//! single entry takes a `NapiHostCompileRequest`, and a JS program that
//! hands it objects a caller could actually write.
//!
//! Discrimination contract: restoring the generic materialisation in
//! `NapiHostCompileRequest::from_napi_value` — reading the payload with
//! `<serde_json::Value as FromNapiValue>::from_napi_value` — turns this
//! suite red. That conversion drops an `undefined`-valued key before the
//! schema sees it, so every cross-framework and unknown-key case here is
//! silently accepted; and it reads properties through
//! `napi_get_property_names`, which walks the prototype chain, so the
//! inherited-key case is refused instead of accepted.
//!
//! Mutation recipes (each plant is unique in production source):
//! - `JsValueMaterializationBudget::per_request` → `new(usize::MAX, usize::MAX)`
//!   in `NapiHostCompileRequest::from_napi_value` turns
//!   `typed-single-refuses-an-oversized-request-graph` green-false.
//! - Drop `napi_is_typedarray` from `NapiValueGraph::classify` and
//!   `typed-single-refuses-a-dense-typed-array` hangs or OOMs enumerating keys.
//! - `empty_diagnostics_snapshot` `hasErrors: false` → `true` turns
//!   `typed-single-throws-a-typed-framework-failure` red.
//! - Restore `Error::from(Unknown::from_raw_unchecked(...))` in
//!   `recover_pending_exception` and
//!   `typed-batch-clears-hostile-thrown-values-and-preserves-siblings` poisons
//!   the sibling.
//! - `input.get::<Buffer>("source")` without `napi_is_buffer` turns
//!   `typed-batch-isolates-a-wrong-typed-source` into a leaked reference;
//!   the case still fails closed on the wrong type.
//! - Skip `napi_is_array` before `get_array_length` and
//!   `typed-batch-refuses-a-non-array-input` becomes a pending-exception
//!   batch failure.
//! - Restore `#[napi(object)] NapiCompileRequestsOptions` and
//!   `typed-batch-refuses-unknown-options` is silently accepted.
//! - Format construction refusals as `{error:?}` and
//!   `typed-batch-isolates-a-canonical-request-refusal` matches
//!   `DuplicateProduct(RuntimeClient)` again.
//! - Return the whole batch as one `Err` when any entry's outcome is a
//!   failure (rather than filling that entry's `failure` slot) and
//!   `typed-batch-isolates-public-api-and-declarations-refusals` loses its
//!   compiling sibling.
//! - Push the caller's raw `canonicalId` into `expected_canonical_ids`
//!   (drop the `resolve_alias_or_canonical` call in `compile_requests`) and
//!   `typed-batch-accepts-non-canonical-ids` fails the whole batch with a
//!   false transposition, discarding its valid sibling too.
//! - Drop the `retained_bytes` bound before `keys.at` in
//!   `decode_compile_requests_priority` and
//!   `typed-batch-bounds-a-hostile-options-key` sees a megabyte key quoted
//!   back verbatim.
//!
//! # Product-kind coverage on both routes
//!
//! `compile_request_response_to_napi` is the one conversion function both
//! `compileRequest` and `compileRequests` share, but the two routes do not
//! share the code that reaches it: the batch route zips each response back
//! onto its input's source and output slot, and the singular route does
//! not. So each route carries its own product-kind coverage rather than
//! inheriting the other's — `analysis` and `runtimeServer` publish their
//! payloads on both (`typed-single-vue-analysis-is-a-json-string`,
//! `typed-single-runtime-server-publishes-its-nodes`,
//! `typed-batch-runs-analysis-and-runtime-server-products`), and
//! `publicApi`/`declarations` refuse on both
//! (`typed-single-refuses-public-api-and-declarations`,
//! `typed-batch-isolates-public-api-and-declarations-refusals`), the batch
//! refusal isolated to its own entry beside compiling siblings.
//!
//! `ideCompanion` is where that zip matters most, because it is the only
//! product whose payload is computed FROM the paired source: its
//! destructured-binding offsets are re-encoded to UTF-16 against it, so a
//! mispairing publishes offsets into the wrong file rather than failing.
//! `typed-single-vue-preserves-ide-utf16-offsets` proves the singular
//! route; `typed-batch-preserves-ide-utf16-offsets-per-entry` proves the
//! batch route, with two entries whose multi-byte prefixes differ so each
//! entry's offset is wrong against the other's source.
//!
//! # Where the addon comes from
//!
//! This suite builds nothing. The addon is its own `cdylib` package that
//! `verter_napi` dev-depends on, so cargo links it while it builds this
//! binary and leaves it beside it in the same `deps` directory;
//! `.config/nextest.toml` carries that one file into a nextest archive at
//! the same relative path, so an archived run finds it beside the binary
//! too. Locating it is therefore reading a sibling path, and a missing
//! addon is a loud failure rather than a rebuild.
//!
//! Building it here instead would mean spawning a `cargo build` that
//! links the native crate's entire dependency closure. Against a cold
//! target directory — which is what an archived run extracted into a
//! fresh tree has — that costs minutes, and minutes inside a workspace
//! run is indistinguishable from a hang: the run terminates the test and
//! cancels every case it had not yet scheduled. Build cost belongs in the
//! build, not in a test body.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Every case the JS program is expected to report on.
///
/// Named here as well as there so a program that dies early, or that
/// quietly stops running a case, fails instead of passing on the cases it
/// did reach.
const EXPECTED_CASES: &[&str] = &[
    "typed-single-vue-produces-runtime-output",
    "typed-single-svelte-produces-runtime-output",
    "typed-single-vue-preserves-ide-utf16-offsets",
    "typed-single-vue-preserves-diagnostic-utf16-spans",
    "typed-single-vue-analysis-is-a-json-string",
    "typed-single-runtime-server-publishes-its-nodes",
    "typed-single-refuses-public-api-and-declarations",
    "typed-single-refuses-an-own-unknown-key",
    "typed-single-throws-a-typed-framework-failure",
    "typed-single-refuses-an-oversized-request-graph",
    "typed-single-refuses-a-dense-typed-array",
    "typed-batch-isolates-a-request-decode-refusal",
    "typed-batch-isolates-a-canonical-request-refusal",
    "typed-batch-isolates-malformed-entry-fields",
    "typed-batch-isolates-invalid-canonical-ids",
    "typed-batch-isolates-a-wrong-typed-source",
    "typed-batch-clears-throwing-accessors-and-preserves-siblings",
    "typed-batch-clears-hostile-thrown-values-and-preserves-siblings",
    "typed-batch-bounds-a-hostile-thrown-string",
    "typed-batch-preserves-order-isolates-failure-and-registers-once",
    "typed-batch-refuses-a-non-array-input",
    "typed-batch-refuses-unknown-options",
    "typed-batch-bounds-a-hostile-options-key",
    "typed-batch-accepts-non-canonical-ids",
    "typed-batch-refuses-binary-options-before-enumerating-them",
    "typed-batch-clears-a-throwing-options-trap-and-still-serves",
    "typed-batch-runs-analysis-and-runtime-server-products",
    "typed-batch-preserves-ide-utf16-offsets-per-entry",
    "typed-batch-answers-an-empty-source-buffer-beside-a-sibling",
    "typed-batch-isolates-public-api-and-declarations-refusals",
    "valid-request-accepted",
    "unknown-top-level-key-stated-as-undefined-refused",
    "unknown-option-key-stated-as-undefined-refused",
    "unknown-product-key-stated-as-undefined-refused",
    "vue-cross-framework-option-stated-as-undefined-refused",
    "svelte-cross-framework-option-stated-as-undefined-refused",
    "required-option-stated-as-undefined-refused",
    "optional-slot-stated-as-undefined-compiles-as-omitted",
    "prototype-inherited-key-ignored",
    "array-declaring-more-elements-than-a-request-may-carry-refused",
    "self-referential-graph-refused",
    "typed-batch-refuses-an-oversized-outer-array",
    "typed-batch-refuses-an-aggregate-source-payload",
    "typed-batch-refuses-an-aggregate-decoded-value-payload",
    "typed-batch-refuses-an-invalid-priority",
];

/// The fixture addon's shared library, as its platform names it.
#[cfg(target_os = "windows")]
const FIXTURE_LIBRARY: &str = "verter_napi_request_fixture.dll";
#[cfg(target_os = "macos")]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.so";

/// Answers the fixture addon built beside this test binary.
///
/// An absent addon fails here, loudly and by name. It is never rebuilt:
/// this suite is the only evidence that discriminates the repair, and a
/// suite that can quietly produce its own input can also quietly produce
/// the wrong one.
fn fixture_addon_library() -> PathBuf {
    let exe = std::env::current_exe().expect("a running test has a path");
    let library = exe
        .parent()
        .expect("a test binary sits in a directory")
        .join(FIXTURE_LIBRARY);
    assert!(
        library.exists(),
        "the fixture addon must sit beside this test binary, at {}. \
         `verter_napi` dev-depends on it so cargo builds it with this \
         binary; a run from a nextest archive additionally needs the \
         `archive.include` entry naming it in `.config/nextest.toml`.",
        library.display()
    );
    library
}

/// The JS program driving the boundary.
///
/// Every case reports one `PASS <name>` or `FAIL <name>: <detail>` line as
/// it finishes, so a case that kills the process is visible as a missing
/// line rather than as a silent pass.
const DRIVER: &str = r#"
const addon = require(process.argv[2]);
const decode = addon.decodeHostCompileRequest;

// `VerterHost` is not re-exported by this fixture crate's own source; it
// reaches this addon's cdylib only because a Rust `cdylib` statically links
// its whole-archive Rust dependency closure (so `verter_napi`'s `#[napi]`
// class registration survives even though nothing in the fixture crate
// names the type). That is real, documented `rustc` `cdylib` linking
// behavior, not a fragile accident — but it is also not something this
// suite should trust silently: if it ever stops holding (a future
// `--gc-sections`/LTO/strip setting, for example), every case below would
// fail with a confusing "not a constructor" error instead of naming the
// actual problem. Assert it explicitly, once, up front.
if (typeof addon.VerterHost !== "function") {
  console.log(`FAIL fixture-addon-exports-VerterHost: typeof addon.VerterHost is ${typeof addon.VerterHost}`);
  process.exit(1);
}

function report(name, failure) {
  console.log(failure === null ? `PASS ${name}` : `FAIL ${name}: ${failure}`);
}

/** The case body answers null when it holds, or why it did not. */
function check(name, body) {
  let failure;
  try {
    failure = body();
  } catch (error) {
    failure = `threw unexpectedly: ${error && error.message}`;
  }
  report(name, failure === undefined ? null : failure);
}

/** Answers the refusal message, or null when the payload was accepted. */
function refusalOf(request) {
  try {
    decode(request);
    return null;
  } catch (error) {
    return String((error && error.message) || error);
  }
}

function refusalOfRoute(call) {
  try {
    call();
    return null;
  } catch (error) {
    return String((error && error.message) || error);
  }
}

function errorOfRoute(call) {
  try {
    call();
    return null;
  } catch (error) {
    return error;
  }
}

function refuses(request, expected) {
  const message = refusalOf(request);
  if (message === null) return `accepted a payload that must be refused`;
  if (!message.includes(expected)) return `refused with ${message}, expected it to name ${expected}`;
  return undefined;
}

const identity = () => ({
  filename: "Comp.vue",
  componentId: "c-1",
  isProduction: false,
  forceJs: false,
});
const vueOptions = () => ({
  backend: "inferred",
  ssr: false,
  isCustomElement: [],
  babelParserPlugins: [],
});
const products = () => [
  { kind: "analysis", wantScriptBindings: true, wantTemplateData: true },
];
const vue = () => ({ framework: "vue", identity: identity(), products: products(), options: vueOptions() });
const svelte = () => ({ framework: "svelte", identity: identity(), products: products(), options: { dev: true } });

function runtimeProduct() {
  return { kind: "runtimeClient", runtimeSourceMap: true };
}

function ideProduct() {
  return {
    kind: "ideCompanion",
    wantSourceMap: true,
    embedAmbientTypes: false,
    conditionalRootNarrowing: false,
    strictSlots: false,
    ideChunkBoundaries: false,
  };
}

function runtimeIdentity(filename) {
  return {
    filename,
    componentId: "typed-route",
    isProduction: false,
    forceJs: false,
  };
}

function vueProducts(filename, requestedProducts) {
  return {
    framework: "vue",
    identity: runtimeIdentity(filename),
    products: requestedProducts,
    options: vueOptions(),
  };
}

function vueRuntime(filename) {
  return vueProducts(filename, [runtimeProduct()]);
}

function svelteRuntime(filename) {
  return {
    framework: "svelte",
    identity: runtimeIdentity(filename),
    products: [runtimeProduct()],
    options: {},
  };
}

function upsertSource(host, canonicalId, source, fileKind) {
  return host.upsert({
    canonicalId,
    inputId: canonicalId,
    source: Buffer.from(source),
    fileKind,
  });
}

function runtimeMain(response) {
  if (!response || response.canonicalId.length === 0) return null;
  const product = response.products[0];
  if (!product || product.kind !== "runtimeClient" || !Array.isArray(product.nodes)) return null;
  return product.nodes.find((node) => node.node.kind === "main") || null;
}

function runtimeNodes(response, kind) {
  if (!response || !Array.isArray(response.products)) return null;
  const product = response.products.find((row) => row.kind === kind);
  if (!product || !Array.isArray(product.nodes)) return null;
  return product.nodes.map((node) => ({
    kind: node.node && node.node.kind,
    index: node.node && node.node.index != null ? node.node.index : null,
    code: node.code,
    sourceMap: node.sourceMap == null ? null : node.sourceMap,
    lang: node.lang == null ? null : node.lang,
    meta: node.meta,
  }));
}

function runtimeProfile(canonicalId, ssr) {
  return {
    filename: canonicalId,
    isProduction: false,
    ssr: !!ssr,
    componentId: "typed-route",
    hmrStrategy: "none",
    forceJs: false,
    sourceMap: true,
    target: "bundler",
  };
}

function legacyRuntimeProduct(canonicalId, source, fileKind, ssr) {
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, fileKind);
  const kinds = host.listVirtualFiles(canonicalId);
  const compileProfile = runtimeProfile(canonicalId, ssr);
  const nodes = kinds.map((nodeKind) => {
    const file = host.getVirtualFile({ canonicalId, nodeKind, compileProfile });
    return {
      kind: nodeKind.kind,
      index: nodeKind.index != null ? nodeKind.index : null,
      code: file.code,
      sourceMap: file.sourceMap == null ? null : file.sourceMap,
      lang: file.lang == null ? null : file.lang,
      meta: file.meta,
    };
  });
  const main = host.getVirtualFile({
    canonicalId,
    nodeKind: { kind: "main" },
    compileProfile,
  });
  host.close();
  return { nodes, diagnostics: main && main.diagnostics };
}

function legacyRuntimeMain(canonicalId, source, fileKind) {
  const product = legacyRuntimeProduct(canonicalId, source, fileKind, false);
  const main = product.nodes.find((node) => node.kind === "main") || null;
  if (!main) return null;
  return { code: main.code, sourceMap: main.sourceMap, diagnostics: product.diagnostics };
}

function sameRuntimeNodes(typed, legacy) {
  return JSON.stringify(typed) === JSON.stringify(legacy);
}

function legacyIde(canonicalId, source) {
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, "vue");
  const profile = {
    filename: canonicalId,
    isProduction: false,
    componentId: "typed-route",
    forceJs: false,
    sourceMap: true,
    target: "ide",
  };
  host.ensureIdeCompiled(canonicalId, profile);
  const response = host.getIde(canonicalId, profile);
  host.close();
  return response;
}

check("typed-single-vue-produces-runtime-output", () => {
  const canonicalId = "/typed/Single.vue";
  const source = `<script setup>const message = "native-vue"</script><template><p>{{ message }}</p></template>`;
  const host = new addon.VerterHost({ metricsEnabled: true });
  upsertSource(host, canonicalId, source, "vue");
  const response = host.compileRequest(canonicalId, vueRuntime(canonicalId));
  const main = runtimeMain(response);
  const typedNodes = runtimeNodes(response, "runtimeClient");
  const legacy = legacyRuntimeProduct(canonicalId, source, "vue", false);
  const metrics = host.getMetrics();
  host.close();
  if (!main || typeof main.code !== "string" || !main.code.includes("Single")) {
    return `typed Vue runtime output was absent: ${JSON.stringify(response)}`;
  }
  if (typeof main.sourceMap !== "string" || !main.sourceMap.includes(canonicalId)) {
    return `typed Vue source map did not preserve the canonical id: ${main.sourceMap}`;
  }
  if (
    !legacy ||
    !typedNodes ||
    typedNodes.length === 0 ||
    !sameRuntimeNodes(typedNodes, legacy.nodes) ||
    JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)
  ) {
    return `typed Vue output diverged from the profile route: ${JSON.stringify({ typedNodes, legacy })}`;
  }
  if (!metrics || metrics.upserts !== 1) {
    return `the source-only registration count was ${metrics && metrics.upserts}, expected 1`;
  }
  return undefined;
});

check("typed-single-svelte-produces-runtime-output", () => {
  const canonicalId = "/typed/Single.svelte";
  const source = `<script>let message = "native-svelte";</script><p>{message}</p>`;
  const host = new addon.VerterHost({ metricsEnabled: true });
  upsertSource(host, canonicalId, source, "svelte");
  const response = host.compileRequest(canonicalId, svelteRuntime(canonicalId));
  const main = runtimeMain(response);
  const typedNodes = runtimeNodes(response, "runtimeClient");
  const legacy = legacyRuntimeProduct(canonicalId, source, "svelte", false);
  const metrics = host.getMetrics();
  host.close();
  if (!main || typeof main.code !== "string" || !main.code.includes("native-svelte")) {
    return `typed Svelte runtime output was absent: ${JSON.stringify(response)}`;
  }
  if (typeof main.sourceMap !== "string" || main.sourceMap.length === 0) {
    return `typed Svelte source map was absent: ${main.sourceMap}`;
  }
  if (
    !legacy ||
    !typedNodes ||
    typedNodes.length === 0 ||
    !sameRuntimeNodes(typedNodes, legacy.nodes) ||
    JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)
  ) {
    return `typed Svelte output diverged from the profile route: ${JSON.stringify({ typedNodes, legacy })}`;
  }
  if (!metrics || metrics.upserts !== 1) {
    return `the source-only registration count was ${metrics && metrics.upserts}, expected 1`;
  }
  return undefined;
});

check("typed-single-vue-preserves-ide-utf16-offsets", () => {
  const canonicalId = "/typed/Offsets.vue";
  const source = `<script setup lang="ts">
// héllo — 😀
const greeting = "x"
const { title = greeting } = defineProps<{ title?: string }>()
</script>
<template><div>{{ title }}</div></template>`;
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, "vue");
  const response = host.compileRequest(canonicalId, vueProducts(canonicalId, [ideProduct()]));
  host.close();
  const product = response.products[0];
  const ide = product && product.kind === "ideCompanion" && product.ide;
  const published = ide && ide.destructuredBlock;
  const binding = published && published.bindings.find((entry) => entry.name === "greeting");
  if (!binding) return `typed IDE destructured binding was absent: ${JSON.stringify(response)}`;
  const utf16Start = source.indexOf("greeting");
  const byteStart = Buffer.byteLength(source.slice(0, utf16Start));
  if (byteStart === utf16Start) return "fixture did not distinguish UTF-8 bytes from UTF-16 units";
  if (binding.sourceStart !== utf16Start) {
    return `binding sourceStart was ${binding.sourceStart}, expected UTF-16 offset ${utf16Start} (byte ${byteStart})`;
  }
  if (!(published.blockStart > source.length && published.blockStart <= ide.code.length)) {
    return `generated blockStart ${published.blockStart} did not index IDE code of length ${ide.code.length}`;
  }
  const legacy = legacyIde(canonicalId, source);
  if (!legacy || JSON.stringify(ide) !== JSON.stringify(legacy)) {
    return `typed IDE output diverged from the profile route: ${JSON.stringify({ ide, legacy })}`;
  }
  return undefined;
});

check("typed-single-vue-preserves-diagnostic-utf16-spans", () => {
  const template = (prefix) => `<script setup lang="ts">const ok = true</script>
<template><p>${prefix}</p><div v-if="ok" v-if="ok">shown</div></template>`;
  const multibyte = template("héllo — 😀");
  const ascii = template("plain-text");
  if (multibyte.length !== ascii.length || Buffer.byteLength(multibyte) === Buffer.byteLength(ascii)) {
    return "fixture did not preserve UTF-16 length while changing UTF-8 byte length";
  }
  const host = new addon.VerterHost();
  upsertSource(host, "/typed/Warn.vue", multibyte, "vue");
  upsertSource(host, "/typed/WarnAscii.vue", ascii, "vue");
  const demand = (canonicalId) => vueProducts(canonicalId, [
    { kind: "runtimeClient", runtimeSourceMap: false },
  ]);
  const response = host.compileRequest("/typed/Warn.vue", demand("/typed/Warn.vue"));
  const asciiResponse = host.compileRequest(
    "/typed/WarnAscii.vue",
    demand("/typed/WarnAscii.vue"),
  );
  host.close();
  const spans = (value) => value.diagnostics.diagnostics.map((diagnostic) => [
    diagnostic.spanStart,
    diagnostic.spanEnd,
  ]);
  const published = spans(response);
  if (published.length === 0 || !published.some(([start]) => start > 0)) {
    return `fixture produced no discriminating diagnostics: ${JSON.stringify(response.diagnostics)}`;
  }
  if (JSON.stringify(published) !== JSON.stringify(spans(asciiResponse))) {
    return `diagnostic offsets changed with UTF-8 byte length: ${JSON.stringify({ published, ascii: spans(asciiResponse) })}`;
  }
  const legacy = legacyRuntimeProduct("/typed/Warn.vue", multibyte, "vue", false);
  if (!legacy || JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)) {
    return `typed diagnostics diverged from the profile route: ${JSON.stringify({ typed: response.diagnostics, legacy: legacy && legacy.diagnostics })}`;
  }
  const diagnostic = response.diagnostics.diagnostics[0];
  if (!Array.isArray(diagnostic.arguments)) {
    return `typed diagnostics dropped arguments: ${JSON.stringify(diagnostic)}`;
  }
  return undefined;
});

check("typed-single-refuses-an-own-unknown-key", () => {
  const canonicalId = "/typed/Closed.vue";
  const source = `<template><p>closed</p></template>`;
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, "vue");
  const request = vueRuntime(canonicalId);
  request.options.runes = undefined;
  const refusal = refusalOfRoute(() => host.compileRequest(canonicalId, request));
  host.close();
  if (refusal === null) return "accepted a cross-framework own property";
  if (!refusal.includes("unknown field `runes`")) {
    return `refused with ${refusal}, expected it to name runes`;
  }
  return undefined;
});

// @ai-generated - The singular route must throw its complete discriminated failure object.
check("typed-single-throws-a-typed-framework-failure", () => {
  const canonicalId = "/typed/TypedFailure.vue";
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, `<template><p>vue</p></template>`, "vue");
  const error = errorOfRoute(() => host.compileRequest(canonicalId, svelteRuntime(canonicalId)));
  host.close();
  if (!(error instanceof Error)) return `route did not throw an Error: ${String(error)}`;
  if (error.code !== "InvalidArg") return `framework mismatch status was ${error.code}`;
  if (error.kind !== "frameworkMismatch" || error.canonicalId !== canonicalId) {
    return `thrown failure lost its discriminator or id: ${JSON.stringify(error)}`;
  }
  if (error.requestedFramework !== "svelte" || error.registeredFramework !== "vue") {
    return `thrown failure lost framework values: ${JSON.stringify({ requested: error.requestedFramework, registered: error.registeredFramework })}`;
  }
  if (
    !error.diagnostics ||
    error.diagnostics.hasErrors !== false ||
    !Array.isArray(error.diagnostics.diagnostics) ||
    error.diagnostics.diagnostics.length !== 0
  ) {
    return `thrown failure snapshot must stay empty and not claim errors: ${JSON.stringify(error.diagnostics)}`;
  }
  return undefined;
});

check("typed-single-vue-analysis-is-a-json-string", () => {
  const canonicalId = "/typed/Analysis.vue";
  const source = `<script setup>const n = 1</script><template><p>{{ n }}</p></template>`;
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, "vue");
  const response = host.compileRequest(
    canonicalId,
    vueProducts(canonicalId, [{ kind: "analysis", wantScriptBindings: false, wantTemplateData: true }]),
  );
  host.close();
  const product = response.products[0];
  if (!product || product.kind !== "analysis" || typeof product.analysis !== "string") {
    return `analysis row was not a JSON string: ${JSON.stringify(product)}`;
  }
  const keys = Object.keys(product).sort();
  if (JSON.stringify(keys) !== JSON.stringify(["analysis", "kind"])) {
    return `analysis row published inactive union keys: ${JSON.stringify(keys)}`;
  }
  const parsed = JSON.parse(product.analysis);
  if (!parsed || typeof parsed !== "object") return "analysis JSON was not an object";
  return undefined;
});

check("typed-single-runtime-server-publishes-its-nodes", () => {
  const canonicalId = "/typed/Server.vue";
  const source = `<script setup>const n = 1</script><template><p>{{ n }}</p></template>`;
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, "vue");
  const request = vueProducts(canonicalId, [{ kind: "runtimeServer", runtimeSourceMap: true }]);
  request.options.ssr = true;
  const response = host.compileRequest(canonicalId, request);
  host.close();
  const typedNodes = runtimeNodes(response, "runtimeServer");
  const legacy = legacyRuntimeProduct(canonicalId, source, "vue", true);
  if (!typedNodes || typedNodes.length === 0) {
    return `runtimeServer nodes were absent: ${JSON.stringify(response)}`;
  }
  if (!legacy || !sameRuntimeNodes(typedNodes, legacy.nodes)) {
    return `runtimeServer nodes diverged from the profile route: ${JSON.stringify({ typedNodes, legacy })}`;
  }
  return undefined;
});

check("typed-single-refuses-public-api-and-declarations", () => {
  const source = `<template><p>x</p></template>`;
  for (const kind of ["publicApi", "declarations"]) {
    const canonicalId = `/typed/Unsupported-${kind}.vue`;
    const host = new addon.VerterHost();
    upsertSource(host, canonicalId, source, "vue");
    const error = errorOfRoute(() => host.compileRequest(canonicalId, vueProducts(canonicalId, [{ kind }])));
    host.close();
    if (!(error instanceof Error)) return `${kind} did not throw an Error`;
    if (error.kind !== "unsupportedProduct" || error.productKind !== kind) {
      return `${kind} failure was ${JSON.stringify({ kind: error.kind, productKind: error.productKind })}`;
    }
    if (error.products) return `${kind} published a product row`;
  }
  return undefined;
});

check("typed-single-refuses-an-oversized-request-graph", () => {
  const canonicalId = "/typed/HugeGraph.vue";
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, `<template><p>x</p></template>`, "vue");
  const request = vueRuntime(canonicalId);
  request.options.isCustomElement = Array.from({ length: 40000 }, () => ({ a: 1, b: 2, c: 3 }));
  const refusal = refusalOfRoute(() => host.compileRequest(canonicalId, request));
  host.close();
  if (refusal === null) return "accepted a request graph above the per-request value budget";
  if (!refusal.includes("decoded values")) return `oversized request refusal was ${refusal}`;
  return undefined;
});

check("typed-single-refuses-a-dense-typed-array", () => {
  const canonicalId = "/typed/TypedArray.vue";
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, `<template><p>x</p></template>`, "vue");
  const request = vueRuntime(canonicalId);
  request.options.cssModules = new Uint8Array(1024);
  const refusal = refusalOfRoute(() => host.compileRequest(canonicalId, request));
  host.close();
  if (refusal === null) return "accepted a typed array as a request object";
  if (!refusal.includes("binary or typed-array")) return `typed-array refusal was ${refusal}`;
  return undefined;
});

check("typed-batch-isolates-a-request-decode-refusal", () => {
  const badId = "/typed/DecodeRefusal.vue";
  const goodId = "/typed/AfterDecodeRefusal.svelte";
  const badRequest = vueRuntime(badId);
  badRequest.options.runes = undefined;
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: badId,
      source: Buffer.from(`<template><p>closed</p></template>`),
      request: badRequest,
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still-compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ]);
  host.close();
  if (entries.length !== 2 || entries[0].canonicalId !== badId || entries[1].canonicalId !== goodId) {
    return `batch decode refusal changed entry count or order: ${JSON.stringify(entries)}`;
  }
  if (entries[0].response || !entries[0].failure || entries[0].failure.kind !== "binding") {
    return `request decode refusal was not isolated to its entry: ${JSON.stringify(entries[0])}`;
  }
  if (!entries[0].failure.message.includes("unknown field `runes`")) {
    return `decode refusal did not name runes: ${entries[0].failure.message}`;
  }
  if (!entries[1].response || entries[1].failure || !runtimeMain(entries[1].response)) {
    return `valid sibling after decode refusal did not compile: ${JSON.stringify(entries[1])}`;
  }
  return undefined;
});

// @ai-generated - A canonical product-set refusal must stay positional and not suppress siblings.
check("typed-batch-isolates-a-canonical-request-refusal", () => {
  const firstId = "/typed/BeforeCanonicalRefusal.vue";
  const badId = "/typed/CanonicalRefusal.vue";
  const lastId = "/typed/AfterCanonicalRefusal.svelte";
  const duplicate = vueRuntime(badId);
  duplicate.products.push({ kind: "runtimeClient", runtimeSourceMap: true });
  const host = new addon.VerterHost({ metricsEnabled: true });
  const entries = host.compileRequests([
    {
      canonicalId: firstId,
      source: Buffer.from(`<template><p>first-valid</p></template>`),
      request: vueRuntime(firstId),
    },
    {
      canonicalId: badId,
      source: Buffer.from(`<template><p>duplicate-product</p></template>`),
      request: duplicate,
    },
    {
      canonicalId: lastId,
      source: Buffer.from(`<p>last-valid</p>`),
      request: svelteRuntime(lastId),
    },
  ]);
  const metrics = host.getMetrics();
  host.close();
  if (entries.length !== 3 || entries[0].canonicalId !== firstId || entries[2].canonicalId !== lastId) {
    return `canonical refusal changed entry count or order: ${JSON.stringify(entries)}`;
  }
  if (!entries[0].response || entries[0].failure || !runtimeMain(entries[0].response)) {
    return `valid sibling before canonical refusal did not compile: ${JSON.stringify(entries[0])}`;
  }
  if (entries[1].response || !entries[1].failure || entries[1].failure.kind !== "binding") {
    return `canonical refusal was not isolated: ${JSON.stringify(entries[1])}`;
  }
  if (
    !entries[1].failure.message.includes("duplicate product 'runtimeClient'") ||
    entries[1].failure.message.includes("DuplicateProduct")
  ) {
    return `canonical refusal lost its reason: ${entries[1].failure.message}`;
  }
  if (!entries[2].response || entries[2].failure || !runtimeMain(entries[2].response)) {
    return `valid sibling after canonical refusal did not compile: ${JSON.stringify(entries[2])}`;
  }
  if (!metrics || metrics.upserts !== 2) {
    return `canonical refusal prevented valid registrations: ${metrics && metrics.upserts}`;
  }
  return undefined;
});

// @ai-generated - Field and UTF-8 decoding failures stay positional and preserve valid work.
check("typed-batch-isolates-malformed-entry-fields", () => {
  const invalidUtf8Id = "/typed/InvalidUtf8.vue";
  const missingRequestId = "/typed/MissingRequest.vue";
  const goodId = "/typed/AfterMalformed.svelte";
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: invalidUtf8Id,
      source: Buffer.from([0xff]),
      request: vueRuntime(invalidUtf8Id),
    },
    {
      canonicalId: missingRequestId,
      source: Buffer.from(`<template><p>missing request</p></template>`),
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ]);
  host.close();
  if (entries.length !== 3 || entries[2].canonicalId !== goodId) {
    return `malformed fields changed entry count or order: ${JSON.stringify(entries)}`;
  }
  if (entries[0].response || entries[0].failure?.kind !== "binding") {
    return `invalid UTF-8 was not an isolated binding failure: ${JSON.stringify(entries[0])}`;
  }
  if (entries[1].response || entries[1].failure?.kind !== "binding" ||
      !entries[1].failure.message.includes("request")) {
    return `missing request was not an isolated binding failure: ${JSON.stringify(entries[1])}`;
  }
  if (!entries[2].response || entries[2].failure || !runtimeMain(entries[2].response)) {
    return `valid sibling after malformed fields did not compile: ${JSON.stringify(entries[2])}`;
  }
  return undefined;
});

check("typed-batch-isolates-invalid-canonical-ids", () => {
  const goodId = "/typed/AfterInvalidId.vue";
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      source: Buffer.from(`<template><p>missing id</p></template>`),
      request: vueRuntime("/typed/Missing.vue"),
    },
    {
      canonicalId: 42,
      source: Buffer.from(`<template><p>malformed id</p></template>`),
      request: vueRuntime("/typed/Malformed.vue"),
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<template><p>still compiled</p></template>`),
      request: vueRuntime(goodId),
    },
  ]);
  host.close();
  if (entries.length !== 3 || entries[2].canonicalId !== goodId) {
    return `invalid ids changed entry count or order: ${JSON.stringify(entries)}`;
  }
  for (const [position, entry] of entries.slice(0, 2).entries()) {
    if (entry.canonicalId !== "" || entry.response || !entry.failure || entry.failure.kind !== "binding") {
      return `invalid id at ${position} was not an isolated binding failure: ${JSON.stringify(entry)}`;
    }
    if (!entry.failure.message.includes("canonicalId")) {
      return `invalid id at ${position} was not named by its failure: ${entry.failure.message}`;
    }
  }
  if (!entries[2].response || entries[2].failure || !runtimeMain(entries[2].response)) {
    return `valid sibling after invalid ids did not compile: ${JSON.stringify(entries[2])}`;
  }
  return undefined;
});

// @ai-generated - Thrown element and field accessors must not poison later N-API work.
check("typed-batch-clears-throwing-accessors-and-preserves-siblings", () => {
  const goodId = "/typed/AfterThrowingAccessors.svelte";
  const throwingEntry = (field) => {
    const id = `/typed/Throws-${field}.vue`;
    const entry = {
      canonicalId: id,
      source: Buffer.from(`<template><p>${field}</p></template>`),
      request: vueRuntime(id),
    };
    Object.defineProperty(entry, field, {
      enumerable: true,
      get() { throw new Error(`boom-${field}`); },
    });
    return entry;
  };
  const inputs = [
    {},
    throwingEntry("canonicalId"),
    throwingEntry("source"),
    throwingEntry("request"),
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ];
  Object.defineProperty(inputs, 0, {
    enumerable: true,
    get() { throw new Error("boom-element"); },
  });

  const host = new addon.VerterHost();
  const entries = host.compileRequests(inputs);
  host.close();
  if (entries.length !== inputs.length || entries.at(-1).canonicalId !== goodId) {
    return `throwing accessors changed entry count or order: ${JSON.stringify(entries)}`;
  }
  const reasons = entries.slice(0, 4).map((entry) => entry.failure?.message ?? "");
  for (const expected of ["boom-element", "boom-canonicalId", "boom-source", "boom-request"]) {
    if (!reasons.some((reason) => reason.includes(expected))) {
      return `throwing accessor reason ${expected} was not captured: ${JSON.stringify(reasons)}`;
    }
  }
  if (!entries.at(-1).response || entries.at(-1).failure || !runtimeMain(entries.at(-1).response)) {
    return `valid sibling after throwing accessors did not compile: ${JSON.stringify(entries.at(-1))}`;
  }
  return undefined;
});

check("typed-batch-clears-hostile-thrown-values-and-preserves-siblings", () => {
  const goodId = "/typed/AfterHostileThrow.svelte";
  const hostile = {
    get message() { throw { get [Symbol.toPrimitive]() { throw new Error("boom-toPrimitive"); } }; },
    [Symbol.toPrimitive]() { throw new Error("boom-toPrimitive"); },
  };
  const inputs = [
    {
      canonicalId: "/typed/Hostile.vue",
      source: Buffer.from(`<template><p>hostile</p></template>`),
      request: vueRuntime("/typed/Hostile.vue"),
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ];
  Object.defineProperty(inputs, 0, {
    enumerable: true,
    get() { throw hostile; },
  });
  const host = new addon.VerterHost();
  const entries = host.compileRequests(inputs);
  host.close();
  if (entries.length !== 2 || entries[1].canonicalId !== goodId) {
    return `hostile throw changed entry count or order: ${JSON.stringify(entries)}`;
  }
  if (entries[0].response || entries[0].failure?.kind !== "binding") {
    return `hostile throw was not an isolated binding failure: ${JSON.stringify(entries[0])}`;
  }
  if (!entries[1].response || entries[1].failure || !runtimeMain(entries[1].response)) {
    return `valid sibling after a hostile throw did not compile: ${JSON.stringify(entries[1])}`;
  }
  return undefined;
});

// A thrown string is caller-controlled and unbounded, and it is copied
// into a Rust error message. The length probe in front of that copy is a
// BOUND, not a formality: above it the generic label is answered instead,
// so a hostile multi-megabyte throw cannot make the binding retain it.
//
// Mutation recipe: discard the probe (`…and_then(|_| String::from_napi_
// value(...))`, or drop the `> MAX_EXCEPTION_MESSAGE_BYTES` branch). The
// huge string is then copied verbatim and this case reports a message
// megabytes long instead of the label.
check("typed-batch-bounds-a-hostile-thrown-string", () => {
  const goodId = "/typed/AfterHugeThrow.svelte";
  const huge = "x".repeat(1 << 20);
  const inputs = [
    {
      canonicalId: "/typed/HugeThrow.vue",
      source: Buffer.from(`<template><p>huge</p></template>`),
      request: vueRuntime("/typed/HugeThrow.vue"),
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ];
  Object.defineProperty(inputs, 0, {
    enumerable: true,
    get() {
      throw huge;
    },
  });
  const host = new addon.VerterHost();
  const entries = host.compileRequests(inputs);
  host.close();
  if (entries.length !== 2 || entries[1].canonicalId !== goodId) {
    return `a huge thrown string changed entry count or order: ${JSON.stringify(entries)}`;
  }
  const message = entries[0].failure && entries[0].failure.message;
  if (typeof message !== "string") {
    return `the huge throw was not an isolated failure: ${JSON.stringify(entries[0])}`;
  }
  if (message.includes(huge) || message.length > 4096) {
    return `the huge thrown string was retained verbatim (${message.length} chars)`;
  }
  if (!message.includes("JavaScript exception")) {
    return `an over-long throw must answer the generic label, got ${message}`;
  }
  if (!entries[1].response || entries[1].failure || !runtimeMain(entries[1].response)) {
    return `valid sibling after a huge throw did not compile: ${JSON.stringify(entries[1])}`;
  }
  return undefined;
});

check("typed-batch-isolates-a-wrong-typed-source", () => {
  const badId = "/typed/WrongSource.vue";
  const goodId = "/typed/AfterWrongSource.svelte";
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: badId,
      source: {},
      request: vueRuntime(badId),
    },
    {
      canonicalId: goodId,
      source: Buffer.from(`<p>still compiled</p>`),
      request: svelteRuntime(goodId),
    },
  ]);
  host.close();
  if (entries.length !== 2 || entries[1].canonicalId !== goodId) {
    return `wrong-typed source changed entry count or order: ${JSON.stringify(entries)}`;
  }
  if (
    entries[0].response ||
    entries[0].failure?.kind !== "binding" ||
    !entries[0].failure.message.includes("Buffer")
  ) {
    return `wrong-typed source was not an isolated Buffer refusal: ${JSON.stringify(entries[0])}`;
  }
  if (!entries[1].response || entries[1].failure || !runtimeMain(entries[1].response)) {
    return `valid sibling after a wrong-typed source did not compile: ${JSON.stringify(entries[1])}`;
  }
  return undefined;
});

check("typed-batch-preserves-order-isolates-failure-and-registers-once", () => {
  const vueId = "/typed/Batch.vue";
  const badId = "/typed/Bad.vue";
  const svelteId = "/typed/Batch.svelte";
  const vueSource = `<template><p>batch-vue</p></template>`;
  const badSource = `<template><p>wrong-framework</p></template>`;
  const svelteSource = `<p>batch-svelte</p>`;
  const host = new addon.VerterHost({ metricsEnabled: true });
  const entries = host.compileRequests(
    [
      { canonicalId: vueId, source: Buffer.from(vueSource), request: vueRuntime(vueId) },
      { canonicalId: vueId, source: Buffer.from(vueSource), request: vueRuntime(vueId) },
      { canonicalId: badId, source: Buffer.from(badSource), request: svelteRuntime(badId) },
      { canonicalId: svelteId, source: Buffer.from(svelteSource), request: svelteRuntime(svelteId) },
    ],
    { priority: "background" },
  );
  const metrics = host.getMetrics();
  host.close();
  const ids = entries.map((entry) => entry.canonicalId);
  if (JSON.stringify(ids) !== JSON.stringify([vueId, vueId, badId, svelteId])) {
    return `batch order changed: ${JSON.stringify(ids)}`;
  }
  if (!entries[0].response || entries[0].failure || !entries[1].response || entries[1].failure) {
    return `duplicate-canonical successes were not isolated responses: ${JSON.stringify(entries)}`;
  }
  const runtimeProductKeys = Object.keys(entries[0].response.products[0]).sort();
  if (JSON.stringify(runtimeProductKeys) !== JSON.stringify(["kind", "nodes"])) {
    return `runtime product published inactive union keys: ${JSON.stringify(runtimeProductKeys)}`;
  }
  if (entries[2].response || !entries[2].failure || entries[2].failure.kind !== "frameworkMismatch") {
    return `framework mismatch was not a typed entry failure: ${JSON.stringify(entries[2])}`;
  }
  const frameworkFailureKeys = Object.keys(entries[2].failure).sort();
  const expectedFailureKeys = [
    "canonicalId",
    "diagnostics",
    "kind",
    "message",
    "registeredFramework",
    "requestedFramework",
  ];
  if (JSON.stringify(frameworkFailureKeys) !== JSON.stringify(expectedFailureKeys)) {
    return `framework failure published inactive union keys: ${JSON.stringify(frameworkFailureKeys)}`;
  }
  if (
    entries[2].failure.requestedFramework !== "svelte" ||
    entries[2].failure.registeredFramework !== "vue"
  ) {
    return `framework failure lost its values: ${JSON.stringify(entries[2].failure)}`;
  }
  if (!entries[3].response || entries[3].failure) {
    return `the sibling after a refusal did not compile: ${JSON.stringify(entries[3])}`;
  }
  if (!metrics || metrics.upserts !== 3) {
    return `four inputs over three canonicals performed ${metrics && metrics.upserts} registrations`;
  }
  return undefined;
});

// A caller's id is not the host's id. The host resolves every id it is
// handed through one identity function — a drive letter lowercases,
// backslashes become slashes, a bundler query tail and an extended-length
// prefix are stripped, whitespace is trimmed, a registered alias resolves —
// and it does that to the batch's inputs before registering or compiling
// them. So the ids the entries report are canonical, and a binding that
// correlated its output slots by comparing the CALLER's spelling against
// them would read every one of these as a transposed batch.
//
// The Windows row is the load-bearing one: on Windows the id a Node tool
// naturally holds is a drive-letter path with backslashes, so a
// raw-spelling comparison fails this route BY DEFAULT there while passing
// on POSIX.
check("typed-batch-accepts-non-canonical-ids", () => {
  const windowsId = "D:\typed\Windows.vue";
  const windowsCanonical = "d:/typed/Windows.vue";
  const queryId = "/typed/Query.vue?vue&type=script";
  const queryCanonical = "/typed/Query.vue";
  const plainId = "/typed/Plain.vue";
  const source = `<template><p>non-canonical</p></template>`;
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    { canonicalId: windowsId, source: Buffer.from(source), request: vueRuntime(windowsId) },
    { canonicalId: queryId, source: Buffer.from(source), request: vueRuntime(queryId) },
    { canonicalId: plainId, source: Buffer.from(source), request: vueRuntime(plainId) },
  ]);
  host.close();
  const ids = entries.map((entry) => entry.canonicalId);
  const expected = [windowsCanonical, queryCanonical, plainId];
  if (JSON.stringify(ids) !== JSON.stringify(expected)) {
    return `entries did not report canonical ids in input order: ${JSON.stringify(ids)}`;
  }
  const failed = entries.filter((entry) => entry.failure || !entry.response);
  if (failed.length > 0) {
    return `a non-canonical id lost its entry: ${JSON.stringify(failed)}`;
  }
  if (entries.some((entry) => !runtimeMain(entry.response))) {
    return `a non-canonical id compiled to no main node: ${JSON.stringify(ids)}`;
  }
  return undefined;
});

check("valid-request-accepted", () => {
  const rendered = decode(vue());
  if (typeof rendered !== "string" || !rendered.includes("Vue")) {
    return `a valid Vue request rendered as ${rendered}`;
  }
  return undefined;
});

check("unknown-top-level-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.bogus = undefined;
  return refuses(request, "unknown field `bogus`");
});

check("unknown-option-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.hoistStatick = undefined;
  return refuses(request, "unknown field `hoistStatick`");
});

check("unknown-product-key-stated-as-undefined-refused", () => {
  const request = vue();
  request.products[0].bogus = undefined;
  return refuses(request, "unknown field `bogus`");
});

check("vue-cross-framework-option-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.runes = undefined;
  return refuses(request, "unknown field `runes`");
});

check("svelte-cross-framework-option-stated-as-undefined-refused", () => {
  const request = svelte();
  request.options.backend = undefined;
  return refuses(request, "unknown field `backend`");
});

check("required-option-stated-as-undefined-refused", () => {
  const request = vue();
  request.options.ssr = undefined;
  return refuses(request, "invalid type: null, expected a boolean");
});

check("optional-slot-stated-as-undefined-compiles-as-omitted", () => {
  const stated = vue();
  stated.options.hmr = undefined;
  const omitted = vue();
  // A slot the base payload never mentions, so this also proves a slot
  // stated as `undefined` and a slot never written are one request.
  if (Object.prototype.hasOwnProperty.call(omitted.options, "hmr")) {
    return "the base payload was expected not to mention `hmr`";
  }
  const withStated = decode(stated);
  const withOmitted = decode(omitted);
  if (withStated !== withOmitted) {
    return `stated ${withStated} but omitted ${withOmitted}`;
  }
  return undefined;
});

check("prototype-inherited-key-ignored", () => {
  // A payload is its own properties. `runes` is a Svelte option and would
  // be refused as an own key of a Vue request; inherited, it is not part
  // of what the caller wrote.
  const request = vue();
  request.options = Object.assign(Object.create({ runes: true }), vueOptions());
  if (request.options.runes !== true) {
    return "the fixture failed to put `runes` on the prototype";
  }
  if (Object.prototype.hasOwnProperty.call(request.options, "runes")) {
    return "the fixture put `runes` on the object rather than the prototype";
  }
  const inherited = decode(request);
  const plain = decode(vue());
  if (inherited !== plain) {
    return `an inherited key changed the request: ${inherited} vs ${plain}`;
  }
  return undefined;
});

check("array-declaring-more-elements-than-a-request-may-carry-refused", () => {
  // A declared length costs nothing in V8; materialising it unbounded
  // would reserve tens of gigabytes and abort the process.
  const request = vue();
  request.options.isCustomElement = new Array(4294967295);
  return refuses(request, "declares 4294967295 elements");
});

check("self-referential-graph-refused", () => {
  const cyclic = {};
  cyclic.self = cyclic;
  const request = vue();
  request.options.cssModules = cyclic;
  return refuses(request, "refers back to itself");
});

// @ai-generated - The outer batch length must be checked before napi-rs can reserve it.
check("typed-batch-refuses-an-oversized-outer-array", () => {
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests(new Array(4294967295)));
  host.close();
  if (refusal === null) return "accepted an outer batch above the array bound";
  if (!refusal.includes("declares 4294967295 elements")) {
    return `oversized outer batch refusal was ${refusal}`;
  }
  return undefined;
});

// @ai-generated - Reusing one Buffer must still charge every retained native source copy.
check("typed-batch-refuses-an-aggregate-source-payload", () => {
  const sharedSource = Buffer.alloc(8 * 1024 * 1024, 0x20);
  const inputs = Array.from({ length: 9 }, (_, index) => {
    const canonicalId = `/typed/Large-${index}.svelte`;
    return { canonicalId, source: sharedSource, request: svelteRuntime(canonicalId) };
  });
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests(inputs));
  host.close();
  if (refusal === null) return "accepted an aggregate source payload above the batch budget";
  if (!refusal.includes("retains more than") || !refusal.includes("bytes")) {
    return `aggregate source refusal was ${refusal}`;
  }
  return undefined;
});

// @ai-generated - Reusing one nested request array must consume one shared decode budget.
check("typed-batch-refuses-an-aggregate-decoded-value-payload", () => {
  const canonicalId = "/typed/Values.vue";
  const request = vueRuntime(canonicalId);
  request.options.isCustomElement = Array.from({ length: 40000 }, () => ({ a: 1, b: 2, c: 3 }));
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId,
      source: Buffer.from(`<template><p>x</p></template>`),
      request,
    },
  ]);
  host.close();
  if (
    entries.length !== 1 ||
    entries[0].response ||
    entries[0].failure?.kind !== "binding" ||
    !entries[0].failure.message.includes("decoded values")
  ) {
    return `per-request decode refusal was not isolated: ${JSON.stringify(entries)}`;
  }
  return undefined;
});

// @ai-generated - Invalid batch-wide scheduler options must refuse before execution.
check("typed-batch-refuses-an-invalid-priority", () => {
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests([], { priority: "urgent" }));
  host.close();
  if (refusal === null) return "accepted an invalid batch priority";
  if (!refusal.includes("invalid priority 'urgent'")) {
    return `invalid priority refusal was ${refusal}`;
  }
  return undefined;
});

check("typed-batch-refuses-unknown-options", () => {
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() =>
    host.compileRequests([], { priority: "background", prioirty: "interactive" }),
  );
  host.close();
  if (refusal === null) return "accepted an unknown batch option";
  if (!refusal.includes("unknown field `prioirty`")) {
    return `unknown option refusal was ${refusal}`;
  }
  return undefined;
});

// An own key is as caller-controlled and as unbounded as a thrown string,
// and the refusal that names it is a thrown Error message. The key is
// therefore MEASURED before it is copied, exactly as a thrown string is,
// and above the bound it is named by its size rather than quoted.
check("typed-batch-bounds-a-hostile-options-key", () => {
  const huge = "x".repeat(1 << 20);
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests([], { [huge]: "background" }));
  host.close();
  if (refusal === null) return "accepted an unbounded unknown batch option key";
  if (refusal.includes(huge) || refusal.length > 4096) {
    return `the huge option key was quoted verbatim (${refusal.length} chars)`;
  }
  if (!refusal.includes("unknown field")) {
    return `an over-long option key must still refuse as an unknown field, got ${refusal}`;
  }
  return undefined;
});

// `options` is the one enumerated argument on this route that is not
// already an array, so `napi_is_array` does not protect it. A Buffer,
// typed array or DataView exposes every byte index as an enumerable own
// key, so enumerating one materialises a V8 key string per byte before any
// count can refuse them — which is why the options value is classified,
// not merely array-checked.
//
// Mutation recipe: replace the `classify` call in
// `decode_compile_requests_priority` with the old
// `ty != ValueType::Object || napi_value_is_array(...)` check. Both
// payloads below are then enumerated and refused as `unknown field `0``,
// so both assertions go red — and at a caller's chosen size, enumerating
// is what the refusal costs.
check("typed-batch-refuses-binary-options-before-enumerating-them", () => {
  const host = new addon.VerterHost();
  const refusals = [new Uint8Array(1024), Buffer.alloc(1024), new DataView(new ArrayBuffer(1024))]
    .map((payload) => refusalOfRoute(() => host.compileRequests([], payload)));
  host.close();
  for (const [index, refusal] of refusals.entries()) {
    if (refusal === null) return `accepted binary batch options at index ${index}`;
    if (!refusal.includes("binary or typed-array")) {
      return `binary batch options at index ${index} were refused with ${refusal}`;
    }
  }
  return undefined;
});

// A throwing accessor on the options argument has to become a THROWN
// refusal, and must not poison the next call.
//
// Two things have to hold, and neither implies the other. The exception
// has to be captured and cleared, or every later N-API call on this env
// fails. And the recovered error has to stop claiming `PendingException`:
// `JsError::throw_into` drops an error carrying that status instead of
// throwing it, so a route that RETURNS the recovered error resolves to
// `undefined` with no error at all — a refusal a caller cannot see is
// worse than a confusing one. Batch ENTRY accessors never showed this,
// because their recovered error is folded into the entry's own `failure`
// slot rather than thrown.
//
// Mutation recipes:
// - Restore the bare `?` on `own_enumerable_keys` in
//   `decode_compile_requests_priority`: the `ownKeys` leg stops carrying
//   the thrown message and the following batch runs with an exception
//   already pending.
// - Drop the `throwable_status` re-tag in `recover_pending_exception`
//   (return `fallback.status`): BOTH legs resolve to `undefined`, so
//   `refusalOfRoute` answers null and both `accepted …` messages fire.
check("typed-batch-clears-a-throwing-options-trap-and-still-serves", () => {
  const canonicalId = "/typed/OptionsTrap.vue";
  const source = `<template><p>x</p></template>`;
  const host = new addon.VerterHost();
  const hostile = [
    [
      "ownKeys",
      new Proxy(
        {},
        {
          ownKeys() {
            throw new Error("options ownKeys trap");
          },
        },
      ),
      "options ownKeys trap",
    ],
    [
      "priority getter",
      {
        get priority() {
          throw new Error("options priority trap");
        },
      },
      "options priority trap",
    ],
  ];

  for (const [label, options, expected] of hostile) {
    const refusal = refusalOfRoute(() => host.compileRequests([], options));
    if (refusal === null) {
      host.close();
      return `accepted options whose ${label} throws, with no error at all`;
    }
    if (!refusal.includes(expected)) {
      host.close();
      return `throwing ${label} was refused with ${refusal}`;
    }
    // The next call runs on the same env: a still-pending exception would
    // fail it whatever it asked for.
    const entries = host.compileRequests([
      { canonicalId, source: Buffer.from(source), request: vueRuntime(canonicalId) },
    ]);
    if (entries.length !== 1 || entries[0].failure || !runtimeMain(entries[0].response)) {
      host.close();
      return `a later batch was poisoned by the throwing ${label}: ${JSON.stringify(entries)}`;
    }
  }
  host.close();
  return undefined;
});

check("typed-batch-refuses-a-non-array-input", () => {
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests({ length: 1 }));
  host.close();
  if (refusal === null) return "accepted a non-array batch input";
  if (!refusal.includes("must be an array")) {
    return `non-array batch refusal was ${refusal}`;
  }
  return undefined;
});

// The batch route's zip/wrap path pairs each response with its own input's
// source and output slot, which the singular route does not do, so a
// product kind proven on that route is not proven here. These two publish
// payloads the wrap has to carry through: a JSON analysis string and a
// multi-node server product.
check("typed-batch-runs-analysis-and-runtime-server-products", () => {
  const analysisId = "/typed/BatchAnalysis.vue";
  const serverId = "/typed/BatchServer.vue";
  const source = `<script setup>const n = 1</script><template><p>{{ n }}</p></template>`;
  const serverRequest = vueProducts(serverId, [{ kind: "runtimeServer", runtimeSourceMap: true }]);
  serverRequest.options.ssr = true;
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: analysisId,
      source: Buffer.from(source),
      request: vueProducts(analysisId, [
        { kind: "analysis", wantScriptBindings: false, wantTemplateData: true },
      ]),
    },
    {
      canonicalId: serverId,
      source: Buffer.from(source),
      request: serverRequest,
    },
  ]);
  host.close();
  if (entries.length !== 2 || entries[0].failure || entries[1].failure) {
    return `batch analysis/runtimeServer products did not both compile: ${JSON.stringify(entries)}`;
  }
  const analysisProduct = entries[0].response.products[0];
  if (!analysisProduct || analysisProduct.kind !== "analysis" || typeof analysisProduct.analysis !== "string") {
    return `batch analysis row was not a JSON string: ${JSON.stringify(analysisProduct)}`;
  }
  const serverNodes = runtimeNodes(entries[1].response, "runtimeServer");
  if (!serverNodes || serverNodes.length === 0) {
    return `batch runtimeServer nodes were absent: ${JSON.stringify(entries[1].response)}`;
  }
  return undefined;
});

// An empty `source` Buffer takes the one branch that cannot build a slice:
// `slice::from_raw_parts` requires a non-null aligned pointer even at
// length zero, and V8 is free to hand back a null data pointer for a
// zero-length backing store. Whether it does so on any given engine build
// is not something a test can pin, so this case exists to make the
// zero-length path EXECUTED rather than merely reasoned about — under a
// sanitizer or a debug allocator it is where the null slice would be
// caught, and in every build it proves an empty entry is answered rather
// than aborting the batch.
//
// Mutation recipe: register the empty entry alone (drop the sibling) and
// the case stops proving the batch survives it; restore the unconditional
// `from_raw_parts` and the branch is exercised with whatever pointer V8
// supplies.
check("typed-batch-answers-an-empty-source-buffer-beside-a-sibling", () => {
  const emptyId = "/typed/BatchEmpty.vue";
  const siblingId = "/typed/BatchAfterEmpty.vue";
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: emptyId,
      source: Buffer.alloc(0),
      request: vueRuntime(emptyId),
    },
    {
      canonicalId: siblingId,
      source: Buffer.from(`<template><p>after-empty</p></template>`),
      request: vueRuntime(siblingId),
    },
  ]);
  host.close();
  if (entries.length !== 2) {
    return `an empty source changed the entry count: ${JSON.stringify(entries)}`;
  }
  const empty = entries[0];
  if (empty.canonicalId !== emptyId) {
    return `the empty entry named ${empty.canonicalId}`;
  }
  if (Boolean(empty.response) === Boolean(empty.failure)) {
    return `the empty entry carried neither or both of response/failure: ${JSON.stringify(empty)}`;
  }
  if (entries[1].failure || !runtimeMain(entries[1].response)) {
    return `the sibling after an empty source did not compile: ${JSON.stringify(entries[1])}`;
  }
  return undefined;
});

// `ideCompanion` is the one product whose payload is computed FROM the
// entry's own source: the destructured-binding offsets are re-encoded to
// UTF-16 against it. The batch route zips each response back onto its
// input's source, so a zip that paired entry N's response with entry M's
// source would publish offsets that index the wrong file — silently, and
// only for this product. Two entries whose multi-byte prefixes have
// DIFFERENT byte lengths make that mispairing observable: each entry's
// binding offset can only be right against its own source.
//
// Mutation recipes:
// - Reverse `sources` before the zip in `compile_requests`
//   (`.zip(sources.into_iter().rev())`): both entries publish the other's
//   UTF-16 offsets and this case goes red on the first entry.
// - Convert the offsets as UTF-8 bytes (pass `OffsetEncoding::Utf8` to
//   `convert_destructured_block_meta` in `host_ide_to_napi`): both entries
//   report their byte offsets, which the assertions distinguish from the
//   UTF-16 ones by construction.
check("typed-batch-preserves-ide-utf16-offsets-per-entry", () => {
  // Different prefixes, so entry 0's UTF-16 offset for `greeting` is not
  // entry 1's, and neither equals its own byte offset.
  const entries = [
    { id: "/typed/BatchIdeA.vue", prefix: "// é" },
    { id: "/typed/BatchIdeB.vue", prefix: "// 😀😀😀 héllo —" },
  ].map(({ id, prefix }) => {
    const source = `<script setup lang="ts">
${prefix}
const greeting = "x"
const { title = greeting } = defineProps<{ title?: string }>()
</script>
<template><div>{{ title }}</div></template>`;
    return { id, source };
  });

  const utf16Of = (source) => source.indexOf("greeting");
  const byteOf = (source) => Buffer.byteLength(source.slice(0, utf16Of(source)));
  for (const { id, source } of entries) {
    if (byteOf(source) === utf16Of(source)) {
      return `${id} did not distinguish UTF-8 bytes from UTF-16 units`;
    }
  }
  if (utf16Of(entries[0].source) === utf16Of(entries[1].source)) {
    return "both entries share one UTF-16 offset, so a mispaired source would be invisible";
  }

  const host = new addon.VerterHost();
  const responses = host.compileRequests(
    entries.map(({ id, source }) => ({
      canonicalId: id,
      source: Buffer.from(source),
      request: vueProducts(id, [ideProduct()]),
    })),
  );
  host.close();

  if (responses.length !== entries.length) {
    return `batch returned ${responses.length} entries for ${entries.length} inputs`;
  }
  for (const [index, { id, source }] of entries.entries()) {
    const entry = responses[index];
    if (entry.failure || !entry.response) {
      return `${id} did not compile: ${JSON.stringify(entry)}`;
    }
    if (entry.canonicalId !== id) {
      return `entry ${index} named ${entry.canonicalId}, expected ${id}`;
    }
    const product = entry.response.products[0];
    const ide = product && product.kind === "ideCompanion" && product.ide;
    const published = ide && ide.destructuredBlock;
    const binding = published && published.bindings.find((row) => row.name === "greeting");
    if (!binding) return `${id} published no destructured binding: ${JSON.stringify(entry)}`;
    if (binding.sourceStart !== utf16Of(source)) {
      const other = entries[1 - index];
      const blamed =
        binding.sourceStart === utf16Of(other.source)
          ? ` (that is ${other.id}'s offset — the batch paired the wrong source)`
          : ` (byte offset is ${byteOf(source)})`;
      return `${id} binding sourceStart was ${binding.sourceStart}, expected ${utf16Of(source)}${blamed}`;
    }
    if (!(published.blockStart > source.length && published.blockStart <= ide.code.length)) {
      return `${id} generated blockStart ${published.blockStart} did not index IDE code of length ${ide.code.length}`;
    }
  }
  return undefined;
});

// The two products the compiler does not produce refuse per entry here, not
// for the whole batch: a caller that asked for one alongside compilable
// siblings still receives every sibling's output. The singular route throws
// for the same demand, so this is the batch route's own half of that contract.
check("typed-batch-isolates-public-api-and-declarations-refusals", () => {
  const validId = "/typed/BatchRefusalSibling.vue";
  const source = `<template><p>x</p></template>`;
  const host = new addon.VerterHost();
  const entries = host.compileRequests([
    {
      canonicalId: validId,
      source: Buffer.from(source),
      request: vueRuntime(validId),
    },
    ...["publicApi", "declarations"].map((kind) => ({
      canonicalId: `/typed/BatchUnsupported-${kind}.vue`,
      source: Buffer.from(source),
      request: vueProducts(`/typed/BatchUnsupported-${kind}.vue`, [{ kind }]),
    })),
  ]);
  host.close();
  if (entries.length !== 3) return `batch refusal changed the entry count: ${JSON.stringify(entries)}`;
  if (!entries[0].response || entries[0].failure || !runtimeMain(entries[0].response)) {
    return `sibling before the unsupported products did not compile: ${JSON.stringify(entries[0])}`;
  }
  for (const [offset, kind] of ["publicApi", "declarations"].entries()) {
    const entry = entries[offset + 1];
    if (entry.response || !entry.failure) {
      return `${kind} was not isolated as a failure: ${JSON.stringify(entry)}`;
    }
    if (entry.failure.kind !== "unsupportedProduct" || entry.failure.productKind !== kind) {
      return `${kind} failure was ${JSON.stringify({ kind: entry.failure.kind, productKind: entry.failure.productKind })}`;
    }
    if (entry.canonicalId !== `/typed/BatchUnsupported-${kind}.vue`) {
      return `${kind} failure named ${entry.canonicalId}`;
    }
  }
  return undefined;
});
"#;

#[test]
fn the_host_request_boundary_holds_for_real_js_values() {
    let library = fixture_addon_library();

    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("napi_request_fixture");
    std::fs::create_dir_all(&scratch).expect("the scratch directory is creatable");

    // Node loads an addon by extension, not by content.
    let addon = scratch.join("verter_napi_request_fixture.node");
    std::fs::copy(&library, &addon).expect("the built addon is copyable");
    let driver = scratch.join("drive.cjs");
    std::fs::write(&driver, DRIVER).expect("the driver is writable");

    let run = Command::new("node")
        .arg(&driver)
        .arg(&addon)
        .output()
        .expect("node is on PATH");

    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let transcript = format!("stdout:\n{stdout}\nstderr:\n{stderr}");

    assert!(
        run.status.success(),
        "the JS driver must run to completion; {transcript}"
    );

    for case in EXPECTED_CASES {
        assert!(
            stdout.contains(&format!("PASS {case}")),
            "`{case}` did not pass at the real JS boundary; {transcript}"
        );
    }
    assert!(
        !stdout.contains("FAIL "),
        "every case must pass; {transcript}"
    );
}

// @ai-generated - Host compile failures retain diagnostics and public UTF-16 offsets.
#[test]
fn typed_host_failure_preserves_diagnostics_and_utf16_offsets() {
    let failure = verter_session::CompileRequestFailure::Host(
        verter_session::HostError::CompileError(verter_session::CompileFailure {
            diagnostics: verter_session::DiagnosticsSnapshot {
                diagnostics: vec![verter_session::HostDiagnostic {
                    severity: verter_session::HostSeverity::Error,
                    code: "HOST_NATIVE_BINDING_UNAVAILABLE".to_string(),
                    message: "native binding unavailable".to_string(),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, 4),
                }],
                has_errors: true,
            },
            requested_mode: verter_session::CompileCacheMode::Stateless,
            actual_mode: verter_session::CompileCacheMode::Stateless,
            downgrade_reason: None,
        }),
    );

    let projected = verter_napi::compile_request_failure_to_napi(
        failure,
        "/src/Failure.vue".to_string(),
        Some("😀"),
    );

    assert_eq!(projected.diagnostics.diagnostics.len(), 1);
    let diagnostic = &projected.diagnostics.diagnostics[0];
    assert_eq!(diagnostic.severity, "error");
    assert_eq!(diagnostic.code, "HOST_NATIVE_BINDING_UNAVAILABLE");
    assert_eq!(diagnostic.message, "native binding unavailable");
    assert_eq!((diagnostic.spanStart, diagnostic.spanEnd), (0, 2));
    assert!(projected.diagnostics.hasErrors);
}

// @ai-generated - A warning must not become the terminal message when an error follows it.
#[test]
fn typed_failure_message_prefers_the_first_error_diagnostic() {
    let diagnostics = verter_session::DiagnosticsSnapshot {
        diagnostics: vec![
            verter_session::HostDiagnostic {
                severity: verter_session::HostSeverity::Warning,
                code: "W_FIRST".to_string(),
                message: "warning first".to_string(),
                arguments: Vec::new(),
                span: verter_span::Span::new(0, 0),
            },
            verter_session::HostDiagnostic {
                severity: verter_session::HostSeverity::Error,
                code: "E_SECOND".to_string(),
                message: "error second".to_string(),
                arguments: Vec::new(),
                span: verter_span::Span::new(0, 0),
            },
        ],
        has_errors: true,
    };
    let failure = verter_session::CompileRequestFailure::ProductNotProduced {
        canonical_id: "/src/Failure.vue".to_string(),
        kind: verter_compiler::compile_request::ProductKind::RuntimeClient,
        diagnostics,
    };

    let projected = verter_napi::compile_request_failure_to_napi(
        failure,
        "/src/Failure.vue".to_string(),
        Some(""),
    );

    assert_eq!(projected.message, "error second");
}
