/**
 * STP19 Advanced generic binders and external component interoperability
 * protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 *
 * - the owning Rust suites compile the crate and execute over real template
 *   bytes and admitted carrier blocks per mandatory case;
 * - the anchored negative probes run on every resolved engine: each
 *   construction that violates a component's exact contract reports one
 *   TS2769 at the offending authored member, and each explicit argument that
 *   violates the authored binder reports one TS2344 at that argument;
 * - every Rust dirty twin is applied as a source patch against the owned
 *   product and must make its discriminator tests FAIL;
 * - every TypeScript dirty twin is applied to a copy of a pinned probe (the
 *   probes are the product's rendering byte for byte) and must change the
 *   engine verdict it names on every resolved engine.
 *
 * Rejections are reproduced, never asserted from clean passes.
 *
 * Engine execution is injected by the caller as `runEngine(engine, probes)`
 * (verify-node owns the engine runners). The protocol never imports
 * verify-node.mjs: that module is the CLI entry, and a static import of the
 * still-evaluating entry from a dynamically imported protocol never settles.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCT_RS = "crates/verter_compiler/src/ide/vue_projection/generic_interop.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const PROBES = "tests/sfc-projection/STP19/probes";
const LIB_FILTER = "ide::vue_projection::generic_interop";
const CARRIER_TEST = "advanced_generic_uses_reads_admitted_carrier_blocks";

export const STP19_MANDATORY_CASES = Object.freeze([
  "STP19-explicit",
  "STP19-higher-rank",
  "STP19-forward",
  "STP19-overloads",
  "STP19-foreign",
  "STP19-erasure",
  "STP19-instantiation-alias",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "AdvancedGenericUseProjection",
  "ForeignComponentContractAdapter",
]);

// Rejected mutations (never applied to the landed tree; the clean product
// must fail each one).
export const DIRTY_FIXED_SIGNATURE_ADAPTER = { callee: { exactContract: "dropped" } };
export const DIRTY_OPEN_ARGS_WIDENED = { contract: { openArguments: "passthrough" } };
export const DIRTY_SCOPE_DROPS_BINDER = { scope: { binder: "dropped" } };
export const DIRTY_FABRICATED_WITNESS = { availability: { unavailable: "fabricated" } };
export const DIRTY_ADAPTER_ANY = { contract: { callable: "any" } };
export const DIRTY_ALIAS_ERASED = { callee: { alias: "base-component" } };
export const DIRTY_OPEN_CATCH_ALL_REPLACES_OVERLOADS = { contract: { openArguments: "last-only" } };
export const DIRTY_CALL_OVERLOADS_COLLAPSE = { contract: { callable: "last-only" } };
export const DIRTY_CALL_OVERLOADS_INDEXED = { contract: { callable: "tolerant-overloads" } };
export const DIRTY_GENERIC_STALL_REOPENS_CATCH_ALL = {
  contract: { openArguments: "declared-on-generic" },
};
export const DIRTY_GENERIC_STALL_DROPS_READ_OVERLOADS = {
  contract: { callable: "last-only-on-generic" },
};

const RUST_CASES = Object.freeze({
  "STP19-explicit": [
    "generic_use_scope_carries_the_authored_binder",
    "picker_probe_fixture_is_the_rendered_declaration",
    "generic_use_witness_values_keep_their_authored_origin",
  ],
  "STP19-higher-rank": ["generic_use_explicit_aliases_construct_the_alias_itself"],
  "STP19-forward": ["generic_use_scope_carries_the_authored_binder"],
  "STP19-overloads": [
    "generic_use_construction_applies_the_exact_contract_first",
    "probe_fixtures_are_the_rendered_products",
  ],
  "STP19-foreign": ["foreign_contract_declarations_keep_published_shapes"],
  "STP19-erasure": [
    "generic_use_availability_keeps_unavailable_uses_unwitnessed",
    "foreign_contract_declarations_keep_published_shapes",
  ],
  "STP19-instantiation-alias": [
    "generic_use_explicit_aliases_construct_the_alias_itself",
    "generic_use_products_are_deterministic_and_invalid_binders_refuse",
  ],
});

// Each Rust dirty twin is a source patch against the owned product plus the
// discriminator tests that must FAIL while it is applied: lib discriminators
// run in the unit lane, the carrier test in the production `--test main`
// lane.
const DIRTY_TWIN_PATCHES = Object.freeze({
  DIRTY_FIXED_SIGNATURE_ADAPTER: Object.freeze({
    caseId: "STP19-overloads",
    find: 'format!("{USE_COMPONENT}({component}, {USE_CONSTRUCTOR}({component}))")',
    replace: 'format!("{USE_CONSTRUCTOR}({component})")',
    discriminators: Object.freeze([
      "generic_use_construction_applies_the_exact_contract_first",
      "probe_fixtures_are_the_rendered_products",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_OPEN_ARGS_WIDENED: Object.freeze({
    caseId: "STP19-foreign",
    find: "(I extends { readonly $props: infer P } ? new (props: P) => I : new (...args: A) => I)",
    replace: "new (...args: A) => I",
    discriminators: Object.freeze([
      "foreign_contract_declarations_keep_published_shapes",
      "probe_fixtures_are_the_rendered_products",
    ]),
  }),
  DIRTY_SCOPE_DROPS_BINDER: Object.freeze({
    caseId: "STP19-forward",
    find: "self.scope_binder()\n",
    replace: "String::new()\n",
    discriminators: Object.freeze([
      "generic_use_scope_carries_the_authored_binder",
      "probe_fixtures_are_the_rendered_products",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_FABRICATED_WITNESS: Object.freeze({
    caseId: "STP19-erasure",
    find: "None => UseContractAvailability::Unavailable,",
    replace:
      'None => UseContractAvailability::Witnessed { binding: String::from("__VerterUse_unavailable") },',
    discriminators: Object.freeze([
      "generic_use_availability_keeps_unavailable_uses_unwitnessed",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_ADAPTER_ANY: Object.freeze({
    caseId: "STP19-erasure",
    find: ": C) : __VerterUseCalls<C, unknown, never, []>;",
    replace: ": C) : any;",
    discriminators: Object.freeze(["foreign_contract_declarations_keep_published_shapes"]),
  }),
});

// Each TypeScript dirty twin rewrites a copy of one pinned probe and names
// the verdict every engine must change: a clean probe gains diagnostics, or
// an anchored diagnostic of a negative probe disappears.
const TS_TWINS = Object.freeze({
  DIRTY_FIXED_SIGNATURE_ADAPTER: Object.freeze({
    caseId: "STP19-overloads",
    probe: "positive.ts",
    rewrite: (text) =>
      text.replace(
        /__VerterUseComponent\(([\w.]+), __VerterUseConstructor\(\1\)\)/g,
        "__VerterUseConstructor($1)",
      ),
    expect: { gains: 'new (__VerterUseConstructor(Shape))({ "kind": "circle"' },
  }),
  DIRTY_OPEN_ARGS_WIDENED: Object.freeze({
    caseId: "STP19-foreign",
    probe: "negative-construction.ts",
    rewrite: (text) =>
      text
        .replace(
          "(I extends { readonly $props: infer P } ? new (props: P) => I : new (...args: A) => I)",
          "new (...args: A) => I",
        )
        .replace("(0 extends 1 & P ? (I extends { readonly $props: infer Q } ? Q : P) : P)", "P"),
    expect: { loses: "\"count\": ('1')" },
  }),
  DIRTY_ADAPTER_ANY: Object.freeze({
    caseId: "STP19-erasure",
    probe: "negative.ts",
    rewrite: (text) =>
      text.replace(": C) : __VerterUseCalls<C, unknown, never, []>;", ": C) : any;"),
    expect: { loses: '"level": (4)' },
  }),
  // The open catch-all replaces the whole constructor with its own `$props`
  // signature: an earlier precise overload is no longer selectable.
  DIRTY_OPEN_CATCH_ALL_REPLACES_OVERLOADS: Object.freeze({
    caseId: "STP19-overloads",
    probe: "positive.ts",
    rewrite: (text) =>
      text.replace(
        "__VerterUseConstructs<C, unknown, never, []>",
        "(C extends abstract new (...args: infer _) => infer I ? (I extends { readonly $props: infer P } ? new (props: P) => I : C) : C)",
      ),
    expect: { gains: '"unit": "celsius"' },
  }),
  // An overloaded callable is matched to one call signature: only its last
  // overload is selectable.
  DIRTY_CALL_OVERLOADS_COLLAPSE: Object.freeze({
    caseId: "STP19-overloads",
    probe: "positive.ts",
    rewrite: (text) =>
      text.replace(": C) : __VerterUseCalls<C, unknown, never, []>;", ": C) : unknown;"),
    expect: { gains: '"mode": "on"' },
  }),
  // Every rebuilt call overload tolerates unknown attributes: an earlier
  // overload absorbs the prop that selects a later one.
  DIRTY_CALL_OVERLOADS_INDEXED: Object.freeze({
    caseId: "STP19-overloads",
    probe: "positive.ts",
    rewrite: (text) =>
      text.replace(
        "new (props: P) => __VerterUseFunctional<P, X extends",
        "new (props: P & Record<string, unknown>) => __VerterUseFunctional<P, X extends",
      ),
    expect: { gains: ".$props.flag;" },
  }),
  // A generic construct signature stops the rebuild and the declared
  // constructor returns: its open catch-all accepts any props again.
  DIRTY_GENERIC_STALL_REOPENS_CATCH_ALL: Object.freeze({
    caseId: "STP19-overloads",
    probe: "negative-construction.ts",
    rewrite: (text) =>
      text.replace(
        "__VerterUseSame<[A, I], Prev> extends true ? __VerterUseOrdered<Out, unknown> :",
        "__VerterUseSame<[A, I], Prev> extends true ? C :",
      ),
    expect: { loses: '"label": (1)' },
  }),
  // A generic call signature stops the rebuild and every overload already
  // read is discarded: only the last call signature stays selectable.
  DIRTY_GENERIC_STALL_DROPS_READ_OVERLOADS: Object.freeze({
    caseId: "STP19-overloads",
    probe: "positive.ts",
    rewrite: (text) =>
      text.replace(
        "__VerterUseSame<[A, R], Prev> extends true ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) :",
        "__VerterUseSame<[A, R], Prev> extends true ? unknown :",
      ),
    expect: { gains: '"kind": "g", "value": "v"' },
  }),
  DIRTY_ALIAS_ERASED: Object.freeze({
    caseId: "STP19-instantiation-alias",
    probe: "negative-construction.ts",
    rewrite: (text) =>
      text.replace(
        "__VerterUseComponent(BarrelPicker, __VerterUseConstructor(BarrelPicker))",
        "__VerterUseComponent(Picker, __VerterUseConstructor(Picker))",
      ),
    expect: { loses: '"field": "label"' },
  }),
});

// Construction violations: each anchored line carries exactly one TS2769 at
// an authored member. With more than three candidate signatures TypeScript
// reports its last candidate's failure, exactly as it does for the same
// props passed to `new Shape(...)` directly, so the six-overload use anchors
// at the member that last candidate rejects.
const CONSTRUCTION_ANCHORS = Object.freeze([
  { caseId: "STP19-forward", line: '"field": "missing"', at: '"field"' },
  { caseId: "STP19-instantiation-alias", line: '"field": "label"', at: '"field"' },
  { caseId: "STP19-foreign", line: "\"count\": ('1')", at: '"count"' },
  { caseId: "STP19-overloads", line: "\"sides\": ('five')", at: '"kind"' },
  { caseId: "STP19-overloads", line: '"label": (1)', at: '"label"' },
]);

// Explicit-argument violations: each line marked `// violates` carries
// exactly one TS2344 at the authored argument.
const CONSTRAINT_ANCHORS = Object.freeze([
  { caseId: "STP19-explicit", line: "export const TextId", at: "{ id: string }" },
  { caseId: "STP19-explicit", line: "export type NumberInstance", at: "number>" },
  { caseId: "STP19-explicit", line: "export const WrongField", at: '"name">' },
]);

export function dirtyTwinExpectations() {
  return Object.fromEntries(
    Object.entries(DIRTY_TWIN_PATCHES).map(([twin, spec]) => [twin, [...spec.discriminators]]),
  );
}

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function validateStp19Products({ repoRoot = REPO_ROOT } = {}) {
  // Structural inventory only: every behavior claim below is proven by the
  // fresh cargo and engine runs, never by matching source text.
  const errors = [];
  for (const rel of [
    PRODUCT_RS,
    BACKEND_RS,
    "tests/sfc-projection/STP19/manifest.json",
    `${PROBES}/positive.ts`,
    `${PROBES}/negative.ts`,
    `${PROBES}/negative-construction.ts`,
    `${PROBES}/negative-constraint.ts`,
    `${PROBES}/foreign.ts`,
    `${PROBES}/aliases.ts`,
    `${PROBES}/barrel.ts`,
    `${PROBES}/components/Picker.vue.ts`,
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP19-explicit", "removed-fixture", `missing ${rel}`));
    }
  }
  const manifestAbs = path.join(repoRoot, "tests/sfc-projection/STP19/manifest.json");
  if (fs.existsSync(manifestAbs)) {
    let manifest = null;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8"));
    } catch {
      errors.push(err("STP19-explicit", "removed-fixture", "manifest.json is not valid JSON"));
    }
    if (manifest !== null) {
      const mandatoryCases = Array.isArray(manifest.mandatoryCases) ? manifest.mandatoryCases : [];
      const products = Array.isArray(manifest.products) ? manifest.products : [];
      for (const id of STP19_MANDATORY_CASES) {
        if (!mandatoryCases.includes(id)) {
          errors.push(err(id, "unknown-row", `manifest is missing mandatory case ${id}`));
        }
      }
      for (const product of ACCEPTED_PRODUCTS) {
        if (!products.includes(product)) {
          errors.push(
            err("STP19-explicit", "removed-fixture", `manifest is missing product ${product}`),
          );
        }
      }
    }
  }
  return errors;
}

function runCargo(repoRoot, args) {
  const result = spawnSync("cargo", args, {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    timeout: 600000,
    env: process.env,
  });
  return {
    status: result.status,
    error: result.error,
    stdout: `${result.stdout || ""}${result.stderr || ""}`,
  };
}

export function runRustCases(repoRoot = REPO_ROOT) {
  // Both lanes compile the owning crate and execute the advanced generic
  // use suites: unit facts over real template bytes plus the production
  // `advanced_generic_uses` path over admitted `.vue` carrier bytes. Each
  // lane keeps its own result so a zero-test lane cannot hide behind the
  // other lane's passes.
  const lanes = [
    ["test", "-p", "verter_compiler", "--lib", LIB_FILTER, "--", "--test-threads=1"],
    ["test", "-p", "verter_compiler", "--test", "main", CARRIER_TEST, "--", "--test-threads=1"],
  ].map((args) => runCargo(repoRoot, args));
  return {
    status: lanes.every((lane) => lane.status === 0) ? 0 : 1,
    error: lanes.find((lane) => lane.error)?.error,
    stdout: lanes.map((lane) => lane.stdout).join("\n"),
    lanes,
  };
}

function testPassed(output, name) {
  return output.includes(`${name} ... ok`) && !output.includes(`${name} ... FAILED`);
}

function testFailed(output, name) {
  return output.includes(`${name} ... FAILED`);
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP19_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  const lanes = Array.isArray(run.lanes) ? run.lanes : [{ status: run.status, stdout: output }];
  for (const [index, lane] of lanes.entries()) {
    const laneOutput = String(lane.stdout || "");
    if (lane.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(laneOutput)) {
      return STP19_MANDATORY_CASES.map((id) =>
        err(
          id,
          "rust-case",
          `cargo lane ${index} did not execute its advanced generic use cases (status=${lane.status})`,
        ),
      );
    }
  }
  if (!testPassed(output, CARRIER_TEST)) {
    errors.push(
      err("STP19-forward", "rust-case", `the production carrier lane did not pass ${CARRIER_TEST}`),
    );
  }
  for (const [id, names] of Object.entries(RUST_CASES)) {
    for (const name of names) {
      if (!testPassed(output, name)) {
        errors.push(err(id, "rust-case", `cargo test did not pass ${name}`));
      }
    }
  }
  // Consume every exported dirty twin so an unused export fails loudly;
  // physical rejection is proven by the patch runs.
  const twins = {
    DIRTY_FIXED_SIGNATURE_ADAPTER,
    DIRTY_OPEN_ARGS_WIDENED,
    DIRTY_SCOPE_DROPS_BINDER,
    DIRTY_FABRICATED_WITNESS,
    DIRTY_ADAPTER_ANY,
    DIRTY_ALIAS_ERASED,
    DIRTY_OPEN_CATCH_ALL_REPLACES_OVERLOADS,
    DIRTY_CALL_OVERLOADS_COLLAPSE,
    DIRTY_CALL_OVERLOADS_INDEXED,
    DIRTY_GENERIC_STALL_REOPENS_CATCH_ALL,
    DIRTY_GENERIC_STALL_DROPS_READ_OVERLOADS,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined || (!DIRTY_TWIN_PATCHES[twin] && !TS_TWINS[twin])) {
      errors.push(err("STP19-erasure", "dirty-twin-unproven", `${twin} has no applied patch`));
    }
  }
  return errors;
}

export function assertDirtyTwinsRejected(repoRoot = REPO_ROOT, only = null) {
  const errors = [];
  const abs = path.join(repoRoot, PRODUCT_RS);
  let original = null;
  try {
    original = fs.readFileSync(abs, "utf8");
  } catch {
    return [err("STP19-erasure", "missing-check", `${PRODUCT_RS} is unreadable`)];
  }
  for (const [twin, spec] of Object.entries(DIRTY_TWIN_PATCHES)) {
    if (only !== null && twin !== only) continue;
    // The anchor must occur exactly once so the plant provably lands on the
    // intended site and nowhere else.
    if (original.split(spec.find).length !== 2) {
      errors.push(
        err(
          spec.caseId,
          "dirty-twin-unproven",
          `${twin} patch anchor is not unique in the owned product: ${spec.find.slice(0, 80)}`,
        ),
      );
      continue;
    }
    const mutated = original.replace(spec.find, spec.replace);
    const libNames = spec.discriminators.filter((name) => name !== CARRIER_TEST);
    const carrierNames = spec.discriminators.filter((name) => name === CARRIER_TEST);
    let run = null;
    let carrierRun = null;
    fs.writeFileSync(abs, mutated);
    try {
      if (!fs.readFileSync(abs, "utf8").includes(spec.replace)) {
        errors.push(err(spec.caseId, "dirty-twin-unproven", `${twin} plant did not land`));
        continue;
      }
      run = runCargo(repoRoot, [
        "test",
        "-p",
        "verter_compiler",
        "--lib",
        LIB_FILTER,
        "--",
        "--test-threads=1",
      ]);
      if (carrierNames.length > 0) {
        carrierRun = runCargo(repoRoot, [
          "test",
          "-p",
          "verter_compiler",
          "--test",
          "main",
          CARRIER_TEST,
          "--",
          "--test-threads=1",
        ]);
      }
    } finally {
      fs.writeFileSync(abs, original);
    }
    if (run.error || carrierRun?.error) {
      errors.push(
        err(
          spec.caseId,
          "missing-check",
          `${twin} run failed to spawn: ${(run.error || carrierRun.error).message}`,
        ),
      );
      continue;
    }
    const output = String(run.stdout || "");
    const carrierOutput = String(carrierRun?.stdout || "");
    const ran = libNames.every((name) => output.includes(name));
    const carrierRan = carrierNames.every((name) => carrierOutput.includes(name));
    if (!ran || !carrierRan) {
      errors.push(
        err(
          spec.caseId,
          "missing-check",
          `${twin} run did not execute its discriminators (status=${run.status})`,
        ),
      );
      continue;
    }
    // Rejection proof requires each discriminator to FAIL for the stated
    // mutation: a nonzero status alone is not proof, since an unrelated
    // compile error also exits nonzero while the discriminator never ran.
    const rejected =
      run.status !== 0 &&
      libNames.every((name) => testFailed(output, name)) &&
      (carrierRun === null ||
        (carrierRun.status !== 0 && carrierNames.every((name) => testFailed(carrierOutput, name))));
    if (!rejected) {
      errors.push(
        err(
          spec.caseId,
          "dirty-twin-unproven",
          `${twin} applied but its discriminators did not all fail: the mutation is not discriminated`,
        ),
      );
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(err("STP19-erasure", "missing-check", "mutated source was not restored to clean"));
  }
  return errors;
}

async function checkFile(runEngine, engine, repoRoot, rel) {
  const probes = {
    positive: rel,
    negative: rel,
    cleanTwin: rel,
    tsconfig: `${PROBES}/tsconfig.json`,
  };
  const run = await runEngine(engine, probes);
  const text = fs.readFileSync(path.join(repoRoot, rel), "utf8");
  return { diags: run.positive.diags, text };
}

function lineAt(text, pos) {
  const start = text.lastIndexOf("\n", pos - 1) + 1;
  const end = text.indexOf("\n", pos);
  return text.slice(start, end < 0 ? text.length : end);
}

/**
 * Every diagnostic of `rel` has `code` and sits on an anchored line at the
 * anchored authored token; every anchor has exactly one diagnostic.
 */
export async function assertAnchoredNegatives({ repoRoot = REPO_ROOT, engines, runEngine }) {
  const errors = [];
  const files = [
    { rel: `${PROBES}/negative-construction.ts`, code: 2769, anchors: CONSTRUCTION_ANCHORS },
    { rel: `${PROBES}/negative-constraint.ts`, code: 2344, anchors: CONSTRAINT_ANCHORS },
  ];
  for (const engine of engines) {
    for (const file of files) {
      const { diags, text } = await checkFile(runEngine, engine, repoRoot, file.rel);
      const hits = new Map(file.anchors.map((anchor) => [anchor, 0]));
      for (const diag of diags) {
        const line = Number.isInteger(diag.pos) ? lineAt(text, diag.pos) : "";
        const anchor = file.anchors.find(
          (a) => line.includes(a.line) && text.startsWith(a.at, diag.pos),
        );
        if (diag.code !== file.code || !anchor) {
          errors.push(
            err(
              file.anchors[0].caseId,
              "unrelated-generated-error",
              `${engine.id} ${file.rel}: TS${diag.code} at an unanchored site: ${diag.message}`,
            ),
          );
          continue;
        }
        hits.set(anchor, hits.get(anchor) + 1);
      }
      for (const [anchor, count] of hits) {
        if (count !== 1) {
          errors.push(
            err(
              anchor.caseId,
              "missing-negative",
              `${engine.id} ${file.rel}: ${count} TS${file.code} at ${anchor.at} on the line with ${anchor.line}`,
            ),
          );
        }
      }
    }
  }
  return errors;
}

/**
 * Each TypeScript dirty twin changes its named verdict on every engine: the
 * clean probe must first hold the opposite verdict, so a plant that fails to
 * apply (or a verdict the clean probe already has) cannot pass.
 */
export async function assertTsTwinsRejected({ repoRoot = REPO_ROOT, engines, runEngine }) {
  const errors = [];
  const verdict = (diags, text, expect) => {
    const anchor = expect.gains || expect.loses;
    return diags.some(
      (diag) => Number.isInteger(diag.pos) && lineAt(text, diag.pos).includes(anchor),
    );
  };
  for (const [twin, spec] of Object.entries(TS_TWINS)) {
    const originalRel = `${PROBES}/${spec.probe}`;
    const original = fs.readFileSync(path.join(repoRoot, originalRel), "utf8");
    const mutated = spec.rewrite(original);
    if (mutated === original) {
      errors.push(err(spec.caseId, "dirty-twin-unproven", `${twin} rewrite did not apply`));
      continue;
    }
    const twinRel = `${PROBES}/zz-twin-${twin.toLowerCase()}.ts`;
    const twinAbs = path.join(repoRoot, twinRel);
    fs.writeFileSync(twinAbs, mutated);
    try {
      for (const engine of engines) {
        const clean = await checkFile(runEngine, engine, repoRoot, originalRel);
        const dirty = await checkFile(runEngine, engine, repoRoot, twinRel);
        const cleanHit = verdict(clean.diags, clean.text, spec.expect);
        const dirtyHit = verdict(dirty.diags, dirty.text, spec.expect);
        const rejected = spec.expect.gains ? !cleanHit && dirtyHit : cleanHit && !dirtyHit;
        if (!rejected) {
          errors.push(
            err(
              spec.caseId,
              "dirty-twin-unproven",
              `${engine.id} ${twin} did not change the verdict at ${spec.expect.gains || spec.expect.loses}`,
            ),
          );
        }
      }
    } finally {
      fs.rmSync(twinAbs, { force: true });
    }
  }
  return errors;
}

export async function evaluateStp19({
  repoRoot = REPO_ROOT,
  engines = [],
  runEngine = null,
  skipProbes = false,
} = {}) {
  const errors = [];
  errors.push(...validateStp19Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP19 || []) {
    if (!STP19_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP19-explicit", "unknown-row", `unowned mandatory case ${id}`));
    }
  }
  if (!skipProbes) {
    if (engines.length === 0) {
      errors.push(err("STP19-explicit", "missing-probes", "no resolved engine for STP19 probes"));
    } else if (typeof runEngine !== "function") {
      errors.push(err("STP19-explicit", "missing-probes", "no engine runner for STP19 probes"));
    } else {
      errors.push(...(await assertAnchoredNegatives({ repoRoot, engines, runEngine })));
      errors.push(...(await assertTsTwinsRejected({ repoRoot, engines, runEngine })));
    }
  }
  const before = errors.length;
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  if (errors.length === before) {
    // Clean discriminators pass: now prove each dirty twin fails them.
    errors.push(...assertDirtyTwinsRejected(repoRoot));
  }
  return { errors };
}
