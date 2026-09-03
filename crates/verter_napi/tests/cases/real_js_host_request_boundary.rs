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
    "typed-single-refuses-an-own-unknown-key",
    "typed-single-throws-a-typed-framework-failure",
    "typed-batch-isolates-a-request-decode-refusal",
    "typed-batch-isolates-a-canonical-request-refusal",
    "typed-batch-isolates-malformed-entry-fields",
    "typed-batch-isolates-invalid-canonical-ids",
    "typed-batch-clears-throwing-accessors-and-preserves-siblings",
    "typed-batch-preserves-order-isolates-failure-and-registers-once",
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

function legacyRuntimeMain(canonicalId, source, fileKind) {
  const host = new addon.VerterHost();
  upsertSource(host, canonicalId, source, fileKind);
  const response = host.getVirtualFile({
    canonicalId,
    nodeKind: { kind: "main" },
    compileProfile: {
      filename: canonicalId,
      isProduction: false,
      ssr: false,
      componentId: "typed-route",
      hmrStrategy: "none",
      forceJs: false,
      sourceMap: true,
      target: "bundler",
    },
  });
  host.close();
  return response;
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
  const legacy = legacyRuntimeMain(canonicalId, source, "vue");
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
    main.code !== legacy.code ||
    main.sourceMap !== legacy.sourceMap ||
    JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)
  ) {
    return `typed Vue output diverged from the profile route: ${JSON.stringify({ main, legacy })}`;
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
  const legacy = legacyRuntimeMain(canonicalId, source, "svelte");
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
    main.code !== legacy.code ||
    main.sourceMap !== legacy.sourceMap ||
    JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)
  ) {
    return `typed Svelte output diverged from the profile route: ${JSON.stringify({ main, legacy })}`;
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
  const legacy = legacyRuntimeMain("/typed/Warn.vue", multibyte, "vue");
  if (!legacy || JSON.stringify(response.diagnostics) !== JSON.stringify(legacy.diagnostics)) {
    return `typed diagnostics diverged from the profile route: ${JSON.stringify({ typed: response.diagnostics, legacy: legacy && legacy.diagnostics })}`;
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
  if (!error.diagnostics || error.diagnostics.hasErrors !== true || !Array.isArray(error.diagnostics.diagnostics)) {
    return `thrown failure lost diagnostics: ${JSON.stringify(error.diagnostics)}`;
  }
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
  if (!entries[1].failure.message.includes("DuplicateProduct(RuntimeClient)")) {
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
  const sharedRequest = vueRuntime("/typed/SharedRequest.vue");
  sharedRequest.options.isCustomElement = new Array(32768).fill("x-element");
  const inputs = Array.from({ length: 9 }, (_, index) => ({
    canonicalId: `/typed/Values-${index}.vue`,
    source: Buffer.from(`<template><x-element /></template>`),
    request: sharedRequest,
  }));
  const host = new addon.VerterHost();
  const refusal = refusalOfRoute(() => host.compileRequests(inputs));
  host.close();
  if (refusal === null) return "accepted an aggregate request graph above the decode budget";
  if (!refusal.includes("decoded values")) return `aggregate decode refusal was ${refusal}`;
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
