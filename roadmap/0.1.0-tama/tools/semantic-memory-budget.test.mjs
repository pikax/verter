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

import {
  coverageErrors,
  loadCatalog,
  loadSchema,
  overlapGroupErrors,
  PACKAGE_ROOT,
  postMutationErrors,
  projectTypeStoreFields,
  trancheErrors,
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

test("a pressure entry cap above the per-request cap makes the oversized boundary unreachable", () => {
  const catalog = freshCatalog();
  const before = catalog.budget.pressure.per_entry_admission_max_bytes;
  assert.equal(before, catalog.budget.pressure.per_request_active_max_bytes);
  catalog.budget.pressure.per_entry_admission_max_bytes = before * 2;
  assert.notEqual(catalog.budget.pressure.per_entry_admission_max_bytes, before);
  refusedBecause(validate(catalog), "the oversized-entry boundary is unreachable");
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
  // The omission check that the previous inventory did not have: the
  // required population is the store's own field list, so removing a row
  // cannot quietly shrink the contract.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find((row) => row.id === "route_db");
  assert.ok(victim, "pre-state: the route cache is inventoried");
  assert.deepEqual(victim.covers_store_fields, ["routes"]);
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter((row) => row.id !== "route_db");
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "ProjectTypeStore field routes is retained by the store but has no allocation class",
  );
});

test("deleting a class that covers no store field is refused", () => {
  // The store's field list cannot notice this one: retained parse
  // snapshots live on the lowering service, not on the store. The
  // charter's own named categories are the independent inventory for it.
  const catalog = freshCatalog();
  const victim = catalog.allocation_class.find((row) => row.id === "retained_parse_snapshot");
  assert.deepEqual(victim.covers_store_fields, [], "pre-state: it is not a store field");
  assert.equal(victim.ownership, "cache_owned");
  const before = catalog.allocation_class.length;
  catalog.allocation_class = catalog.allocation_class.filter(
    (row) => row.id !== "retained_parse_snapshot",
  );
  assert.equal(catalog.allocation_class.length, before - 1, "post-state: the row was not removed");
  refusedBecause(
    validate(catalog),
    "the charter requires allocation class retained_parse_snapshot, which this catalog does not declare",
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

test("a store field disposition the struct no longer has is refused", () => {
  const catalog = freshCatalog();
  const fields = projectTypeStoreFields();
  assert.ok(fields.includes("counters"), "pre-state: the field list resolves from source");
  catalog.store_field.push({
    field: "a_field_that_was_deleted",
    disposition: "non_retaining",
    reason: "A stale disposition for a field the store no longer declares at all.",
  });
  refusedBecause(validate(catalog), "ProjectTypeStore has no such field");
});

test("declaring a retained store field non-retaining while also charging it is refused", () => {
  const catalog = freshCatalog();
  catalog.store_field.push({
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
  refusedBecause(validate(catalog), "no bound negative-control command is declared");
});

test("a stale manifest pin is refused", () => {
  const catalog = freshCatalog();
  const before = catalog.workload.manifest_sha256;
  catalog.workload.manifest_sha256 = `${"0".repeat(63)}1`;
  assert.notEqual(catalog.workload.manifest_sha256, before);
  refusedBecause(validate(catalog), "manifest digest recomputes to");
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
  const errors = validate(catalog);
  assert.ok(errors.length > 0, `an unreverted edit must not validate clean:\n${errors.join("\n")}`);
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
