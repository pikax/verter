// Negative controls for the aggregate memory budget contract.
//
// Discipline every case below follows, in order:
//
//   1. assert the PRE-mutation state, so a mutation that silently fails to
//      apply cannot be mistaken for a refusal;
//   2. apply the mutation;
//   3. assert the POST-mutation state actually differs;
//   4. assert validation refuses it, and refuses it FOR THE INTENDED
//      REASON, not merely with some error.
//
// A control that cannot distinguish "the mutation did not apply" from "the
// contract is sound" is not a control, so step 1 and step 3 are not
// decoration. The clean-tree case runs first and asserts zero errors, so a
// validator that failed everything unconditionally would not pass here.

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  loadCatalog,
  loadSchema,
  PACKAGE_ROOT,
  validateSemanticMemoryBudgetModel,
} from "./semantic-memory-budget.mjs";
import { expandWorkload, serializeManifest } from "./semantic-memory-workload.mjs";
import { validateSchemaObject } from "./lib.mjs";

const schema = loadSchema();

function freshCatalog() {
  return loadCatalog();
}

function validate(catalog, packageRoot = PACKAGE_ROOT) {
  return validateSemanticMemoryBudgetModel(catalog, schema, validateSchemaObject, packageRoot);
}

/** Assert that at least one error mentions `needle`. */
function refusedBecause(errors, needle) {
  assert.ok(errors.length > 0, "expected the mutated contract to be refused");
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `expected a refusal mentioning ${JSON.stringify(needle)}, got:\n${errors.join("\n")}`,
  );
}

test("the committed contract validates clean", () => {
  assert.deepEqual(validate(freshCatalog()), []);
});

test("a missing budget limit is refused", () => {
  const catalog = freshCatalog();
  assert.ok(
    Number.isSafeInteger(catalog.budget.normal.per_entry_admission_max_bytes),
    "pre-state: the per-entry admission cap must exist before it can be removed",
  );
  delete catalog.budget.normal.per_entry_admission_max_bytes;
  assert.ok(
    !Object.hasOwn(catalog.budget.normal, "per_entry_admission_max_bytes"),
    "post-state: the mutation did not apply",
  );
  refusedBecause(validate(catalog), "missing required property per_entry_admission_max_bytes");
});

test("a symbolic placeholder in place of a finite limit is refused", () => {
  const catalog = freshCatalog();
  assert.equal(typeof catalog.budget.pressure.cache_owned_retained_max_bytes, "number");
  catalog.budget.pressure.cache_owned_retained_max_bytes = "unlimited";
  assert.equal(catalog.budget.pressure.cache_owned_retained_max_bytes, "unlimited");
  refusedBecause(validate(catalog), "cache_owned_retained_max_bytes: expected integer");
});

test("a budget whose components do not partition the ceiling is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.budget.normal.cache_owned_retained_max_bytes;
  catalog.budget.normal.cache_owned_retained_max_bytes = before + 4096;
  assert.notEqual(catalog.budget.normal.cache_owned_retained_max_bytes, before);
  refusedBecause(validate(catalog), "is not the declared process ceiling");
});

test("a pressure ceiling at or above the normal ceiling is refused", () => {
  const catalog = freshCatalog();
  const normal = catalog.budget.normal.cache_owned_retained_max_bytes;
  assert.ok(catalog.budget.pressure.cache_owned_retained_max_bytes < normal);
  catalog.budget.pressure.cache_owned_retained_max_bytes = normal;
  catalog.budget.pressure.process_rss_max_bytes =
    normal +
    catalog.budget.pressure.request_active_max_bytes +
    catalog.budget.pressure.external_pinned_backpressure_threshold_bytes +
    catalog.budget.pressure.allocator_slack_bytes;
  assert.equal(catalog.budget.pressure.cache_owned_retained_max_bytes, normal);
  refusedBecause(validate(catalog), "must be below the normal ceiling");
});

test("a pinned-result policy claiming a hard bound over caller-held results is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.pinned_result_policy.hard_bound_over_caller_retained_results, false);
  catalog.pinned_result_policy.hard_bound_over_caller_retained_results = true;
  assert.equal(catalog.pinned_result_policy.hard_bound_over_caller_retained_results, true);
  refusedBecause(validate(catalog), "hard_bound_over_caller_retained_results: expected constant");
});

test("a policy that revokes live public handles is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.pinned_result_policy.revocation_of_live_public_handles, "forbidden");
  catalog.pinned_result_policy.revocation_of_live_public_handles = "permitted_under_pressure";
  refusedBecause(validate(catalog), "revocation_of_live_public_handles: expected constant");
});

test("a scale derivation that does not follow from the recorded measurement is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.scale.measured_peak_rss_bytes_per_file;
  catalog.scale.measured_peak_rss_bytes_per_file = before - 1;
  assert.notEqual(catalog.scale.measured_peak_rss_bytes_per_file, before);
  refusedBecause(validate(catalog), "but the recorded measurement divides to");
});

test("two allocation classes sharing one charge owner are refused", () => {
  const catalog = freshCatalog();
  const [first, second] = catalog.allocation_class;
  assert.notEqual(first.charge_owner, second.charge_owner, "pre-state: owners must start distinct");
  second.charge_owner = first.charge_owner;
  assert.equal(second.charge_owner, first.charge_owner);
  refusedBecause(validate(catalog), "would double-charge");
});

test("an allocation class whose charge owner no longer exists is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.allocation_class[0];
  const before = row.charge_owner;
  row.charge_owner = `${before}ThatWasRenamedAway`;
  assert.notEqual(row.charge_owner, before);
  refusedBecause(validate(catalog), "no longer occurs in");
});

test("dropping an ownership category from the inventory is refused", () => {
  const catalog = freshCatalog();
  const pinned = catalog.allocation_class.filter((row) => row.ownership === "externally_pinned");
  assert.ok(pinned.length > 0, "pre-state: an externally pinned class must exist");
  catalog.allocation_class = catalog.allocation_class.filter(
    (row) => row.ownership !== "externally_pinned",
  );
  assert.equal(
    catalog.allocation_class.filter((row) => row.ownership === "externally_pinned").length,
    0,
  );
  refusedBecause(validate(catalog), "no class carries externally_pinned ownership");
});

test("a metric row naming a nonexistent instrumentation site is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.metric_row[0];
  const before = row.id;
  row.id = "session.retained_bytes_that_does_not_exist";
  assert.notEqual(row.id, before);
  refusedBecause(validate(catalog), "no such instrumentation site exists");
});

test("a metric row declaring the wrong unit is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.metric_row.find((entry) => entry.unit === "Calls");
  assert.ok(row, "pre-state: a Calls-unit row must exist");
  row.unit = "Bytes";
  refusedBecause(validate(catalog), "is not the site's unit");
});

test("a metric row anchored at a file that does not raise it is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.metric_row.find((entry) => entry.id === "session.decl_body_lower");
  const before = row.emitter_anchor;
  assert.ok(before.endsWith(".rs"), "pre-state: the anchor must start at a real emitter file");
  // A file that exists, is Rust, and is even in the same subsystem — but
  // does not raise this site. A bare existence check would accept it.
  row.emitter_anchor = "crates/verter_session/src/project_type_store.rs";
  assert.notEqual(row.emitter_anchor, before);
  refusedBecause(validate(catalog), "no longer raises DeclBodyLower");
});

test("a baseline whose corpus digest no longer recomputes is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.baseline.corpus_sha256;
  catalog.baseline.corpus_sha256 = `${"0".repeat(63)}1`;
  assert.notEqual(catalog.baseline.corpus_sha256, before);
  refusedBecause(validate(catalog), "is not equivalent work on this tree");
});

test("a baseline that stops citing the locked gate file is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.baseline.baseline_sha;
  catalog.baseline.baseline_sha = "0".repeat(40);
  assert.notEqual(catalog.baseline.baseline_sha, before);
  refusedBecause(validate(catalog), "baseline sha is not cited by");
});

test("an unretained document claimed as retained is refused", () => {
  const catalog = freshCatalog();
  assert.equal(
    catalog.baseline.recorded_measurement_document_retained,
    false,
    "pre-state: the raw measurement record is disclosed as unretained",
  );
  catalog.baseline.recorded_measurement_document_retained = true;
  refusedBecause(validate(catalog), "declared retained but does not resolve");
});

test("an incomplete emitter recorded as complete is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.metric_row.find((entry) => entry.emitter_status === "partial");
  assert.ok(row, "pre-state: the contract must record at least one partial emitter");
  row.emitter_status = "complete";
  assert.equal(row.emitter_status, "complete");
  refusedBecause(validate(catalog), "a complete emitter may not record a gap");
});

test("a workload below the contract minimum is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.workload.tranches, 100);
  catalog.workload.tranches = 50;
  catalog.workload.total_actions = 5000;
  assert.equal(catalog.workload.tranches, 50);
  const errors = validate(catalog);
  refusedBecause(errors, "below the contract minimum of 10000");
  refusedBecause(errors, "total_actions: value is below 10000");
});

test("an omitted lifecycle class is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_step.length;
  catalog.workload_step = catalog.workload_step.filter((step) => step.kind !== "request_cancelled");
  assert.ok(catalog.workload_step.length < before, "post-state: the step rows did not change");
  // Keep each cycle's action count intact so the refusal is about the
  // MISSING CLASS rather than about a short tranche.
  for (const step of catalog.workload_step) if (step.kind === "request_cold") step.count += 4;
  refusedBecause(validate(catalog), "required lifecycle class cancellation is not covered");
});

test("an omitted carrier is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_fixture.length;
  catalog.workload_fixture = catalog.workload_fixture.filter(
    (fixture) => fixture.carrier !== "svelte",
  );
  assert.ok(catalog.workload_fixture.length < before);
  refusedBecause(validate(catalog), "the svelte carrier is not covered");
});

test("a tranche that does not restore its control live set is refused", () => {
  const catalog = freshCatalog();
  const revert = catalog.workload_step.find((step) => step.cycle === "a" && step.kind === "revert");
  const cold = catalog.workload_step.find(
    (step) => step.cycle === "a" && step.kind === "request_cold",
  );
  assert.equal(revert.count, 8, "pre-state: cycle a must revert its eight edits");
  // Revert one fewer edit than the tranche applies, and keep the tranche's
  // action count intact so the refusal is about the unreverted edit rather
  // than about a short tranche.
  revert.count = 7;
  cold.count += 1;
  assert.equal(revert.count, 7, "post-state: the revert count did not change");
  refusedBecause(validate(catalog), "not exactly the edits it reverts");
});

test("a same-key-only workload is refused", () => {
  const catalog = freshCatalog();
  assert.ok(catalog.workload.min_distinct_query_identities >= 3000);
  const cold = catalog.workload_step.filter((step) => step.kind === "request_cold");
  assert.equal(cold.length, 2, "pre-state: both cycles mint cold identities");
  assert.ok(cold.every((step) => step.count === 30));
  // Collapse each tranche to a single cold identity and give the freed
  // actions to warm requests, which re-enter that one identity. The tranche
  // still runs 100 actions; it just stops covering distinct keys.
  for (const step of cold) step.count = 1;
  for (const cycle of ["a", "b"])
    catalog.workload_step.find(
      (step) => step.cycle === cycle && step.kind === "request_warm",
    ).count += 29;
  assert.ok(
    cold.every((step) => step.count === 1),
    "post-state: the mutation did not apply",
  );
  refusedBecause(validate(catalog), "same-key repetition cannot satisfy this workload");
});

test("failed construction against a healthy fixture is refused", () => {
  const catalog = freshCatalog();
  const kind = catalog.workload_kind.find((row) => row.kind === "request_failed_construction");
  assert.equal(kind.fixture_pool, "malformed");
  kind.fixture_pool = "healthy";
  assert.equal(kind.fixture_pool, "healthy");
  refusedBecause(validate(catalog), "must run against a malformed fixture");
});

test("an unbound negative-control command is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.command.find((entry) => entry.kind === "negative_control");
  assert.ok(row?.bound === true, "pre-state: the negative controls must start bound");
  row.bound = false;
  row.lane = "unbound";
  refusedBecause(validate(catalog), "no bound negative-control command is declared");
});

test("a stale manifest pin is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload.manifest_sha256;
  catalog.workload.manifest_sha256 = `${"0".repeat(63)}1`;
  assert.notEqual(catalog.workload.manifest_sha256, before);
  refusedBecause(validate(catalog), "manifest digest recomputes to");
});

test("an edited fixture on disk is refused", (t) => {
  // A real on-disk control: mirror the package's contract inputs into a
  // temporary root, change one byte of one frozen fixture, and confirm the
  // pin catches it. The in-memory controls above cannot prove the fixture
  // digests are read from disk at all; this one can.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "mem0-budget-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relative of ["catalogs", "schemas", "tools/semantic-memory-workload.mjs"])
    fs.cpSync(path.join(PACKAGE_ROOT, relative), path.join(root, relative), { recursive: true });

  assert.deepEqual(
    validate(loadCatalog(root), root),
    [],
    "pre-state: the mirrored tree must validate before it is mutated",
  );

  const catalog = loadCatalog(root);
  const fixture = catalog.workload_fixture.find((entry) => entry.carrier === "vue");
  const target = path.join(root, fixture.path);
  const original = fs.readFileSync(target);
  fs.writeFileSync(target, Buffer.concat([original, Buffer.from("\n", "utf8")]));
  assert.notDeepEqual(
    fs.readFileSync(target),
    original,
    "post-state: the fixture bytes did not change",
  );

  refusedBecause(validate(loadCatalog(root), root), "digest recomputes to");
});

test("a mutated expander on disk is refused", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "mem0-expander-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relative of ["catalogs", "schemas", "tools/semantic-memory-workload.mjs"])
    fs.cpSync(path.join(PACKAGE_ROOT, relative), path.join(root, relative), { recursive: true });

  const expander = path.join(root, "tools/semantic-memory-workload.mjs");
  const original = fs.readFileSync(expander, "utf8");
  assert.ok(original.includes("export const NONE"), "pre-state: the expander source is intact");
  fs.writeFileSync(expander, `${original}\n// planted\n`, "utf8");
  assert.ok(
    fs.readFileSync(expander, "utf8").includes("// planted"),
    "post-state: the plant did not apply",
  );

  refusedBecause(validate(loadCatalog(root), root), "expander digest recomputes to");
});

test("the expansion is deterministic", () => {
  const catalog = freshCatalog();
  const spec = {
    tranches: catalog.workload.tranches,
    tranche_actions: catalog.workload.tranche_actions,
    projects_per_tranche: catalog.workload.projects_per_tranche,
    projects: catalog.workload_project,
    fixtures: catalog.workload_fixture,
    steps: catalog.workload_step,
    configuration_deltas: catalog.workload_configuration_delta,
    cancellation_points: catalog.workload_cancellation_point,
    kinds: Object.fromEntries(catalog.workload_kind.map((row) => [row.kind, row])),
  };
  assert.equal(serializeManifest(expandWorkload(spec)), serializeManifest(expandWorkload(spec)));
});
