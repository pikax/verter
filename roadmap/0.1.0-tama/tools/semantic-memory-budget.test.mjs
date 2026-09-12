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
//
// Several controls drive one invariant checker directly with a hand-built
// action list. That is deliberate: the frozen expander cannot produce a
// mispaired configuration revert or a split overlap group, so mutating the
// catalog could not reach those invariants at all, and a check nothing can
// fail is not a check.

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { CI_WORKFLOW } from "./closure-register.mjs";
import {
  coverageErrors,
  ingredientUseErrors,
  loadCatalog,
  loadSchema,
  overlapGroupErrors,
  PACKAGE_ROOT,
  postMutationErrors,
  REPO_ROOT,
  RETAINED_STATE_ROOT,
  structFields,
  trancheErrors,
  validateSemanticMemoryBudgetModel,
  workloadSpec,
} from "./semantic-memory-budget.mjs";
import { expandWorkload, serializeManifest } from "./semantic-memory-workload.mjs";
import { validateSchemaObject } from "./lib.mjs";

const schema = loadSchema();

function freshCatalog() {
  return loadCatalog();
}

function validate(catalog, packageRoot = PACKAGE_ROOT, options = {}) {
  return validateSemanticMemoryBudgetModel(
    catalog,
    schema,
    validateSchemaObject,
    packageRoot,
    options,
  );
}

/** Mirror the package's contract inputs into a temporary root. */
function mirrorPackage(t, prefix) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const relative of ["catalogs", "schemas", "tools/semantic-memory-workload.mjs"])
    fs.cpSync(path.join(PACKAGE_ROOT, relative), path.join(root, relative), { recursive: true });
  return root;
}

/**
 * A temporary copy of the CI workflow with `find` replaced, after proving
 * `find` occurs exactly once before and not at all after.
 */
function mutatedWorkflow(t, find, replace) {
  const source = fs.readFileSync(path.join(REPO_ROOT, CI_WORKFLOW), "utf8");
  assert.equal(
    source.split(find).length - 1,
    1,
    `pre-state: ${JSON.stringify(find)} must occur exactly once in the workflow`,
  );
  const mutated = source.replace(find, replace);
  assert.equal(mutated.split(find).length - 1, 0, "post-state: the plant did not apply");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mem0-workflow-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, "ci.yml");
  fs.writeFileSync(file, mutated, "utf8");
  return file;
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

// ── budget shape ─────────────────────────────────────────────────────────

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

// A complete result is built inside one request's active budget, so an entry
// cap that is not strictly below that budget leaves no result both too large
// to admit and small enough to finish: the equality edge is as unreachable as
// a cap above it, in either mode.
for (const [mode, edge] of [
  ["pressure", "equal to"],
  ["pressure", "above"],
  ["normal", "equal to"],
  ["normal", "above"],
])
  test(`a ${mode} entry cap ${edge} the per-request cap makes the oversized boundary unreachable`, () => {
    const catalog = freshCatalog();
    const budget = catalog.budget[mode];
    const perRequest = budget.per_request_active_max_bytes;
    assert.ok(
      budget.per_entry_admission_max_bytes < perRequest,
      "pre-state: the committed entry cap sits below the per-request cap",
    );
    budget.per_entry_admission_max_bytes = edge === "equal to" ? perRequest : perRequest * 2;
    // Keep every other budget identity intact, so the refusal is this one.
    if (mode === "normal")
      catalog.budget.pressure.per_entry_admission_max_bytes = budget.per_entry_admission_max_bytes;
    for (const row of catalog.limit_derivation)
      if (row.limit === "per_entry_admission_max_bytes")
        row.value = catalog.budget[row.mode].per_entry_admission_max_bytes;
    assert.equal(
      budget.per_entry_admission_max_bytes >= perRequest,
      true,
      "post-state: the mutation did not apply",
    );
    const errors = validate(catalog);
    refusedBecause(errors, `budget.${mode}: the per-entry admission cap must be below`);
  });

// ── control-live-set floor ───────────────────────────────────────────────

test("a control-live-set floor naming the larger byte-bounded class is refused", () => {
  const catalog = freshCatalog();
  const measurement = catalog.measurement;
  assert.equal(measurement.control_live_set_slack_class, "identity_intern_pool");
  assert.deepEqual(measurement.control_live_set_unexercised_byte_bounded_classes, [
    "supplied_block_content_store",
  ]);
  // Swap which pool sets the floor without moving the floor itself.
  measurement.control_live_set_slack_class = "supplied_block_content_store";
  measurement.control_live_set_unexercised_byte_bounded_classes = ["identity_intern_pool"];
  assert.equal(measurement.control_live_set_slack_class, "supplied_block_content_store");
  refusedBecause(
    validate(catalog),
    "the control-live-set floor is 4194304 bytes, but its pool supplied_block_content_store is bounded at 67108864",
  );
});

test("a byte-bounded class the floor's rationale does not account for is refused", () => {
  const catalog = freshCatalog();
  const measurement = catalog.measurement;
  const before = measurement.control_live_set_unexercised_byte_bounded_classes.length;
  assert.equal(before, 1, "pre-state: one byte-bounded class is declared unexercised");
  measurement.control_live_set_unexercised_byte_bounded_classes = [];
  assert.equal(measurement.control_live_set_unexercised_byte_bounded_classes.length, 0);
  refusedBecause(
    validate(catalog),
    "byte-bounded allocation class supplied_block_content_store neither sets the control-live-set floor nor is declared unexercised",
  );
});

test("a control-live-set floor that drifts from its pool's bound is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.measurement.control_live_set_absolute_slack_bytes;
  const pool = catalog.allocation_class.find((row) => row.id === "identity_intern_pool");
  assert.equal(before, pool.current_bound_value, "pre-state: the floor is one whole pool");
  catalog.measurement.control_live_set_absolute_slack_bytes = before * 2;
  assert.notEqual(catalog.measurement.control_live_set_absolute_slack_bytes, before);
  refusedBecause(validate(catalog), "but its pool identity_intern_pool is bounded at");
});

// ── limit provenance ─────────────────────────────────────────────────────

test("a provisional limit relabelled as measured is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.limit_derivation.find(
    (entry) => entry.id === "normal.cache_owned_retained_max_bytes",
  );
  assert.equal(row.derivation, "provisional", "pre-state: the retained ceiling is not measured");
  row.derivation = "measured";
  assert.equal(row.derivation, "measured", "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "a measured limit is not blocked on anything");
});

test("a provisional limit blocked on a complete emitter is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.limit_derivation.find(
    (entry) => entry.id === "normal.cache_owned_retained_max_bytes",
  );
  const complete = catalog.metric_row.find((entry) => entry.emitter_status === "complete");
  assert.ok(complete, "pre-state: at least one emitter is complete");
  assert.notEqual(row.blocking_metric_row, complete.id);
  row.blocking_metric_row = complete.id;
  refusedBecause(validate(catalog), "blocks nothing");
});

test("a ratified limit with no recorded derivation is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.limit_derivation.length;
  catalog.limit_derivation = catalog.limit_derivation.filter(
    (entry) => entry.id !== "pressure.per_entry_admission_max_bytes",
  );
  assert.equal(catalog.limit_derivation.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "budget.pressure.per_entry_admission_max_bytes is ratified with no recorded derivation",
  );
});

test("a derivation whose value drifts from the budget it explains is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.limit_derivation.find((entry) => entry.id === "normal.process_rss_max_bytes");
  const before = row.value;
  row.value = before + 1;
  assert.notEqual(row.value, before);
  refusedBecause(validate(catalog), "but the budget declares");
});

// ── live memory observations ─────────────────────────────────────────────
//
// This table records byte-valued memory the process really does report.
// The controls below exist because an inventory of live instrumentation is
// only worth having if it cannot drift away from the instrumentation: a
// producer that was renamed, a scope claim that outgrew what the code
// computes, or a widened observation that silently goes on blocking the
// limits it no longer blocks.

test("dropping the live memory observations is refused", () => {
  const catalog = freshCatalog();
  assert.ok(catalog.memory_observation.length >= 2, "pre-state: the surface is recorded");
  delete catalog.memory_observation;
  assert.ok(
    !Object.hasOwn(catalog, "memory_observation"),
    "post-state: the mutation did not apply",
  );
  refusedBecause(validate(catalog), "missing required property memory_observation");
});

test("a memory observation whose producer no longer exists is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.host_cache_bytes");
  const before = row.producer;
  assert.ok(before.includes("#"), "pre-state: the producer is an anchored symbol");
  row.producer = before + "ThatWasRenamedAway";
  assert.notEqual(row.producer, before, "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "no longer occurs in");
});

test("a memory observation whose liveness proof no longer exists is refused", () => {
  // Without a resolving proof anchor the table would be free to assert
  // that a defaulted zero is a live measurement.
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.process_rss_peak");
  const before = row.proof;
  row.proof = "crates/verter_session/tests/cases/g_misc1/a_proof_that_was_deleted.rs";
  assert.notEqual(row.proof, before, "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "anchor path does not resolve");
});

test("widening an observation to a true aggregate without re-ratifying its limits is refused", () => {
  // The load-bearing control. Every aggregate retained-byte limit is
  // provisional BECAUSE no live observation aggregates cache-owned bytes.
  // The day one does, those limits become measurable against it and may
  // not go on being recorded provisional.
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.host_cache_bytes");
  assert.equal(row.aggregates_cache_owned, false, "pre-state: nothing aggregates today");
  const blocked = catalog.limit_derivation.filter(
    (entry) => entry.blocking_metric_row === "memory.host_cache_bytes",
  );
  assert.ok(blocked.length > 0, "pre-state: limits are blocked on it");
  assert.ok(
    blocked.every((entry) => entry.derivation === "provisional"),
    "pre-state: every limit blocked on it is provisional",
  );
  row.aggregates_cache_owned = true;
  assert.equal(row.aggregates_cache_owned, true, "post-state: the mutation did not apply");
  refusedBecause(validate(catalog), "already aggregates cache-owned bytes, so it blocks nothing");
});

test("a whole-process observation claiming to cover an allocation class is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.process_rss");
  assert.equal(row.coverage, "whole_process");
  assert.deepEqual(row.covers_allocation_classes, [], "pre-state: it covers no class");
  row.covers_allocation_classes = ["file_artifact_store"];
  refusedBecause(validate(catalog), "is owned by no class");
});

test("an observation covering a class recorded as unobserved is refused", () => {
  // The inventory and the observation table are two views of one fact and
  // may not disagree about whether a class is observed at all.
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.host_cache_bytes");
  const unobserved = catalog.allocation_class.find(
    (entry) => entry.charge_observability === "uninstrumented",
  );
  assert.ok(unobserved, "pre-state: some class is recorded uninstrumented");
  row.covers_allocation_classes = [...row.covers_allocation_classes, unobserved.id];
  refusedBecause(validate(catalog), "but this observation reports bytes for it");
});

test("an observation covering a class the inventory does not declare is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.host_cache_bytes");
  row.covers_allocation_classes = ["a_class_that_was_deleted"];
  refusedBecause(validate(catalog), "which is not a declared allocation class");
});

test("dropping the partial cache observation is refused", () => {
  // Losing this row would take the contract back to asserting that no
  // cache bytes are observed at all, which is not true of this tree.
  const catalog = freshCatalog();
  const before = catalog.memory_observation.length;
  catalog.memory_observation = catalog.memory_observation.filter(
    (entry) => entry.coverage !== "partial_cache_owned",
  );
  assert.ok(catalog.memory_observation.length < before, "post-state: no row was removed");
  refusedBecause(validate(catalog), "no partial_cache_owned observation is declared");
});

test("an assignment site passed off as a discriminating test is refused", () => {
  // The distinction this control protects: a production site that
  // assigns a value is evidence the value is written, not evidence that
  // anything would notice if it stopped being written.
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.host_cache_bytes");
  assert.equal(row.proof_kind, "live_assignment", "pre-state: it cites an assignment site");
  assert.ok(!row.proof.includes("/tests/"), "pre-state: the anchor is production source");
  row.proof_kind = "discriminating_test";
  refusedBecause(validate(catalog), "is not under a tests tree");
});

test("a test passed off as a production assignment site is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.memory_observation.find((entry) => entry.id === "memory.process_rss_peak");
  assert.equal(row.proof_kind, "discriminating_test", "pre-state: it cites a landed test");
  row.proof_kind = "live_assignment";
  refusedBecause(validate(catalog), "is a test, not a production assignment site");
});

test("an observation table with no discriminating test behind it is refused", () => {
  const catalog = freshCatalog();
  const tested = catalog.memory_observation.filter(
    (entry) => entry.proof_kind === "discriminating_test",
  );
  assert.equal(tested.length, 1, "pre-state: exactly one row carries a landed test");
  catalog.memory_observation = catalog.memory_observation.filter(
    (entry) => entry.proof_kind !== "discriminating_test",
  );
  assert.ok(catalog.memory_observation.length > 0, "post-state: rows remain");
  refusedBecause(validate(catalog), "no row carries a discriminating test");
});

test("a limit blocked on something in neither vocabulary is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.limit_derivation.find(
    (entry) => entry.id === "normal.cache_owned_retained_max_bytes",
  );
  assert.equal(
    row.blocking_metric_row,
    "memory.host_cache_bytes",
    "pre-state: it names the live observation",
  );
  row.blocking_metric_row = "memory.a_gauge_nobody_wrote";
  refusedBecause(
    validate(catalog),
    "is neither a declared metric row nor a declared memory observation",
  );
});

test("a pressure obligation with no complete signal behind it is refused", () => {
  // An obligation discharged only against partial instrumentation is a
  // promise: no run on this tree could satisfy it.
  const catalog = freshCatalog();
  const signals = catalog.measurement.pressure_boundary_signals;
  const complete = signals.filter((signal) =>
    catalog.metric_row.some((row) => row.id === signal && row.emitter_status === "complete"),
  );
  assert.equal(complete.length, 1, "pre-state: exactly one signal is a complete emitter");
  catalog.measurement.pressure_boundary_signals = signals.filter(
    (signal) => !complete.includes(signal),
  );
  assert.ok(
    catalog.measurement.pressure_boundary_signals.length > 0,
    "post-state: signals remain, so the refusal is about completeness rather than emptiness",
  );
  refusedBecause(validate(catalog), "rests entirely on partial evidence");
});

test("a pressure signal that resolves nowhere is refused", () => {
  const catalog = freshCatalog();
  catalog.measurement.pressure_boundary_signals = [
    ...catalog.measurement.pressure_boundary_signals,
    "session.a_counter_nobody_wrote",
  ];
  refusedBecause(validate(catalog), "pressure boundary signal session.a_counter_nobody_wrote");
});

test("dropping the runner-class promotion rule is refused", () => {
  // The rule records why no local run can promote a provisional limit,
  // which is half the reason the split is still provisional at all.
  const catalog = freshCatalog();
  assert.ok(catalog.baseline.local_run_promotion_rule.length > 40, "pre-state: the rule is stated");
  delete catalog.baseline.local_run_promotion_rule;
  refusedBecause(validate(catalog), "missing required property local_run_promotion_rule");
});

// ── pinned-result policy ─────────────────────────────────────────────────

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

test("dropping the copy-versus-share ownership rule is refused", () => {
  const catalog = freshCatalog();
  assert.ok(
    catalog.pinned_result_policy.copy_versus_share_rule.length > 40,
    "pre-state: the rule distinguishing a copy from a shared handle is stated",
  );
  delete catalog.pinned_result_policy.copy_versus_share_rule;
  assert.ok(!Object.hasOwn(catalog.pinned_result_policy, "copy_versus_share_rule"));
  refusedBecause(validate(catalog), "missing required property copy_versus_share_rule");
});

test("dropping the release-order rule is refused", () => {
  const catalog = freshCatalog();
  assert.ok(catalog.pinned_result_policy.release_order_rule.length > 40);
  delete catalog.pinned_result_policy.release_order_rule;
  refusedBecause(validate(catalog), "missing required property release_order_rule");
});

// ── baseline provenance ──────────────────────────────────────────────────

test("a scale derivation that does not follow from the recorded measurement is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.scale.measured_peak_rss_bytes_per_file;
  catalog.scale.measured_peak_rss_bytes_per_file = before - 1;
  assert.notEqual(catalog.scale.measured_peak_rss_bytes_per_file, before);
  refusedBecause(validate(catalog), "but the recorded measurement divides to");
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

test("a fabricated peak-RSS measurement is refused even with self-consistent arithmetic", () => {
  // The forgery this control models is the coherent one: not a number that
  // fails an internal identity, but a number the lock never recorded, with
  // every derived figure adjusted to agree with it. Only resolving the
  // value against the authority that recorded it can reject this.
  const catalog = freshCatalog();
  assert.equal(catalog.baseline.measured_peak_rss_bytes, 74850304, "pre-state: the locked value");
  catalog.baseline.measured_peak_rss_bytes = 41;
  catalog.baseline.measured_peak_rss_citation = "baseline is 41 bytes";
  catalog.scale.measured_peak_rss_bytes_per_file = 1;
  catalog.scale.projected_peak_rss_bytes_at_supported_scale = 1000;
  catalog.scale.headroom_bytes_at_supported_scale =
    catalog.budget.normal.process_rss_max_bytes - 1000;
  assert.equal(catalog.baseline.measured_peak_rss_bytes, 41, "post-state: the forgery applied");
  const errors = validate(catalog);
  // The arithmetic no longer discriminates: the derived figures agree.
  assert.ok(
    !errors.some((error) => error.includes("divides to")),
    `the forgery was made self-consistent, so arithmetic must not be what rejects it:\n${errors.join("\n")}`,
  );
  refusedBecause(errors, "does not record the peak RSS measurement");
});

test("a fabricated wall-time measurement is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.baseline.measured_wall_ns, 70525000);
  catalog.baseline.measured_wall_ns = 1;
  catalog.baseline.measured_wall_citation = "baseline is 1 ns";
  refusedBecause(validate(catalog), "does not record the wall time measurement");
});

test("a citation that does not render its own restated value is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.baseline.measured_peak_rss_citation;
  catalog.baseline.measured_peak_rss_citation = "baseline is 74850304 bytes";
  assert.notEqual(catalog.baseline.measured_peak_rss_citation, before);
  refusedBecause(validate(catalog), "renders as");
});

test("a fabricated relative regression limit is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.baseline.locked_peak_rss_no_regression_milli_percent;
  assert.equal(before, 4952, "pre-state: the locked 4.952% carried as milli-percent");
  catalog.baseline.locked_peak_rss_no_regression_milli_percent = 999999;
  assert.notEqual(catalog.baseline.locked_peak_rss_no_regression_milli_percent, before);
  refusedBecause(validate(catalog), "but the locked cell declares 4952");
});

test("a fabricated absolute wall limit is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.baseline.locked_wall_absolute_max_ns;
  catalog.baseline.locked_wall_absolute_max_ns = before * 10;
  assert.notEqual(catalog.baseline.locked_wall_absolute_max_ns, before);
  refusedBecause(validate(catalog), "wall_ns|median|absolute_max is restated as");
});

// ── allocation inventory ─────────────────────────────────────────────────

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

test("deleting an allocation class leaves its retained storage unaccounted", () => {
  // The required population is the walked structs' own field lists, so
  // removing a row cannot quietly shrink the contract.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find((row) => row.id === "route_db");
  assert.ok(victim, "pre-state: the route cache is inventoried");
  assert.deepEqual(victim.covers_fields, ["ProjectTypeStore.routes"]);
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter((row) => row.id !== "route_db");
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "ProjectTypeStore field routes is retained but has no allocation class",
  );
});

test("deleting a host-lifetime cache outside the project store is refused", () => {
  // The walk starts at VerterHost, not at the project store, so a cache
  // that lives on a framework subsystem is part of the population too.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find(
    (row) => row.id === "framework_script_candidate_store",
  );
  assert.deepEqual(victim.covers_fields, ["FrameworkScriptCaches.candidates"]);
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter((row) => row !== victim);
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "FrameworkScriptCaches field candidates is retained but has no allocation class",
  );
});

test("cutting a subsystem out of the walk is refused", () => {
  // Re-labelling a delegated field as non-retaining would otherwise drop
  // every cache behind it without any field ever being walked.
  const catalog = freshCatalog();
  const row = catalog.struct_field.find(
    (entry) => entry.struct === "VerterHost" && entry.field === "resolver",
  );
  assert.equal(row.disposition, "delegated", "pre-state: the resolver is walked");
  row.disposition = "non_retaining";
  delete row.delegate;
  assert.equal(row.disposition, "non_retaining");
  refusedBecause(validate(catalog), "never reaches UnifiedResolverRuntime");
});

test("a shared reference to a payload no class charges is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.struct_field.find(
    (entry) => entry.struct === "UnifiedResolverRuntime" && entry.field === "routes",
  );
  assert.equal(row.charged_at, "ProjectTypeStore.routes", "pre-state: charged at the store");
  row.charged_at = "ProjectTypeStore.counters";
  assert.equal(row.charged_at, "ProjectTypeStore.counters");
  refusedBecause(validate(catalog), "which no allocation class charges");
});

test("outside-inventory attribution to a non-workspace observation is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.struct_field.find(
    (entry) => entry.struct === "VerterHost" && entry.field === "workspace",
  );
  assert.equal(row.attributed_by, "memory.workspace_bytes", "pre-state");
  row.attributed_by = "memory.process_rss";
  assert.equal(row.attributed_by, "memory.process_rss");
  refusedBecause(validate(catalog), "must be attributed to a workspace memory observation");
});

test("moving a second field outside the inventory on the workspace observation is refused", () => {
  // Otherwise any retained field could be excused by pointing at the one
  // observation that reports VFS bytes.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find((row) => row.id === "scheduler_file_state");
  assert.deepEqual(victim.covers_fields, ["VerterHost.scheduler"]);
  catalog.allocation_class = catalog.allocation_class.filter((row) => row !== victim);
  catalog.struct_field.push({
    struct: "VerterHost",
    field: "scheduler",
    disposition: "outside_inventory",
    attributed_by: "memory.workspace_bytes",
    reason: "Claiming the scheduler's source versions are VFS bytes the workspace reports.",
  });
  assert.ok(!catalog.allocation_class.some((row) => row.id === "scheduler_file_state"));
  refusedBecause(validate(catalog), "one observation cannot excuse two fields");
});

test("the walked population is the production field set", () => {
  const fields = structFields(RETAINED_STATE_ROOT.file, RETAINED_STATE_ROOT.struct);
  // A target gate is production and stays in.
  assert.ok(fields.includes("host_cpu_pool"));
  assert.ok(fields.includes("framework_script_caches"));
  // Test-only fields never exist in a shipped build.
  assert.ok(!fields.includes("materialize_seam_hook"));
  assert.ok(!fields.includes("_test_worker_pool_lease"));

  // Charging a test-only field is charging something that does not ship.
  const catalog = freshCatalog();
  const row = catalog.allocation_class.find((entry) => entry.id === "host_path_bookkeeping");
  row.covers_fields = [...row.covers_fields, "VerterHost.materialize_seam_hook"];
  assert.ok(row.covers_fields.includes("VerterHost.materialize_seam_hook"));
  refusedBecause(
    validate(catalog),
    "covers VerterHost.materialize_seam_hook, which VerterHost no longer has",
  );
});

test("deleting a class that covers no walked field is refused", () => {
  // The walk cannot notice this one: the declaration body memo is a
  // sub-region reached through file artifacts, not a field of its own.
  // The charter's own named categories are the independent inventory for it.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find((row) => row.id === "declaration_body_memo");
  assert.deepEqual(victim.covers_fields, [], "pre-state: it covers no walked field");
  assert.equal(victim.ownership, "cache_owned");
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter(
    (row) => row.id !== "declaration_body_memo",
  );
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "the charter requires allocation class declaration_body_memo, which this catalog does not declare",
  );
});

test("charging one store field with several sub-region classes is refused", () => {
  // The walk never enters a charged field, so sub-region classes listed on
  // one field are a closed inventory nothing checks. The store has to be
  // delegated so each region is a walked field of its own.
  const catalog = freshCatalog();
  const delegation = catalog.struct_field.find(
    (entry) => entry.struct === "ProjectTypeStore" && entry.field === "semantic_graph",
  );
  assert.equal(delegation?.disposition, "delegated", "pre-state: the graph store is walked");
  catalog.struct_field = catalog.struct_field.filter(
    (entry) => entry !== delegation && entry.struct !== "SemanticGraphStore",
  );
  for (const id of ["semantic_graph_node_arena", "family_candidate_slots"])
    catalog.allocation_class.find((entry) => entry.id === id).covers_fields = [
      "ProjectTypeStore.semantic_graph",
    ];
  assert.ok(
    !catalog.struct_field.some((entry) => entry.struct === "SemanticGraphStore"),
    "post-state: the graph store is still dispositioned",
  );
  const errors = validate(catalog);
  refusedBecause(
    errors,
    "struct field ProjectTypeStore.semantic_graph: charged by 2 allocation classes",
  );
  refusedBecause(errors, "never reaches SemanticGraphStore");
});

test("an intern table the semantic graph store retains cannot go unowned", () => {
  const fields = structFields(
    "crates/verter_session/src/semantic_query_memo/mod.rs",
    "SemanticGraphStore",
  );
  assert.ok(fields.includes("relation_proof_table"), "pre-state: the table is a production field");
  const catalog = freshCatalog();
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter(
    (row) => row.id !== "relation_proof_intern_table",
  );
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "SemanticGraphStore field relation_proof_table is retained but has no allocation class and no disposition",
  );
});

// ── embedded classes ─────────────────────────────────────────────────────

test("a class stored inside a charged payload with no declared host is charged twice and refused", () => {
  const catalog = freshCatalog();
  const memo = catalog.allocation_class.find((row) => row.id === "declaration_body_memo");
  assert.deepEqual(memo.covers_fields, [], "pre-state: the memo charges no walked field");
  const before = catalog.embedding.length;
  catalog.embedding = catalog.embedding.filter((row) => row.embedded !== "declaration_body_memo");
  assert.equal(
    catalog.embedding.length,
    before - 1,
    "post-state: the embedding row was not removed",
  );
  refusedBecause(
    validate(catalog),
    "allocation class declaration_body_memo: a cache-owned class that charges no walked field lives inside some other class's charged payload, and no [[embedding]] row names that host",
  );
});

test("an embedding whose host charges no field carves nothing out and is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.embedding.find((entry) => entry.embedded === "declaration_body_memo");
  assert.equal(row.host, "file_artifact_store");
  row.host = "public_result_copy";
  assert.equal(row.host, "public_result_copy", "post-state: the mutation did not apply");
  refusedBecause(
    validate(catalog),
    "embedding declaration_body_memo in public_result_copy: the host charges no walked field",
  );
});

test("embedding a class that already charges a field is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.embedding.find((entry) => entry.embedded === "declaration_body_memo");
  row.embedded = "retained_parse_snapshot";
  assert.equal(row.embedded, "retained_parse_snapshot", "post-state: the mutation did not apply");
  refusedBecause(
    validate(catalog),
    "only a cache-owned class that charges no walked field is embedded; retained_parse_snapshot is cache_owned and charges 1 fields",
  );
});

test("an embedding whose held_at anchor no longer resolves is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.embedding.find((entry) => entry.embedded === "declaration_body_memo");
  const before = row.held_at;
  row.held_at = before.replace("decl_bodies:", "decl_bodies_moved:");
  assert.notEqual(row.held_at, before, "post-state: the mutation did not apply");
  refusedBecause(
    validate(catalog),
    "symbol decl_bodies_moved: Arc<crate::decl_body_memo::DeclBodyMemo> no longer occurs",
  );
});

test("deleting the caller-owned copy class is refused", () => {
  const catalog = freshCatalog();
  assert.ok(catalog.allocation_class.some((row) => row.id === "public_result_copy"));
  catalog.allocation_class = catalog.allocation_class.filter(
    (row) => row.id !== "public_result_copy",
  );
  // The shared-pin class still carries externally_pinned ownership, so the
  // ownership check alone would let this through.
  assert.ok(catalog.allocation_class.some((row) => row.ownership === "externally_pinned"));
  refusedBecause(validate(catalog), "the charter requires allocation class public_result_copy");
});

test("a field disposition the struct no longer has is refused", () => {
  const catalog = freshCatalog();
  const fields = structFields(
    "crates/verter_session/src/project_type_store.rs",
    "ProjectTypeStore",
  );
  assert.ok(fields.includes("counters"), "pre-state: the field list resolves from source");
  catalog.struct_field.push({
    struct: "ProjectTypeStore",
    field: "a_field_that_was_deleted",
    disposition: "non_retaining",
    reason: "A stale disposition for a field the store no longer declares at all.",
  });
  refusedBecause(validate(catalog), "ProjectTypeStore has no such production field");
});

test("declaring a charged field non-retaining as well is refused", () => {
  const catalog = freshCatalog();
  catalog.struct_field.push({
    struct: "ProjectTypeStore",
    field: "routes",
    disposition: "non_retaining",
    reason: "Claiming a real cache holds no payload while a class also charges it.",
  });
  refusedBecause(validate(catalog), "it is one or the other");
});

test("a class claiming its charge is instrumented without a byte gauge is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.allocation_class.find(
    (entry) => entry.id === "materialize_structure_candidates",
  );
  assert.equal(row.charge_observability, "proxy_instrumented", "pre-state: it is a proxy only");
  row.charge_observability = "charge_instrumented";
  row.gap = "none";
  assert.equal(row.charge_observability, "charge_instrumented");
  refusedBecause(validate(catalog), "requires a cited Bytes or Gauge row with a complete emitter");
});

test("a class recorded uninstrumented while citing metric rows is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.allocation_class.find(
    (entry) => entry.id === "materialize_structure_candidates",
  );
  assert.ok(row.metric_rows.length > 0, "pre-state: it cites a site");
  row.charge_observability = "uninstrumented";
  refusedBecause(validate(catalog), "cite none or record it as proxy_instrumented");
});

test("a class hiding its observability gap is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.allocation_class.find((entry) => entry.id === "public_result_copy");
  assert.equal(row.charge_observability, "uninstrumented");
  assert.notEqual(row.gap, "none", "pre-state: the gap is stated");
  row.gap = "none";
  refusedBecause(validate(catalog), "must state the observability gap it leaves");
});

// ── metric rows ──────────────────────────────────────────────────────────

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

test("an incomplete emitter recorded as complete is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.metric_row.find((entry) => entry.emitter_status === "partial");
  assert.ok(row, "pre-state: the contract must record at least one partial emitter");
  row.emitter_status = "complete";
  assert.equal(row.emitter_status, "complete");
  refusedBecause(validate(catalog), "a complete emitter may not record a gap");
});

test("the admission and arena rows are recorded partial, not complete", () => {
  // These three sites do not measure the roles the budget reads them for:
  // the return-only counter is raised for every ReturnOnly decision
  // including broken taint while budget and cancellation paths bypass it
  // entirely, and the two arena rows are recorded once at parse time
  // rather than tracked across the pin transfer and release. Recording
  // them complete would make the accounting look instrumented.
  const catalog = freshCatalog();
  for (const id of [
    "session.cache_admit_return_only",
    "session.parse_arena_used",
    "session.parse_arena_capacity",
  ]) {
    const row = catalog.metric_row.find((entry) => entry.id === id);
    assert.ok(row, `${id} must be declared`);
    assert.equal(row.emitter_status, "partial", `${id} must not claim a complete emitter`);
    assert.notEqual(row.gap, "none", `${id} must state its gap`);
  }
});

// ── fixtures, edits and configuration ────────────────────────────────────

test("an omitted carrier is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_fixture.length;
  catalog.workload_fixture = catalog.workload_fixture.filter(
    (fixture) => fixture.carrier !== "svelte",
  );
  assert.ok(catalog.workload_fixture.length < before);
  refusedBecause(validate(catalog), "the svelte carrier is not covered");
});

test("an omitted oversize input is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_fixture.length;
  catalog.workload_fixture = catalog.workload_fixture.filter(
    (fixture) => fixture.role !== "oversize_carrier",
  );
  assert.equal(catalog.workload_fixture.length, before - 1, "post-state: the row was not removed");
  refusedBecause(validate(catalog), "no oversize_carrier fixture is declared");
});

test("an oversize template without its declared expansion markers is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_materialization.oversize_ordinal_placeholder;
  catalog.workload_materialization.oversize_ordinal_placeholder = "__NOT_IN_THE_TEMPLATE__";
  assert.notEqual(catalog.workload_materialization.oversize_ordinal_placeholder, before);
  refusedBecause(validate(catalog), "does not contain the declared ordinal placeholder");
});

test("claiming the oversize entry size has been measured is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.workload_materialization.oversize_size_claim, "unmeasured");
  catalog.workload_materialization.oversize_size_claim = "exceeds_pressure_entry_cap";
  refusedBecause(validate(catalog), "oversize_size_claim: expected constant");
});

test("an edit delta whose find text does not occur is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_edit_delta[0];
  const before = delta.find;
  delta.find = "export type DensityThatWasNeverWritten = never;";
  assert.notEqual(delta.find, before);
  refusedBecause(validate(catalog), "does not occur in");
});

test("an edit delta whose find text is ambiguous is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_edit_delta.find((entry) => entry.id === "card_props_widen");
  const before = delta.find;
  // Two occurrences: neither the edit nor its inverse is well defined.
  delta.find = "import";
  assert.notEqual(delta.find, before);
  refusedBecause(validate(catalog), "so neither the edit nor its inverse is unambiguous");
});

test("an edit delta whose replacement is already present is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_edit_delta.find((entry) => entry.id === "card_props_widen");
  const before = delta.replace;
  delta.replace = "const accent = defaultTheme.accent;";
  assert.notEqual(delta.replace, before);
  refusedBecause(validate(catalog), "so the revert would not restore the original bytes");
});

test("an editable template with no declared edit is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_edit_delta.length;
  catalog.workload_edit_delta = catalog.workload_edit_delta.filter(
    (entry) => entry.template !== "svelte/panel.svelte",
  );
  assert.ok(catalog.workload_edit_delta.length < before, "post-state: no delta was removed");
  refusedBecause(validate(catalog), "template svelte/panel.svelte has no declared edit");
});

test("a configuration delta naming a setting the projects do not have is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_configuration_delta[0];
  const before = delta.setting;
  delta.setting = "compilerOptions.aSettingNobodyConfigured";
  assert.notEqual(delta.setting, before);
  refusedBecause(validate(catalog), "does not exist in the materialized baseline configuration");
});

test("a configuration delta misstating its baseline value is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_configuration_delta.find(
    (entry) => entry.id === "strict_null_checks",
  );
  assert.equal(delta.baseline_value, "false", "pre-state: the materialized baseline value");
  delta.baseline_value = "true";
  refusedBecause(validate(catalog), "but the materialized configuration holds");
});

test("a configuration delta that changes nothing is refused", () => {
  const catalog = freshCatalog();
  const delta = catalog.workload_configuration_delta.find(
    (entry) => entry.id === "strict_null_checks",
  );
  delta.applied_value = delta.baseline_value;
  refusedBecause(validate(catalog), "applying the baseline value changes nothing");
});

test("a brace-expanded include pattern is refused", () => {
  // TypeScript performs no brace expansion, so such an entry owns nothing
  // and the configured project would not own its carriers at all.
  const catalog = freshCatalog();
  const before = catalog.workload_materialization.project_config_baseline;
  catalog.workload_materialization.project_config_baseline = before.replace(
    '"vue/*.vue","svelte/*.svelte"',
    '"*.{vue,svelte}"',
  );
  assert.notEqual(catalog.workload_materialization.project_config_baseline, before);
  refusedBecause(validate(catalog), "uses brace expansion, which TypeScript does not perform");
});

// ── workload coverage ────────────────────────────────────────────────────

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

test("an omitted lifecycle class is refused even when its declaration is removed too", () => {
  // The earlier version of this control only removed the ACTIONS, which a
  // catalog-driven coverage check would still have caught. This one also
  // removes the requirement row, which is the shape that previously
  // validated clean: the required population must come from the resource
  // contract, not from the submission.
  const catalog = freshCatalog();
  const before = catalog.workload_step.length;
  catalog.workload_step = catalog.workload_step.filter((step) => step.kind !== "request_cancelled");
  assert.ok(catalog.workload_step.length < before, "post-state: the step rows did not change");
  for (const step of catalog.workload_step) if (step.kind === "request_cold") step.count += 4;
  catalog.workload_class = catalog.workload_class.filter((row) => row.id !== "cancellation");
  catalog.workload_kind = catalog.workload_kind.filter((row) => row.kind !== "request_cancelled");
  assert.ok(
    !catalog.workload_class.some((row) => row.id === "cancellation"),
    "post-state: the class declaration was not removed",
  );
  refusedBecause(
    validate(catalog),
    "the resource contract requires lifecycle class cancellation, which this catalog does not declare",
  );
});

test("optionalizing a required lifecycle class is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.workload_class.find((entry) => entry.id === "oversized_entry");
  assert.equal(row.required, true, "pre-state: the boundary is required");
  row.required = false;
  assert.equal(row.required, false);
  refusedBecause(validate(catalog), "may not be declared optional");
});

test("a same-key-only workload is refused", () => {
  const catalog = freshCatalog();
  assert.ok(catalog.workload.min_distinct_query_identities >= 3000);
  const cold = catalog.workload_step.filter((step) => step.kind === "request_cold");
  assert.equal(cold.length, 2, "pre-state: both cycles mint cold identities");
  assert.ok(cold.every((step) => step.count === 30));
  // Collapse each tranche to a handful of cold identities and give the
  // freed actions to warm requests, which re-enter that small pool. The
  // tranche still runs 100 actions; it just stops covering distinct keys.
  for (const step of cold) step.count = 5;
  for (const cycle of ["a", "b"])
    catalog.workload_step.find(
      (step) => step.cycle === cycle && step.kind === "request_warm",
    ).count += 25;
  assert.ok(
    cold.every((step) => step.count === 5),
    "post-state: the mutation did not apply",
  );
  refusedBecause(validate(catalog), "same-key repetition cannot satisfy this workload");
});

test("dropping every malformed fixture is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload_fixture.filter((row) => row.health === "malformed").length;
  assert.ok(before > 0, "pre-state: malformed fixtures exist");
  for (const row of catalog.workload_fixture)
    if (row.health === "malformed") row.health = "healthy";
  assert.equal(catalog.workload_fixture.filter((row) => row.health === "malformed").length, 0);
  refusedBecause(validate(catalog), "fixtures declare no malformed carrier");
});

test("failed construction against a healthy fixture is refused", () => {
  // The expander cannot emit this, so the invariant is driven directly.
  const catalog = freshCatalog();
  const healthy = catalog.workload_fixture.find((row) => row.path === "vue/card.vue");
  assert.equal(healthy.health, "healthy", "pre-state: the chosen template is healthy");
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_failed_construction",
      template: "vue/card.vue",
      instance: "r/t000/p/vue/card-00.vue",
      query: "component_meta:r/t000/p/vue/card-00.vue",
      cancel_at: "none",
      result_class: "failed",
    },
  ];
  refusedBecause(
    coverageErrors(catalog, rows),
    "failed construction must run against a malformed fixture",
  );
});

test("requesting a shared module instead of a carrier is refused", () => {
  // Shared modules are edit targets. The request API takes a carrier file,
  // so a manifest that requested a module would not be executable.
  const catalog = freshCatalog();
  const module = catalog.workload_fixture.find((row) => row.path === "shared/props-base.ts");
  assert.equal(module.role, "module", "pre-state: it is a module, not a carrier");
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_cold",
      template: "shared/props-base.ts",
      instance: "r/t000/p/shared/props-base.ts",
      query: "component_meta:r/t000/p/shared/props-base.ts",
      cancel_at: "none",
      result_class: "complete",
    },
  ];
  refusedBecause(
    coverageErrors(catalog, rows),
    "targets a shared module, which the request API does not accept",
  );
});

/** The committed manifest's actions, expanded from the catalog. */
function committedActions(catalog) {
  return expandWorkload(workloadSpec(catalog));
}

test("every healthy carrier, Svelte included, goes through the edit lifecycle", () => {
  const catalog = freshCatalog();
  const actions = committedActions(catalog);
  assert.deepEqual(ingredientUseErrors(catalog, actions), []);
  const edited = new Set(actions.filter((row) => row.kind === "edit").map((row) => row.template));
  const requestedAfter = new Set(
    actions.filter((row) => row.kind === "request_after_edit").map((row) => row.template),
  );
  for (const fixture of catalog.workload_fixture)
    if (fixture.role === "carrier" && fixture.health === "healthy") {
      assert.ok(edited.has(fixture.path), `${fixture.path} is never edited`);
      assert.ok(
        requestedAfter.has(fixture.path),
        `${fixture.path} is never requested after an edit`,
      );
    }
});

test("a declared edit delta the manifest never applies is refused", () => {
  const catalog = freshCatalog();
  const actions = committedActions(catalog);
  const isSvelteEdit = (row) => row.kind === "edit" && row.template.startsWith("svelte/");
  assert.ok(actions.some(isSvelteEdit), "pre-state: the committed manifest edits a Svelte carrier");
  // The shape a Vue-only edit selection produces: no Svelte carrier is ever
  // an edit target, so the Svelte deltas are declared and never applied.
  const vueOnly = actions.filter((row) => !isSvelteEdit(row));
  assert.ok(!vueOnly.some(isSvelteEdit), "post-state: the Svelte edits were not removed");
  refusedBecause(
    ingredientUseErrors(catalog, vueOnly),
    "edit delta panel_props_widen is declared but the manifest never applies it",
  );
});

/** Move every `kind` row of each `cycle` tranche to just before its first `before` row. */
function hoistBefore(actions, cycle, kind, before) {
  const out = [];
  for (const tranche of new Set(actions.map((row) => row.tranche))) {
    const rows = actions.filter((row) => row.tranche === tranche);
    if (rows[0].cycle !== cycle) {
      out.push(...rows);
      continue;
    }
    const moved = rows.filter((row) => row.kind === kind);
    const rest = rows.filter((row) => row.kind !== kind);
    const at = rest.findIndex((row) => row.kind === before);
    out.push(...rest.slice(0, at), ...moved, ...rest.slice(at));
  }
  return out;
}

test("a close claiming active readers after every hold was released is refused", () => {
  const catalog = freshCatalog();
  const actions = committedActions(catalog);
  const tranche = actions.find((row) => row.cycle === "b").tranche;
  const rows = actions.filter((row) => row.tranche === tranche);
  assert.deepEqual(trancheErrors(catalog, rows), [], "pre-state: the cycle-b tranche is clean");
  assert.ok(
    rows
      .filter((row) => row.kind === "close_project")
      .every((row) => row.result_class === "closed_with_active_readers"),
    "pre-state: cycle-b closes claim active readers",
  );
  const released = hoistBefore(rows, "b", "release_result", "close_project");
  const firstOf = (kind) => released.findIndex((row) => row.kind === kind);
  assert.ok(
    firstOf("release_result") < firstOf("close_project"),
    "post-state: the releases do not precede the closes",
  );
  refusedBecause(trancheErrors(catalog, released), "claims active readers, but no result held on");

  // Across the whole manifest the boundary is then observed nowhere, so the
  // label alone no longer counts as covering it.
  refusedBecause(
    coverageErrors(catalog, hoistBefore(actions, "b", "release_result", "close_project")),
    "required lifecycle class project_close_with_active_readers is not covered",
  );
});

test("a close labelled quiescent while results are still held is refused", () => {
  const catalog = freshCatalog();
  const actions = committedActions(catalog);
  const tranche = actions.find((row) => row.cycle === "a").tranche;
  const rows = actions.filter((row) => row.tranche === tranche);
  assert.deepEqual(trancheErrors(catalog, rows), [], "pre-state: the cycle-a tranche is clean");
  const closedEarly = hoistBefore(rows, "a", "close_project", "release_result");
  const firstOf = (kind) => closedEarly.findIndex((row) => row.kind === kind);
  assert.ok(firstOf("close_project") < firstOf("release_result"), "post-state: no close moved");
  refusedBecause(trancheErrors(catalog, closedEarly), "is labelled quiescent while");
});

test("an oversized request against an ordinary carrier is refused", () => {
  const catalog = freshCatalog();
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_oversized",
      template: "vue/card.vue",
      instance: "r/t000/p/vue/card-00.vue",
      query: "component_meta:r/t000/p/vue/card-00.vue",
      cancel_at: "none",
      result_class: "complete",
    },
  ];
  refusedBecause(
    coverageErrors(catalog, rows),
    "the oversized-entry boundary must run against the oversize input",
  );
});

test("an unbound negative-control command is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.command.find((entry) => entry.kind === "negative_control");
  assert.ok(row?.bound === true, "pre-state: the negative controls must start bound");
  row.bound = false;
  row.lane = "unbound";
  delete row.gate_profile;
  delete row.ci_job;
  delete row.ci_filter;
  refusedBecause(validate(catalog), "no bound negative-control command is declared");
});

test("a validate command moved to a local lane is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.command.find((entry) => entry.kind === "validate");
  assert.equal(row?.lane, "required", "pre-state: the validator runs in a required lane");
  row.lane = "local";
  delete row.gate_profile;
  delete row.ci_job;
  delete row.ci_filter;
  assert.equal(row.ci_job, undefined);
  refusedBecause(validate(catalog), "a validate command must run in a required lane");
});

test("a required command its gate profile no longer runs is refused", (t) => {
  const root = mirrorPackage(t, "mem0-profile-");
  assert.deepEqual(validate(loadCatalog(root), root), [], "pre-state: the mirror validates");
  const profiles = path.join(root, "catalogs", "gate-profiles.toml");
  const command = '"node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs", ';
  const original = fs.readFileSync(profiles, "utf8");
  assert.equal(original.split(command).length - 1, 1, "pre-state: the profile runs it once");
  fs.writeFileSync(profiles, original.replace(command, ""), "utf8");
  assert.equal(
    fs.readFileSync(profiles, "utf8").split(command).length - 1,
    0,
    "post-state: the plant did not apply",
  );
  refusedBecause(validate(loadCatalog(root), root), "gate profile docs-domain does not run");
});

test("a required command the CI job no longer issues is refused", (t) => {
  const workflowFile = mutatedWorkflow(
    t,
    "node roadmap/0.1.0-tama/tools/validate-semantic-memory-budget.mjs",
    "true",
  );
  refusedBecause(
    validate(freshCatalog(), PACKAGE_ROOT, { workflowFile }),
    "command budget_validate: job tama-roadmap does not run",
  );
});

test("a required command whose job is not gated on its declared filter is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.command.find((entry) => entry.id === "budget_negative_controls");
  assert.equal(row.ci_filter, "tama", "pre-state");
  row.ci_filter = "rust";
  assert.equal(row.ci_filter, "rust");
  refusedBecause(validate(catalog), "is not gated on the rust trigger filter it declares");
});

test("a cited crate the trigger filter stops covering is refused", (t) => {
  // The scheduler owns a charged allocation class, so a change there has to
  // re-run the job that re-resolves that owner.
  const workflowFile = mutatedWorkflow(t, "- 'crates/verter_scheduler/src/**'", "");
  refusedBecause(
    validate(freshCatalog(), PACKAGE_ROOT, { workflowFile }),
    "reads crates/verter_scheduler/src/scheduler.rs, which no tama trigger pattern covers",
  );
});

test("the contract's own sources the trigger filter stops covering are refused", (t) => {
  // Narrowing the package pattern must not leave the catalog, schema, gate
  // profiles and fixtures with no pattern while the validator keeps passing.
  const toolsOnly = mutatedWorkflow(
    t,
    "- 'roadmap/0.1.0-tama/**'",
    "- 'roadmap/0.1.0-tama/tools/**'",
  );
  const narrowed = validate(freshCatalog(), PACKAGE_ROOT, { workflowFile: toolsOnly });
  for (const relative of [
    "catalogs/semantic-memory-budget.toml",
    "schemas/semantic-memory-budget.schema.json",
    "catalogs/gate-profiles.toml",
    "catalogs/semantic-memory-workload/sources/",
  ])
    refusedBecause(narrowed, `reads roadmap/0.1.0-tama/${relative}`);
  // And the other way: the modules implementing the checks are inputs too.
  const catalogsOnly = mutatedWorkflow(
    t,
    "- 'roadmap/0.1.0-tama/**'",
    "- 'roadmap/0.1.0-tama/catalogs/**'",
  );
  const withoutTools = validate(freshCatalog(), PACKAGE_ROOT, { workflowFile: catalogsOnly });
  for (const relative of [
    "tools/semantic-memory-budget.mjs",
    "tools/semantic-memory-workload.mjs",
    "tools/validate-semantic-memory-budget.mjs",
    "tools/semantic-memory-budget.test.mjs",
  ])
    refusedBecause(withoutTools, `reads roadmap/0.1.0-tama/${relative},`);
});

test("a module the validation only imports is still held to the trigger filter", (t) => {
  // The file-granularity narrowing the filter already uses for other
  // scripts: enumerate the modules the validation names directly and drop
  // the package pattern. The schema check the entry script hands in, and
  // the helpers the named modules import, are executed on every run too.
  const named = [
    "semantic-memory-budget.mjs",
    "semantic-memory-workload.mjs",
    "closure-register.mjs",
    "toml.mjs",
    "validate-semantic-memory-budget.mjs",
    "semantic-memory-budget.test.mjs",
  ];
  const indent = "\n              ";
  const enumerated = mutatedWorkflow(
    t,
    "- 'roadmap/0.1.0-tama/**'",
    [
      "- 'roadmap/0.1.0-tama/catalogs/**'",
      "- 'roadmap/0.1.0-tama/schemas/**'",
      ...named.map((file) => `- 'roadmap/0.1.0-tama/tools/${file}'`),
    ].join(indent),
  );
  const errors = validate(freshCatalog(), PACKAGE_ROOT, { workflowFile: enumerated });
  for (const file of ["lib.mjs", "ledger.mjs", "closure-register.pins.mjs"])
    refusedBecause(errors, `reads roadmap/0.1.0-tama/tools/${file},`);
  for (const file of named)
    assert.ok(
      !errors.some((error) => error.includes(`reads roadmap/0.1.0-tama/tools/${file},`)),
      `${file} is enumerated, so it must not be reported uncovered`,
    );
});

test("an anchor citing a path outside the trigger filter is refused", () => {
  const catalog = freshCatalog();
  const row = catalog.allocation_class.find((entry) => entry.id === "route_db");
  const uncovered = "crates/verter_bench/Cargo.toml";
  assert.ok(fs.existsSync(path.join(REPO_ROOT, uncovered)), "pre-state: the path exists");
  row.producers = [...row.producers, uncovered];
  assert.ok(row.producers.includes(uncovered));
  refusedBecause(validate(catalog), `reads ${uncovered}, which no tama trigger pattern covers`);
});

test("a stale manifest pin is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload.manifest_sha256;
  catalog.workload.manifest_sha256 = `${"0".repeat(63)}1`;
  assert.notEqual(catalog.workload.manifest_sha256, before);
  refusedBecause(validate(catalog), "manifest digest recomputes to");
});

test("a configuration group that is not a pair is refused", () => {
  const catalog = freshCatalog();
  assert.equal(catalog.workload.config_pair_size, 2, "pre-state: a configuration group is a pair");
  catalog.workload.config_pair_size = 4;
  assert.equal(catalog.workload.config_pair_size, 4, "post-state: the group size did not change");
  // Four configuration actions divide evenly by four, so the whole-number
  // check alone admits the value while the expander still emits one apply
  // and three reverts of the same delta. Both halves refuse it: the schema
  // pins the value, and the expander's own shape check rejects it without
  // the schema's help.
  const errors = validate(catalog);
  refusedBecause(errors, "config_pair_size: expected constant 2");
  refusedBecause(errors, "config_pair_size must be 2, got 4");
});

test("a step naming an inherited object property is refused", () => {
  const catalog = freshCatalog();
  const step = catalog.workload_step.find(
    (row) => row.cycle === "a" && row.kind === "request_warm",
  );
  assert.ok(step, "pre-state: cycle a declares a warm request step");
  assert.ok(
    !catalog.workload_kind.some((row) => row.kind === "constructor"),
    "pre-state: constructor is not a declared kind",
  );
  // The hazard this control pins. The projected kinds map is a plain
  // object, so `constructor` resolves through `Object.prototype` even though
  // no row declares it, and the schema's kind pattern admits the name — a
  // truthiness lookup would accept the step and expand it into an action
  // whose class and disposition are undefined.
  const projected = workloadSpec(catalog).kinds;
  assert.ok(projected.constructor, "pre-state: the inherited lookup does resolve");
  assert.equal(Object.hasOwn(projected, "constructor"), false);

  step.kind = "constructor";
  assert.equal(step.kind, "constructor", "post-state: the step kind did not change");
  refusedBecause(validate(catalog), "step names unknown kind constructor");
});

// ── identifier separators ────────────────────────────────────────────────

test("a dotted identifier whose separator is not a dot is refused", () => {
  // An unescaped `.` in a JSON-schema pattern is the any-character
  // metacharacter, so each value below satisfies the UNESCAPED form of its
  // own pattern while separating its parts with something that is not a
  // dot. Each assertion names the escaped pattern, so this control fails if
  // any of the three separators stops being a literal dot.
  const catalog = freshCatalog();
  const derivation = catalog.limit_derivation[0];
  const observation = catalog.memory_observation[0];
  const delta = catalog.workload_configuration_delta[0];
  assert.ok(derivation.id.includes("."), "pre-state: the derivation id is dotted");
  assert.ok(observation.id.includes("."), "pre-state: the observation id is dotted");
  assert.ok(delta.setting.includes("."), "pre-state: the setting path is dotted");

  derivation.id = derivation.id.replace(".", "X");
  observation.id = observation.id.replace(".", "X");
  delta.setting = delta.setting.replace(".", " ");
  assert.ok(!derivation.id.includes("."), "post-state: the derivation separator did not change");
  assert.ok(!observation.id.includes("."), "post-state: the observation separator did not change");
  assert.ok(!delta.setting.includes("."), "post-state: the setting separator did not change");

  const errors = validate(catalog);
  refusedBecause(errors, String.raw`string does not match ^(normal|pressure)\.[a-z][a-z0-9_]*$`);
  refusedBecause(errors, String.raw`string does not match ^memory\.[a-z][a-z0-9_]*$`);
  refusedBecause(
    errors,
    String.raw`string does not match ^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)*$`,
  );
});

// ── per-tranche control restoration ──────────────────────────────────────

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
  // Naming the refusal, not merely counting errors: the extra cold request
  // this mutation adds to keep the tranche whole also exhausts the carrier
  // pool, so a regressed pairing check would still leave an error behind and
  // an `errors.length > 0` assertion would stay green over it.
  refusedBecause(validate(catalog), "cycle a does not revert exactly the edits it applies");
});

/**
 * A minimal tranche the invariant checkers accept, so each control below
 * can break exactly one thing.
 */
function controlTranche(overrides = []) {
  const base = [
    {
      seq: 1,
      tranche: 0,
      kind: "sample",
      project: "p",
      instance: "none",
      query: "none",
      delta: "none",
      overlap_group: "none",
      cache_disposition: "no_admission",
      result_class: "sampled",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "control_checkpoint",
      project: "p",
      instance: "none",
      query: "none",
      delta: "none",
      overlap_group: "none",
      cache_disposition: "no_admission",
      result_class: "control_restored",
    },
  ];
  return [...overrides, ...base].map((row, index) => ({ ...row, seq: index + 1 }));
}

test("a configuration revert on a different project than its apply is refused", () => {
  // The frozen expander cannot produce this, which is exactly why the
  // invariant is exercised directly: a check nothing can fail is not a
  // check. This is the shape the previous manifest actually had.
  const paired = controlTranche([
    {
      seq: 0,
      tranche: 0,
      kind: "config_change",
      project: "p_a",
      instance: "none",
      query: "none",
      delta: "config:strict_null_checks:p_a:apply",
      overlap_group: "none",
      cache_disposition: "no_admission",
      result_class: "configuration_applied",
    },
    {
      seq: 0,
      tranche: 0,
      kind: "config_change",
      project: "p_a",
      instance: "none",
      query: "none",
      delta: "config:strict_null_checks:p_a:revert",
      overlap_group: "none",
      cache_disposition: "no_admission",
      result_class: "configuration_reverted",
    },
  ]);
  const pairedErrors = trancheErrors(
    { measurement: { sampling_interval_actions: paired.length } },
    paired,
  ).filter((error) => error.includes("configuration"));
  assert.deepEqual(pairedErrors, [], "pre-state: a same-project pair is accepted");

  const mispaired = paired.map((row) =>
    row.delta.endsWith(":revert")
      ? { ...row, project: "p_b", delta: "config:strict_null_checks:p_b:revert" }
      : row,
  );
  assert.notEqual(mispaired[1].project, mispaired[0].project, "post-state: the mutation applied");
  const errors = trancheErrors(
    { measurement: { sampling_interval_actions: mispaired.length } },
    mispaired,
  );
  refusedBecause(errors, "which this tranche never applied there");
  refusedBecause(errors, "not exactly the ones it reverts, on the same projects");
});

test("a configuration action whose delta names a project it is not recorded against is refused", () => {
  const rows = controlTranche([
    {
      seq: 0,
      tranche: 0,
      kind: "config_change",
      project: "p_a",
      instance: "none",
      query: "none",
      delta: "config:lib_target:p_b:apply",
      overlap_group: "none",
      cache_disposition: "no_admission",
      result_class: "configuration_applied",
    },
  ]);
  refusedBecause(
    trancheErrors({ measurement: { sampling_interval_actions: rows.length } }, rows),
    "but is recorded against p_a",
  );
});

test("an overlap group spanning two keys is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "must_construct",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:b",
      overlap_group: "og0.0",
      cache_disposition: "join_inflight",
    },
  ];
  assert.notEqual(rows[0].query, rows[1].query, "pre-state: the members demand different keys");
  refusedBecause(overlapGroupErrors(0, rows), "collapses onto nothing");
});

test("an overlap group with no leader is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "join_inflight",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "join_inflight",
    },
  ];
  refusedBecause(overlapGroupErrors(0, rows), "declares 0 leaders");
});

test("an overlap group whose members are not concurrent is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "must_construct",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_cold",
      query: "component_meta:z",
      overlap_group: "none",
      cache_disposition: "must_construct",
    },
    {
      seq: 3,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "join_inflight",
    },
  ];
  refusedBecause(overlapGroupErrors(0, rows), "is not contiguous");
});

test("an overlap group racing on an already-requested key is refused", () => {
  // Joining a COMPLETED result is not a singleflight join. This is the
  // shape the previous manifest had: overlapping requests reused the
  // finished cold pool.
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_cold",
      query: "component_meta:a",
      overlap_group: "none",
      cache_disposition: "must_construct",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "must_construct",
    },
    {
      seq: 3,
      tranche: 0,
      kind: "request_overlapping",
      query: "component_meta:a",
      overlap_group: "og0.0",
      cache_disposition: "join_inflight",
    },
  ];
  refusedBecause(overlapGroupErrors(0, rows), "already requested");
});

test("a tranche with no overlapping group is refused", () => {
  refusedBecause(overlapGroupErrors(0, []), "declares no overlapping-request group");
});

test("a post-mutation request that expects reuse is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "edit",
      instance: "r/t000/p/shared/m.ts",
      delta: "edit:d@r/t000/p/shared/m.ts",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_after_edit",
      instance: "r/t000/p/vue/c-00.vue",
      delta: "after:r/t000/p/shared/m.ts",
      cache_disposition: "reuse_if_valid",
    },
  ];
  refusedBecause(postMutationErrors(0, rows), "must recompute");
});

test("a post-mutation request placed after the revert is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "edit",
      instance: "r/t000/p/shared/m.ts",
      delta: "edit:d@r/t000/p/shared/m.ts",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "revert",
      instance: "r/t000/p/shared/m.ts",
      delta: "revert:d@r/t000/p/shared/m.ts",
    },
    {
      seq: 3,
      tranche: 0,
      kind: "request_after_edit",
      instance: "r/t000/p/vue/c-00.vue",
      delta: "after:r/t000/p/shared/m.ts",
      cache_disposition: "must_recompute",
    },
  ];
  refusedBecause(postMutationErrors(0, rows), "so nothing is invalidated");
});

test("a post-mutation request an edit cannot reach is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "edit",
      instance: "r/t000/p_a/shared/m.ts",
      delta: "edit:d@r/t000/p_a/shared/m.ts",
    },
    {
      seq: 2,
      tranche: 0,
      kind: "request_after_edit",
      instance: "r/t000/p_b/vue/c-00.vue",
      delta: "after:r/t000/p_a/shared/m.ts",
      cache_disposition: "must_recompute",
    },
  ];
  refusedBecause(postMutationErrors(0, rows), "does not reach");
});

test("a post-mutation request whose edit never happened is refused", () => {
  const rows = [
    {
      seq: 1,
      tranche: 0,
      kind: "request_after_edit",
      instance: "r/t000/p/vue/c-00.vue",
      delta: "after:r/t000/p/shared/m.ts",
      cache_disposition: "must_recompute",
    },
  ];
  refusedBecause(postMutationErrors(0, rows), "that never happened");
});

test("a tranche with no post-mutation request is refused", () => {
  refusedBecause(postMutationErrors(0, []), "declares no post-mutation request");
});

test("the committed manifest satisfies the per-invariant checkers it is validated by", () => {
  // The positive leg for the four checkers exercised negatively above: the
  // real manifest passes them, so their refusals discriminate rather than
  // firing on everything.
  const catalog = freshCatalog();
  const actions = expandWorkload({
    tranches: catalog.workload.tranches,
    tranche_actions: catalog.workload.tranche_actions,
    projects_per_tranche: catalog.workload.projects_per_tranche,
    overlap_group_size: catalog.workload.overlap_group_size,
    config_pair_size: catalog.workload.config_pair_size,
    instance_root: catalog.workload_materialization.instance_root,
    carrier_instances_per_tranche: catalog.workload_materialization.carrier_instances_per_tranche,
    projects: catalog.workload_project,
    fixtures: catalog.workload_fixture,
    steps: catalog.workload_step,
    configuration_deltas: catalog.workload_configuration_delta,
    edit_deltas: catalog.workload_edit_delta,
    cancellation_points: catalog.workload_cancellation_point,
    kinds: Object.fromEntries(catalog.workload_kind.map((row) => [row.kind, row])),
  });
  const first = actions.filter((action) => action.tranche === 0);
  assert.deepEqual(overlapGroupErrors(0, first), []);
  assert.deepEqual(postMutationErrors(0, first), []);
  assert.deepEqual(trancheErrors(catalog, actions), []);
  assert.deepEqual(coverageErrors(catalog, actions), []);
});

// ── on-disk controls ─────────────────────────────────────────────────────

test("an edited fixture on disk is refused", (t) => {
  // A real on-disk control: mirror the package's contract inputs into a
  // temporary root, change one byte of one frozen fixture, and confirm the
  // pin catches it. The in-memory controls above cannot prove the fixture
  // digests are read from disk at all; this one can.
  const root = mirrorPackage(t, "mem0-budget-");

  assert.deepEqual(
    validate(loadCatalog(root), root),
    [],
    "pre-state: the mirrored tree must validate before it is mutated",
  );

  const catalog = loadCatalog(root);
  const fixture = catalog.workload_fixture.find((entry) => entry.path === "vue/card.vue");
  const target = path.join(root, catalog.workload.fixture_root, fixture.path);
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
  const root = mirrorPackage(t, "mem0-expander-");

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
    overlap_group_size: catalog.workload.overlap_group_size,
    config_pair_size: catalog.workload.config_pair_size,
    instance_root: catalog.workload_materialization.instance_root,
    carrier_instances_per_tranche: catalog.workload_materialization.carrier_instances_per_tranche,
    projects: catalog.workload_project,
    fixtures: catalog.workload_fixture,
    steps: catalog.workload_step,
    configuration_deltas: catalog.workload_configuration_delta,
    edit_deltas: catalog.workload_edit_delta,
    cancellation_points: catalog.workload_cancellation_point,
    kinds: Object.fromEntries(catalog.workload_kind.map((row) => [row.kind, row])),
  };
  assert.equal(serializeManifest(expandWorkload(spec)), serializeManifest(expandWorkload(spec)));
});
