/**
 * STP7 second-framework / heterogeneous-context boundary proof helpers.
 *
 * Qualifies shared origin/binder/correspondence against a Svelte 5 public
 * Component, two-way hole mapping, and server/client same-program ambient
 * rules. Does not implement a production Svelte, Astro, MDX, or Lit product.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STP7_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP7_DIR, "../../..");

export const PROTOCOL_VERSION = 1;

export const VUE_ONLY_REQUIRED = Object.freeze([
  "vueConstructor",
  "vueRef",
  "vueDirective",
  "vueEmit",
]);

export const VUE_ONLY_SOURCE = Object.freeze([
  "defineComponent",
  "defineProps",
  "defineEmits",
  "v-model",
  "v-for",
  "v-if",
  "$emit",
  "vue.ref",
  "InstanceType<typeof import(",
]);

export const SHARED_ORIGIN_FILES = Object.freeze([
  "crates/verter_language/src/ids.rs",
  "crates/verter_language/src/lib.rs",
  "crates/verter_compiler/src/framework_common/generated_chunk.rs",
  "crates/verter_compiler/src/framework_common/sourcemap_e2e_helpers.rs",
  "crates/verter_compiler/src/svelte/svelte_projection_backend.rs",
]);

export const RECEIVING_OBLIGATION_IDS = Object.freeze(["ASTP", "MDXP", "LITP", "SvelteKit"]);

export const CLEAN_SHARED = Object.freeze({
  adapter: "svelte",
  publicShape: 'import("svelte").Component',
  vueConstructor: false,
  vueRef: false,
  vueDirective: false,
  vueEmit: false,
});

export const DIRTY_REUSE = Object.freeze({
  adapter: "svelte",
  publicShape: "vue-constructor",
  vueConstructor: true,
  vueRef: true,
  vueDirective: true,
  vueEmit: true,
});

export const CLEAN_HOLES = Object.freeze({
  original:
    "Hello {user.name}!\n{#each items as item}{item.label}{/each}\n{#snippet greet(who)}Hi {who}{/snippet}",
  generated:
    "Hello {user.name}!\n{items.map((item) => item.label)}\nfunction greet(who) { return `Hi ${who}`; }",
  holes: Object.freeze([
    { name: "user.name", authored: "user.name" },
    { name: "item.label", authored: "item.label" },
    { name: "who", authored: "who" },
  ]),
});

export const DIRTY_HOLES = Object.freeze({
  original: CLEAN_HOLES.original,
  generated:
    "Hello {__s1}!\n{items.map((item) => __s2)}\nfunction greet(__s3) { return `Hi ${__s3}`; }",
  holes: CLEAN_HOLES.holes,
});

export const CLEAN_REALM = Object.freeze({
  files: ["server.generated.ts", "client.generated.ts"],
  sameProgram: true,
  claimsAmbientIsolation: false,
  claimsCompilerOptionIsolation: false,
});

export const DIRTY_REALM = Object.freeze({
  files: ["server.generated.ts", "client.generated.ts"],
  sameProgram: true,
  claimsAmbientIsolation: true,
  claimsCompilerOptionIsolation: true,
});

export const CLEAN_SCOPE = Object.freeze({
  fullFrameworkSupportClaim: false,
  receivingObligations: Object.freeze(
    RECEIVING_OBLIGATION_IDS.map((id) =>
      Object.freeze({ id, role: "reusable-substrate", certifiedProduct: false }),
    ),
  ),
});

export const DIRTY_SCOPE = Object.freeze({
  fullFrameworkSupportClaim: true,
  receivingObligations: Object.freeze(
    ["Astro", "MDX", "Lit"].map((id) =>
      Object.freeze({ id, role: "full-support", certifiedProduct: true }),
    ),
  ),
});

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function repoPath(rel) {
  return path.resolve(REPO_ROOT, rel);
}

export function loadStp7Product(name) {
  return JSON.parse(fs.readFileSync(path.join(STP7_DIR, "products", name), "utf8"));
}

function tokenIndex(haystack, token, from = 0) {
  return haystack.indexOf(token, from);
}

export function assertSvelteShape(source) {
  const errors = [];
  if (!source || typeof source !== "string") {
    return [err("STP7-svelte-shape", "missing-fixture", "Svelte shape fixture is missing")];
  }
  if (!source.includes("Svelte5Component") && !source.includes('import("svelte").Component')) {
    errors.push(
      err(
        "STP7-svelte-shape",
        "vue-constructor-shim",
        "Svelte public shape must be a Svelte 5 Component, not a Vue constructor",
      ),
    );
  }
  if (
    /\bexport\s+class\b/.test(source) ||
    /\bInstanceType\s*<\s*typeof/.test(source) ||
    /\bdefineComponent\b/.test(source)
  ) {
    errors.push(
      err(
        "STP7-svelte-shape",
        "vue-constructor-shim",
        "Svelte shape fixture must not encode a Vue constructor shim",
      ),
    );
  }
  for (const needle of ["eachScope", "Snippet", "Widget"]) {
    if (!source.includes(needle)) {
      errors.push(
        err("STP7-svelte-shape", "missing-fixture", `Svelte shape fixture missing ${needle}`),
      );
    }
  }
  return errors;
}

export function validateSharedRecord(record) {
  const errors = [];
  if (!record || record.adapter !== "svelte") {
    return [err("STP7-reuse", "missing-adapter", "shared record must name the Svelte adapter")];
  }
  for (const field of VUE_ONLY_REQUIRED) {
    if (record[field] === true) {
      errors.push(
        err(
          "STP7-reuse",
          "vue-only-required",
          `shared record requires Vue-only ${field} for Svelte`,
        ),
      );
    }
  }
  if (record.publicShape === "vue-constructor") {
    errors.push(
      err(
        "STP7-reuse",
        "vue-only-required",
        "shared record requires a Vue constructor as the Svelte public shape",
      ),
    );
  }
  return errors;
}

export function scanSharedOrigins(files = SHARED_ORIGIN_FILES) {
  const errors = [];
  for (const rel of files) {
    const abs = repoPath(rel);
    if (!fs.existsSync(abs)) {
      errors.push(err("STP7-reuse", "missing-origin-file", `missing shared origin file ${rel}`));
      continue;
    }
    const text = fs.readFileSync(abs, "utf8");
    for (const token of VUE_ONLY_SOURCE) {
      if (text.includes(token)) {
        errors.push(
          err(
            "STP7-reuse",
            "vue-only-required",
            `${rel} encodes Vue-only ${token} in shared origin/binder/correspondence`,
          ),
        );
      }
    }
  }
  const idsAbs = repoPath("crates/verter_language/src/ids.rs");
  if (fs.existsSync(idsAbs)) {
    const ids = fs.readFileSync(idsAbs, "utf8");
    if (!ids.includes("fn vue()") || !ids.includes("fn svelte()")) {
      errors.push(
        err(
          "STP7-svelte-shape",
          "missing-adapter-id",
          "FrameworkAdapterId must intern vue() and svelte() as distinct ids",
        ),
      );
    }
  }
  const backendAbs = repoPath("crates/verter_compiler/src/svelte/svelte_projection_backend.rs");
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("SvelteProjectionBackend")) {
      errors.push(
        err("STP7-svelte-shape", "missing-adapter", "Svelte ProjectionBackend adapter is missing"),
      );
    }
    if (backend.includes("FrameworkAdapterId::vue()")) {
      errors.push(
        err(
          "STP7-reuse",
          "vue-only-required",
          "Svelte ProjectionBackend must not bind FrameworkAdapterId::vue()",
        ),
      );
    }
  }
  const chunkAbs = repoPath("crates/verter_compiler/src/framework_common/generated_chunk.rs");
  if (fs.existsSync(chunkAbs)) {
    const chunk = fs.readFileSync(chunkAbs, "utf8");
    if (!chunk.includes("compose_generated_chunk")) {
      errors.push(
        err(
          "STP7-holes",
          "missing-hole-substrate",
          "generated_chunk must expose compose_generated_chunk for hole correspondence",
        ),
      );
    }
  }
  return errors;
}

function holeOccurrence(text, authored, skip) {
  let from = 0;
  for (let i = 0; i <= skip; i += 1) {
    const at = tokenIndex(text, authored, from);
    if (at < 0) return -1;
    if (i === skip) return at;
    from = at + authored.length;
  }
  return -1;
}

export function assertTwoWayHoles(fixture) {
  const errors = [];
  if (!fixture?.original || !fixture?.generated || !Array.isArray(fixture.holes)) {
    return [
      err("STP7-holes", "missing-holes", "hole fixture is missing original/generated ranges"),
    ];
  }
  for (const hole of fixture.holes) {
    const authored = hole?.authored;
    if (!authored) {
      errors.push(err("STP7-holes", "missing-holes", "hole missing authored expression"));
      continue;
    }
    const originalAt = tokenIndex(fixture.original, authored);
    const generatedAt = tokenIndex(fixture.generated, authored);
    if (originalAt < 0 || generatedAt < 0) {
      errors.push(
        err(
          "STP7-holes",
          "one-way-hole",
          `authored expression ${JSON.stringify(authored)} does not survive both sides of the hole`,
        ),
      );
      continue;
    }
    const originalSlice = fixture.original.slice(originalAt, originalAt + authored.length);
    const generatedSlice = fixture.generated.slice(generatedAt, generatedAt + authored.length);
    if (originalSlice !== authored || generatedSlice !== authored) {
      errors.push(
        err(
          "STP7-holes",
          "one-way-hole",
          `hole ${JSON.stringify(authored)} lost two-way identity original=${JSON.stringify(originalSlice)} generated=${JSON.stringify(generatedSlice)}`,
        ),
      );
    }
    const back = holeOccurrence(fixture.original, authored, 0);
    if (back !== originalAt) {
      errors.push(
        err(
          "STP7-holes",
          "one-way-hole",
          `inverse map for ${JSON.stringify(authored)} did not return the original range`,
        ),
      );
    }
  }
  if (!fixture.original.includes("{") || !fixture.generated.includes("{")) {
    errors.push(
      err("STP7-holes", "missing-holes", "hole fixture must retain literal hole delimiters"),
    );
  }
  return errors;
}

export function validateRealmClaim(record) {
  if (!record || !Array.isArray(record.files) || record.files.length < 2) {
    return [
      err("STP7-realm", "missing-files", "realm record must name separate supplemental files"),
    ];
  }
  if (record.sameProgram && record.claimsAmbientIsolation) {
    return [
      err(
        "STP7-realm",
        "false-isolation",
        "separate generated files in one TS program do not isolate ambient globals",
      ),
    ];
  }
  if (record.sameProgram && record.claimsCompilerOptionIsolation) {
    return [
      err(
        "STP7-realm",
        "false-isolation",
        "separate generated files in one TS program do not isolate compiler options",
      ),
    ];
  }
  return [];
}

export function validateScopeClaim(record) {
  const errors = [];
  if (!record) {
    return [err("STP7-scope-claim", "missing-scope", "scope claim record is missing")];
  }
  if (record.fullFrameworkSupportClaim === true) {
    errors.push(
      err(
        "STP7-scope-claim",
        "full-support-claim",
        "architecture proof must not advertise full Astro/MDX/Lit support",
      ),
    );
  }
  const rows = record.receivingObligations || [];
  const ids = new Set(rows.map((row) => row.id));
  for (const id of RECEIVING_OBLIGATION_IDS) {
    if (!ids.has(id)) {
      errors.push(
        err("STP7-scope-claim", "missing-obligation", `receiving obligation ${id} is missing`),
      );
    }
  }
  for (const row of rows) {
    if (row.certifiedProduct === true || row.role === "full-support") {
      errors.push(
        err(
          "STP7-scope-claim",
          "full-support-claim",
          `${row.id} receiving obligation must not certify a complete framework product`,
        ),
      );
    }
  }
  return errors;
}

export function assertRealmLeak(ts, serverPath, clientPath) {
  const errors = [];
  const options = {
    strict: true,
    noEmit: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    skipLibCheck: true,
    types: [],
  };
  const host = ts.createCompilerHost(options, true);
  const program = ts.createProgram({
    rootNames: [serverPath, clientPath],
    options,
    host,
  });
  const diags = ts.getPreEmitDiagnostics(program);
  const clientAbs = path.resolve(clientPath);
  const clientCannotFind = diags.filter((diag) => {
    const file = diag.file && path.resolve(diag.file.fileName);
    return file === clientAbs && diag.code === 2304;
  });
  if (clientCannotFind.length > 0) {
    errors.push(
      err(
        "STP7-realm",
        "false-isolation",
        "client file did not observe the server ambient; same-program leak was not demonstrated",
      ),
    );
  }
  return errors;
}

export function validateStp7Products({
  evidence = loadStp7Product("second-framework-boundary-evidence.json"),
  contract = loadStp7Product("execution-context-boundary-contract.json"),
  evidenceText = fs.readFileSync(path.join(STP7_DIR, "../evidence/STP7/cases.md"), "utf8"),
} = {}) {
  const errors = [];
  if (evidence?.schema !== "SecondFrameworkBoundaryEvidence") {
    errors.push(
      err("STP7-svelte-shape", "removed-fixture", "SecondFrameworkBoundaryEvidence schema"),
    );
  }
  if (contract?.schema !== "ExecutionContextBoundaryContract") {
    errors.push(err("STP7-holes", "removed-fixture", "ExecutionContextBoundaryContract schema"));
  }
  if (evidence?.adapter?.vueConstructorShim !== false) {
    errors.push(
      err(
        "STP7-svelte-shape",
        "vue-constructor-shim",
        "SecondFrameworkBoundaryEvidence must record vueConstructorShim=false",
      ),
    );
  }
  if (evidence?.adapter?.publicShape !== 'import("svelte").Component') {
    errors.push(
      err(
        "STP7-svelte-shape",
        "vue-constructor-shim",
        'Svelte public shape must be import("svelte").Component',
      ),
    );
  }
  if (evidence?.sharedOrigins?.adapterIdsDistinct !== true) {
    errors.push(err("STP7-svelte-shape", "missing-adapter-id", "adapter ids must stay distinct"));
  }
  for (const field of VUE_ONLY_REQUIRED) {
    if (evidence?.sharedOrigins?.[field] !== false) {
      errors.push(err("STP7-reuse", "vue-only-required", `sharedOrigins.${field} must be false`));
    }
  }
  const evidenceIds = new Set((evidence?.cases || []).map((row) => row.id));
  for (const id of [
    "STP7-svelte-shape",
    "STP7-holes",
    "STP7-realm",
    "STP7-reuse",
    "STP7-scope-claim",
  ]) {
    if (!evidenceIds.has(id)) {
      errors.push(err(id, "removed-fixture", `boundary evidence missing ${id}`));
    }
  }
  errors.push(
    ...validateScopeClaim({
      fullFrameworkSupportClaim: evidence?.fullFrameworkSupportClaim,
      receivingObligations: evidence?.receivingObligations,
    }),
  );
  if (contract?.sameProgram?.separateSupplementalFilesIsolateAmbient !== false) {
    errors.push(
      err(
        "STP7-realm",
        "false-isolation",
        "ExecutionContextBoundaryContract must not claim ambient isolation from separate files",
      ),
    );
  }
  if (contract?.sameProgram?.sharedAmbientGlobals !== true) {
    errors.push(
      err("STP7-realm", "false-isolation", "same-program ambient globals must be recorded"),
    );
  }
  if (contract?.holes?.twoWayMapping !== true) {
    errors.push(err("STP7-holes", "one-way-hole", "holes must record two-way mapping"));
  }
  if (contract?.newFrontends !== false) {
    errors.push(
      err("STP7-scope-claim", "full-support-claim", "STP7 must not add Astro/MDX/Lit frontends"),
    );
  }
  if (!String(evidence?.ac3Rationale || "").trim()) {
    errors.push(
      err("STP7-svelte-shape", "removed-fixture", "missing AC3 untouched-owner rationale"),
    );
  }
  if (!String(evidence?.ac4Rationale || "").trim()) {
    errors.push(
      err("STP7-svelte-shape", "removed-fixture", "missing AC4 untouched-owner rationale"),
    );
  }
  if (!/[0-9a-f]{40}/.test(evidenceText)) {
    errors.push(
      err(
        "STP7-svelte-shape",
        "missing-source-revision",
        "STP7 evidence must record a 40-character source revision",
      ),
    );
  }
  if (!/6\.0\.3/.test(evidenceText) || !/7\.0\.2/.test(evidenceText)) {
    errors.push(
      err(
        "STP7-svelte-shape",
        "missing-engine-pins",
        "STP7 evidence must record ts-js 6.0.3 and ts-native 7.0.2 engine pins",
      ),
    );
  }
  return errors;
}

export function evaluateRejectTwins() {
  const errors = [];
  const cleanReuse = validateSharedRecord(CLEAN_SHARED);
  if (cleanReuse.length) errors.push(...cleanReuse);
  const dirtyReuse = validateSharedRecord(DIRTY_REUSE);
  if (dirtyReuse.length === 0) {
    errors.push(
      err("STP7-reuse", "missed-reuse", "Vue-only shared record dirty twin was not rejected"),
    );
  }
  const cleanHoles = assertTwoWayHoles(CLEAN_HOLES);
  if (cleanHoles.length) errors.push(...cleanHoles);
  const dirtyHoles = assertTwoWayHoles(DIRTY_HOLES);
  if (dirtyHoles.length === 0) {
    errors.push(
      err("STP7-holes", "missed-one-way-hole", "one-way hole dirty twin was not rejected"),
    );
  }
  const cleanRealm = validateRealmClaim(CLEAN_REALM);
  if (cleanRealm.length) errors.push(...cleanRealm);
  const dirtyRealm = validateRealmClaim(DIRTY_REALM);
  if (dirtyRealm.length === 0) {
    errors.push(
      err(
        "STP7-realm",
        "missed-isolation-claim",
        "same-program isolation dirty twin was not rejected",
      ),
    );
  }
  const cleanScope = validateScopeClaim(CLEAN_SCOPE);
  if (cleanScope.length) errors.push(...cleanScope);
  const dirtyScope = validateScopeClaim(DIRTY_SCOPE);
  if (dirtyScope.length === 0) {
    errors.push(
      err(
        "STP7-scope-claim",
        "missed-full-support",
        "full Astro/MDX/Lit support dirty twin was not rejected",
      ),
    );
  }
  return errors;
}

export async function evaluateStp7(input = {}) {
  const errors = [];
  errors.push(...validateStp7Products());
  const positiveAbs = path.join(STP7_DIR, "probes", "positive.ts");
  errors.push(...assertSvelteShape(fs.readFileSync(positiveAbs, "utf8")));
  errors.push(...scanSharedOrigins());
  errors.push(...assertTwoWayHoles(CLEAN_HOLES));
  errors.push(...evaluateRejectTwins());
  if (input.ts) {
    errors.push(
      ...assertRealmLeak(
        input.ts,
        path.join(STP7_DIR, "probes", "contexts", "server.ts"),
        path.join(STP7_DIR, "probes", "contexts", "client.ts"),
      ),
    );
  }
  return { errors };
}
