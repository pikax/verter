// Validation for the aggregate semantic memory budget and workload contract.
//
// What this validator is, and is not.
//
// It is a CITATION-INTEGRITY and ARITHMETIC check over a contract document.
// It re-resolves the symbols, files, digests and recorded measurements the
// contract cites against the working tree, re-derives every stated
// arithmetic identity, and re-expands the frozen workload to confirm it
// still covers what it claims. It is the same discipline the
// closure-register instrument already applies to its own cited fixtures
// and evidence anchors.
//
// It is NOT an architecture guard. It does not enforce any invariant on the
// code it reads; it only refuses to let this contract go on describing
// symbols, digests, measurements or arithmetic that no longer hold. A
// renamed owner is a contract-repin obligation, not a rule violation.
//
// Two of its checks are deliberately NOT driven by the submitted document,
// because a check whose required population comes from the submission it
// polices cannot detect an omission:
//
//   * the required lifecycle classes are transcribed here from the binding
//     resource contract, so dropping a class from the catalog fails;
//   * the retained-storage inventory is derived from the production FIELDS
//     of `VerterHost` in the source, and of every struct those fields
//     delegate to, so dropping an allocation class, adding a host-lifetime
//     field, or cutting a subsystem out of the walk fails.
//
// It also refuses to let the contract stop being enforced. A command
// declared to run in a required lane is resolved against the gate profile
// and the CI job it names, so deleting either binding fails; and every
// repository path this validator opens has to be covered by that job's
// trigger filter, so a change to a cited file cannot merge without the
// enforcing job running.
//
// One table is validated for the opposite reason. `[[memory_observation]]`
// records byte-valued memory the process really does report, which the
// work-site vocabulary cannot name; without it the contract would assert
// a stronger negative than the tree supports.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  CI_WORKFLOW,
  laneCommandLine,
  triggerCovers,
  triggerPaths,
  workflowJobs,
} from "./closure-register.mjs";
import {
  expandWorkload,
  MANIFEST_COLUMNS,
  serializeManifest,
} from "./semantic-memory-workload.mjs";
import { readToml } from "./toml.mjs";

export const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = path.resolve(PACKAGE_ROOT, "..", "..");

export const CATALOG_RELATIVE = "catalogs/semantic-memory-budget.toml";
export const SCHEMA_NAME = "semantic-memory-budget.schema.json";
const GATE_PROFILES_RELATIVE = "catalogs/gate-profiles.toml";
const AUDIT_SITE_SCHEMA = "crates/verter_audit/src/attribution/schema.rs";

/**
 * The root of the retained-state walk. Everything the host retains for its
 * lifetime hangs off this struct, so its production field list is where the
 * inventory's required population starts. The walk then follows every
 * `delegated` disposition into the struct that field holds.
 */
export const RETAINED_STATE_ROOT = Object.freeze({
  struct: "VerterHost",
  file: "crates/verter_session/src/lib.rs",
});

/**
 * Subsystems the walk MUST reach. A `delegated` disposition is what carries
 * the walk into a subsystem, so re-labelling one as non-retaining would
 * silently drop every cache behind it. These are the host-lifetime cache
 * owners, transcribed here for the same reason as the lists below.
 */
export const REQUIRED_RETAINED_STRUCTS = Object.freeze([
  "ProjectTypeStore",
  "SemanticGraphStore",
  "UnifiedResolverRuntime",
  "FallthroughResolverState",
  "FrameworkRegistration",
  "FrameworkScriptCaches",
]);

/** How a walked field that no allocation class charges may be accounted. */
export const FIELD_DISPOSITIONS = Object.freeze([
  "non_retaining",
  "delegated",
  "shared_reference",
  "outside_inventory",
]);

/**
 * The lifecycle population the workload MUST cover, transcribed from
 * `contracts/resource-and-finalization.md`: its required pressure
 * boundaries (one entry exceeding the cache cap; many distinct keys; many
 * project sessions sharing one process; edit/revert versions; concurrent
 * cold winners; cancelled and failed construction; closed projects with
 * and without active readers; held-then-released public results) and its
 * required manifest coverage (both Vue and Svelte, cold and warm requests,
 * high-cardinality keys, edit/revert, project open/close,
 * provider-independent configuration changes, overlapping requests, failed
 * construction and retained-result release).
 *
 * This list lives HERE, not in the catalog, so that removing a class from
 * the catalog is a validation failure rather than a smaller obligation.
 */
export const REQUIRED_LIFECYCLE_CLASSES = Object.freeze([
  "vue_carrier",
  "svelte_carrier",
  "cold_request",
  "warm_request",
  "high_cardinality_key",
  "edit",
  "revert",
  "post_mutation_request",
  "project_open",
  "concurrent_project_sessions",
  "project_close",
  "project_close_with_active_readers",
  "configuration_change",
  "overlapping_request",
  "cancellation",
  "failed_construction",
  "oversized_entry",
  "retained_result_hold",
  "retained_result_release",
  "sampling",
  "control_checkpoint",
]);

/**
 * The allocation classes the contract MUST carry, transcribed from this
 * node's charter: "Bind exact current producer/consumer symbols for parse
 * snapshots, graph/interner regions, fact signatures, cache candidates and
 * public result pins", plus the request-active classes the resource
 * contract's cache-owned / request-owned / externally-pinned split needs.
 *
 * The retained-state walk below is the independent inventory for
 * everything the host retains, but several of these classes are sub-regions
 * of one field or live on the request or at the public boundary, so the
 * walk alone would not notice their removal. This list lives HERE
 * for the same reason the lifecycle list does: an omission check whose
 * required population comes from the submission cannot detect an omission.
 */
export const REQUIRED_ALLOCATION_CLASSES = Object.freeze([
  // parse snapshots
  "retained_parse_snapshot",
  "declaration_body_memo",
  // graph and interner regions
  "semantic_graph_node_arena",
  "semantic_graph_family_memo",
  "identity_intern_pool",
  "dep_signature_intern_pool",
  // fact signatures
  "fact_read_set_signature",
  // cache candidates
  "family_candidate_slots",
  "component_meta_result_candidates",
  // request-active bytes
  "request_parse_arena",
  "request_supplied_block_content",
  // public result pins, shared and copied
  "public_result_pin",
  "public_result_copy",
]);

/** The budget limits every mode must carry, and every mode must explain. */
export const BUDGET_LIMITS = Object.freeze([
  "process_rss_max_bytes",
  "cache_owned_retained_max_bytes",
  "request_active_max_bytes",
  "external_pinned_backpressure_threshold_bytes",
  "allocator_slack_bytes",
  "per_entry_admission_max_bytes",
  "max_simultaneously_admitted_requests",
  "per_request_active_max_bytes",
]);

/** Digest form used for every pin in the catalog. */
export function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

/**
 * Every repository path the running validation opens, repository-relative
 * with forward slashes, or `null` outside a validation run. The trigger
 * coverage check is measured against this set rather than against a list
 * of the catalog's citing fields, so it cannot drift from what is read.
 */
let openedRepoPaths = null;

function recordOpened(relative) {
  if (openedRepoPaths) openedRepoPaths.add(relative);
}

/** The repository-relative form of an absolute path, or `null` outside it. */
function repoRelative(absolute) {
  const relative = path.relative(REPO_ROOT, absolute);
  if (!relative || path.isAbsolute(relative) || relative.startsWith("..")) return null;
  return relative.split(path.sep).join("/");
}

function readRepoFile(relative) {
  const bytes = fs.readFileSync(path.join(REPO_ROOT, relative));
  recordOpened(relative);
  return bytes;
}

function repoPathExists(relative) {
  const exists = fs.existsSync(path.join(REPO_ROOT, relative));
  if (exists) recordOpened(relative);
  return exists;
}

/**
 * Resolve one `path#Symbol` anchor. A bare path (no `#`) resolves when the
 * file or directory exists; an anchored one additionally requires the
 * symbol text to occur in the file.
 */
function anchorErrors(anchor, label) {
  const errors = [];
  const hash = anchor.indexOf("#");
  const relative = hash === -1 ? anchor : anchor.slice(0, hash);
  if (!repoPathExists(relative)) {
    errors.push(`${label}: anchor path does not resolve: ${relative}`);
    return errors;
  }
  if (hash === -1) return errors;
  const symbol = anchor.slice(hash + 1);
  const target = path.join(REPO_ROOT, relative);
  if (!fs.statSync(target).isFile()) {
    errors.push(`${label}: anchored symbol needs a file, got a directory: ${relative}`);
    return errors;
  }
  if (!readRepoFile(relative).toString("utf8").includes(symbol))
    errors.push(`${label}: symbol ${symbol} no longer occurs in ${relative}`);
  return errors;
}

/** Declared work-site ids and their units, read from the audit schema. */
export function declaredWorkSites() {
  const source = readRepoFile(AUDIT_SITE_SCHEMA).toString("utf8");
  const sites = new Map();
  const pattern = /^\s*(\w+)\s*=>\s*"([^"]+)",\s*\w+,\s*(\w+);/gmu;
  for (const match of source.matchAll(pattern))
    sites.set(match[2], { variant: match[1], unit: match[3] });
  return sites;
}

/**
 * The PRODUCTION fields of struct `name` in `file`, in source order.
 *
 * This is the INDEPENDENT retained-storage inventory: the contract has to
 * account for every field the walk reaches, so an allocation class cannot
 * be dropped without the field it covered becoming unaccounted, and a field
 * cannot be added without the contract going red. A field gated behind a
 * `cfg` naming `test` never exists in a shipped build, so it retains nothing
 * there and is not part of the population; a target gate such as
 * `not(target_arch = "wasm32")` is production and stays in.
 */
export function structFields(file, name) {
  const source = readRepoFile(file).toString("utf8");
  const header = new RegExp(String.raw`^(?:pub(?:\([^)]*\))?\s+)?struct\s+${name}\b[^;{]*\{`, "mu");
  const match = header.exec(source);
  if (!match) throw new Error(`struct ${name} not found in ${file}`);
  const start = match.index + match[0].length;
  const end = source.indexOf("\n}", start);
  if (end === -1) throw new Error(`struct ${name} in ${file} is unterminated`);
  const fields = [];
  let testOnly = false;
  for (const line of source.slice(start, end).split(/\r?\n/u)) {
    const cfg = /^ {4}#\[cfg\((.*)\)\]\s*$/u.exec(line);
    if (cfg) {
      testOnly = /\btest\b/u.test(cfg[1]);
      continue;
    }
    const field = /^ {4}(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:/u.exec(line);
    if (!field) continue;
    if (!testOnly) fields.push(field[1]);
    testOnly = false;
  }
  return fields;
}

/** Split a `path#Struct` delegate anchor. */
function delegateTarget(anchor) {
  const hash = anchor.indexOf("#");
  if (hash === -1) return null;
  return { file: anchor.slice(0, hash), struct: anchor.slice(hash + 1) };
}

/** Every `.rs` file reachable from a file or directory anchor. */
function rustSourcesUnder(relative) {
  const absolute = path.join(REPO_ROOT, relative);
  if (fs.statSync(absolute).isFile()) return [absolute];
  return fs
    .readdirSync(absolute, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
    .map((entry) => path.join(entry.parentPath ?? entry.path, entry.name));
}

/** The workload spec projection the expander consumes. */
export function workloadSpec(catalog) {
  const kinds = {};
  for (const row of catalog.workload_kind || [])
    kinds[row.kind] = {
      class: row.class,
      result_class: row.result_class,
      cache_disposition: row.cache_disposition,
    };
  const materialization = catalog.workload_materialization || {};
  return {
    tranches: catalog.workload?.tranches,
    tranche_actions: catalog.workload?.tranche_actions,
    projects_per_tranche: catalog.workload?.projects_per_tranche,
    overlap_group_size: catalog.workload?.overlap_group_size,
    config_pair_size: catalog.workload?.config_pair_size,
    instance_root: materialization.instance_root,
    carrier_instances_per_tranche: materialization.carrier_instances_per_tranche,
    projects: catalog.workload_project || [],
    fixtures: catalog.workload_fixture || [],
    steps: catalog.workload_step || [],
    configuration_deltas: catalog.workload_configuration_delta || [],
    edit_deltas: catalog.workload_edit_delta || [],
    cancellation_points: catalog.workload_cancellation_point || [],
    kinds,
  };
}

function budgetErrors(catalog) {
  const errors = [];
  const modes = catalog.budget || {};
  for (const [name, mode] of Object.entries(modes)) {
    if (!mode || typeof mode !== "object") continue;
    const partition =
      mode.cache_owned_retained_max_bytes +
      mode.request_active_max_bytes +
      mode.external_pinned_backpressure_threshold_bytes +
      mode.allocator_slack_bytes;
    if (partition !== mode.process_rss_max_bytes)
      errors.push(
        `budget.${name}: owned components sum to ${partition}, which is not the declared process ceiling ${mode.process_rss_max_bytes}`,
      );
    if (mode.per_entry_admission_max_bytes > mode.cache_owned_retained_max_bytes)
      errors.push(`budget.${name}: per-entry admission cap exceeds the whole retained ceiling`);
    const active = mode.per_request_active_max_bytes * mode.max_simultaneously_admitted_requests;
    if (active !== mode.request_active_max_bytes)
      errors.push(
        `budget.${name}: per-request cap times admitted requests is ${active}, which is not the declared active ceiling ${mode.request_active_max_bytes}`,
      );
  }
  const normal = modes.normal;
  const pressure = modes.pressure;
  if (normal && pressure) {
    if (pressure.cache_owned_retained_max_bytes >= normal.cache_owned_retained_max_bytes)
      errors.push(
        "budget.pressure: the pressure retained ceiling must be below the normal ceiling, otherwise the pressure run never refuses an admission",
      );
    if (pressure.request_active_max_bytes > normal.request_active_max_bytes)
      errors.push("budget.pressure: the active ceiling may not exceed the normal ceiling");
    if (
      pressure.external_pinned_backpressure_threshold_bytes !==
      normal.external_pinned_backpressure_threshold_bytes
    )
      errors.push(
        "budget.pressure: pressure may not change the external pin threshold; it cannot shrink what a caller already holds",
      );
    if (pressure.allocator_slack_bytes !== normal.allocator_slack_bytes)
      errors.push("budget.pressure: pressure may not change allocator slack");
    // The oversized-entry boundary is only reachable when an entry can be
    // too large to admit while still fitting one request's active budget.
    if (pressure.per_entry_admission_max_bytes > pressure.per_request_active_max_bytes)
      errors.push(
        "budget.pressure: the per-entry admission cap must not exceed the per-request active cap, or a complete result can never be too large to admit while still fitting the active budget, and the oversized-entry boundary is unreachable",
      );
  }
  return errors;
}

/**
 * Every limit is explained exactly once, and a limit may claim measured
 * provenance only when it has some. The mapping is total in both
 * directions, so neither a new limit nor a stale explanation can slip
 * through.
 */
function limitDerivationErrors(catalog) {
  const errors = [];
  const rows = catalog.limit_derivation || [];
  const declaredMetrics = new Map((catalog.metric_row || []).map((row) => [row.id, row]));
  const declaredObservations = new Map(
    (catalog.memory_observation || []).map((row) => [row.id, row]),
  );
  const seen = new Map();
  for (const row of rows) {
    const key = `${row.mode}.${row.limit}`;
    if (row.id !== key)
      errors.push(`limit derivation ${row.id}: id must be ${key} to match its own mode and limit`);
    if (seen.has(key)) errors.push(`limit derivation ${key}: declared more than once`);
    seen.set(key, row);
    const mode = catalog.budget?.[row.mode];
    if (!mode) {
      errors.push(`limit derivation ${key}: budget mode ${row.mode} is not declared`);
      continue;
    }
    if (!Object.hasOwn(mode, row.limit)) {
      errors.push(`limit derivation ${key}: budget.${row.mode} declares no limit ${row.limit}`);
      continue;
    }
    if (mode[row.limit] !== row.value)
      errors.push(
        `limit derivation ${key}: explains the value ${row.value}, but the budget declares ${mode[row.limit]}`,
      );
    if (row.derivation === "measured") {
      if (row.blocking_metric_row !== "none" || row.completion_owner !== "none")
        errors.push(
          `limit derivation ${key}: a measured limit is not blocked on anything and names no completion owner`,
        );
    } else {
      // A blocker resolves in either declared vocabulary, and either way
      // it has to actually block: something already capable of measuring
      // the limit leaves no reason to keep recording it provisional.
      const metric = declaredMetrics.get(row.blocking_metric_row);
      const observation = declaredObservations.get(row.blocking_metric_row);
      if (!metric && !observation)
        errors.push(
          `limit derivation ${key}: blocking_metric_row ${row.blocking_metric_row} is neither a declared metric row nor a declared memory observation`,
        );
      else if (metric && metric.emitter_status === "complete")
        errors.push(
          `limit derivation ${key}: blocking_metric_row ${row.blocking_metric_row} has a complete emitter, so it blocks nothing; a limit it could be measured against may not be recorded provisional`,
        );
      else if (observation && observation.aggregates_cache_owned)
        errors.push(
          `limit derivation ${key}: blocking_metric_row ${row.blocking_metric_row} already aggregates cache-owned bytes, so it blocks nothing; a limit it could be measured against may not be recorded provisional`,
        );
      if (row.completion_owner === "none")
        errors.push(`limit derivation ${key}: a provisional limit must name a completion owner`);
    }
  }
  for (const [name, mode] of Object.entries(catalog.budget || {}))
    for (const limit of BUDGET_LIMITS) {
      if (!Object.hasOwn(mode, limit)) continue;
      if (!seen.has(`${name}.${limit}`))
        errors.push(
          `limit derivation: budget.${name}.${limit} is ratified with no recorded derivation`,
        );
    }
  return errors;
}

/**
 * The live byte-valued memory surface.
 *
 * This table exists because the metric-row vocabulary below is drawn from
 * the closed audit work-site schema and structurally cannot name the
 * `RequestMemoryAudit` envelope fields — which are the only byte-valued
 * memory observations the process actually makes. Validating them here
 * keeps the contract from asserting that nothing observes memory, and
 * keeps each row's stated scope pinned to a producer that still exists.
 */
function memoryObservationErrors(catalog) {
  const errors = [];
  const rows = catalog.memory_observation || [];
  const classes = new Map((catalog.allocation_class || []).map((row) => [row.id, row]));
  const ids = new Set();
  const coverages = new Set();
  for (const row of rows) {
    if (ids.has(row.id)) errors.push(`memory observation: duplicate id ${row.id}`);
    ids.add(row.id);
    coverages.add(row.coverage);
    errors.push(...anchorErrors(row.producer, `memory observation ${row.id} producer`));
    errors.push(...anchorErrors(row.proof, `memory observation ${row.id} proof`));

    // "The field is mentioned somewhere" is not evidence that anything
    // writes it, so a proof declares which kind of evidence it is and
    // has to live where that kind of evidence lives.
    const underTests = row.proof.includes("/tests/");
    if (row.proof_kind === "discriminating_test" && !underTests)
      errors.push(
        `memory observation ${row.id}: proof_kind is discriminating_test but ${row.proof} is not under a tests tree`,
      );
    if (row.proof_kind === "live_assignment" && underTests)
      errors.push(
        `memory observation ${row.id}: proof_kind is live_assignment but ${row.proof} is a test, not a production assignment site`,
      );

    const covered = row.covers_allocation_classes || [];
    for (const id of covered)
      if (!classes.has(id))
        errors.push(
          `memory observation ${row.id}: covers_allocation_classes names ${id}, which is not a declared allocation class`,
        );
    if (row.coverage === "partial_cache_owned") {
      if (covered.length === 0)
        errors.push(
          `memory observation ${row.id}: a partial_cache_owned observation must name the allocation classes it actually reaches`,
        );
      // The inventory and this table are two views of one fact. A class
      // an observation reaches is observed, so it cannot also be
      // recorded as reaching no site at all.
      for (const id of covered) {
        const klass = classes.get(id);
        if (klass && klass.charge_observability === "uninstrumented")
          errors.push(
            `memory observation ${row.id}: allocation class ${id} is recorded uninstrumented, but this observation reports bytes for it`,
          );
      }
    } else if (covered.length > 0)
      errors.push(
        `memory observation ${row.id}: only a partial_cache_owned observation covers allocation classes; a ${row.coverage} figure is owned by no class`,
      );
  }
  for (const coverage of ["whole_process", "partial_cache_owned"])
    if (!coverages.has(coverage))
      errors.push(
        `memory observations: no ${coverage} observation is declared, so the contract cannot state what memory it does and does not see`,
      );
  if (rows.length > 0 && !rows.some((row) => row.proof_kind === "discriminating_test"))
    errors.push(
      "memory observations: no row carries a discriminating test, so the whole table rests on assignment sites that a defaulted zero would satisfy",
    );
  return errors;
}

/**
 * The pressure obligation is discharged against declared signals, and at
 * least one of them has to be something this tree can already report.
 * An obligation resting entirely on partial or absent instrumentation is
 * a promise, not a check.
 */
function pressureSignalErrors(catalog) {
  const errors = [];
  const declaredMetrics = new Map((catalog.metric_row || []).map((row) => [row.id, row]));
  const declaredObservations = new Set((catalog.memory_observation || []).map((row) => row.id));
  const signals = catalog.measurement?.pressure_boundary_signals || [];
  let complete = 0;
  for (const signal of signals) {
    const metric = declaredMetrics.get(signal);
    if (!metric && !declaredObservations.has(signal)) {
      errors.push(
        `measurement: pressure boundary signal ${signal} is neither a declared metric row nor a declared memory observation`,
      );
      continue;
    }
    if (metric?.emitter_status === "complete") complete += 1;
  }
  if (signals.length > 0 && complete === 0)
    errors.push(
      "measurement: no pressure boundary signal has a complete emitter, so the pressure obligation rests entirely on partial evidence and no run could discharge it",
    );
  return errors;
}

function scaleErrors(catalog) {
  const errors = [];
  const scale = catalog.scale || {};
  const baseline = catalog.baseline || {};
  const normal = catalog.budget?.normal || {};
  const perFile = Math.floor(baseline.measured_peak_rss_bytes / baseline.corpus_files);
  if (perFile !== scale.measured_peak_rss_bytes_per_file)
    errors.push(
      `scale: measured_peak_rss_bytes_per_file is ${scale.measured_peak_rss_bytes_per_file}, but the recorded measurement divides to ${perFile}`,
    );
  const projected = scale.measured_peak_rss_bytes_per_file * scale.supported_workload_files;
  if (projected !== scale.projected_peak_rss_bytes_at_supported_scale)
    errors.push(
      `scale: projection is ${scale.projected_peak_rss_bytes_at_supported_scale}, but the coefficient scales to ${projected}`,
    );
  const headroom = normal.process_rss_max_bytes - projected;
  if (headroom !== scale.headroom_bytes_at_supported_scale)
    errors.push(
      `scale: headroom is ${scale.headroom_bytes_at_supported_scale}, but the ceiling leaves ${headroom}`,
    );
  if (headroom <= 0)
    errors.push("scale: the projected supported workload does not fit the declared ceiling");
  return errors;
}

/** Render an integer the way the lock's prose renders measured values. */
function groupDigits(value) {
  return String(value).replace(/\B(?=(\d{3})+(?!\d))/gu, ",");
}

/**
 * The lock's prose lives in `#` comments and wraps mid-sentence, so a
 * recorded measurement can straddle a line break. Rejoin comment
 * continuations into running text before looking for a cited fragment;
 * everything else about the file is left alone.
 */
function commentProse(lockText) {
  return lockText.replaceAll(/\n#[ \t]*/gu, " ");
}

/**
 * The `[[cell.metric]]` rows belonging to one cell, as
 * `name|statistic|comparison` -> numeric limit.
 *
 * Parsed from the cell's own region of the lock (its `[[cell]]` header up
 * to the next one) so a limit cannot be matched against a different cell's
 * row of the same name.
 */
export function lockedCellMetrics(lockText, cellId) {
  const cellStarts = [...lockText.matchAll(/^\[\[cell\]\]$/gmu)].map((match) => match.index);
  for (const [position, start] of cellStarts.entries()) {
    const end = cellStarts[position + 1] ?? lockText.length;
    const region = lockText.slice(start, end);
    if (!region.includes(`id = "${cellId}"`)) continue;
    const metrics = new Map();
    for (const block of region.split("[[cell.metric]]").slice(1)) {
      const name = block.match(/^\s*name\s*=\s*"([^"]+)"/mu)?.[1];
      const statistic = block.match(/^\s*statistic\s*=\s*"([^"]+)"/mu)?.[1];
      const comparison = block.match(/^\s*comparison\s*=\s*"([^"]+)"/mu)?.[1];
      const limit = block.match(/^\s*limit\s*=\s*([0-9.]+)/mu)?.[1];
      if (name && statistic && comparison && limit !== undefined)
        metrics.set(`${name}|${statistic}|${comparison}`, Number(limit));
    }
    return metrics;
  }
  return null;
}

function baselineErrors(catalog) {
  const errors = [];
  const baseline = catalog.baseline || {};
  const scale = catalog.scale || {};
  if (!repoPathExists(baseline.source_file)) {
    errors.push(`baseline: cited lock file does not resolve: ${baseline.source_file}`);
    return errors;
  }
  const lock = readRepoFile(baseline.source_file).toString("utf8");
  const prose = commentProse(lock);
  const cited = [
    ['status = "LOCKED"', "lock status"],
    [`baseline_sha = "${baseline.baseline_sha}"`, "baseline sha"],
    [`baseline_tree = "${baseline.baseline_tree}"`, "baseline tree"],
    [`class = "${baseline.runner_class}"`, "runner class"],
    [`memory_bytes = ${baseline.runner_memory_bytes}`, "runner memory"],
    [`id = "${baseline.cell_id}"`, "cell id"],
    [`git-blob:${baseline.corpus_git_blob}`, "corpus blob id"],
    [`sha256:${baseline.corpus_sha256}`, "corpus content digest"],
    [scale.product_budget_citation ?? "", "product budget statement"],
  ];
  for (const [needle, label] of cited)
    if (!needle || !(lock.includes(needle) || prose.includes(needle)))
      errors.push(`baseline: ${label} is not cited by ${baseline.source_file}`);

  // The two recorded measurements. The lock records each one in the prose
  // that justifies its gate, so the citation is checked against that text
  // AND against the integer restated here, rendered the lock's way. A
  // fabricated measurement fails even when its derived arithmetic is
  // adjusted to stay self-consistent.
  const measurements = [
    [baseline.measured_peak_rss_bytes, baseline.measured_peak_rss_citation, "bytes", "peak RSS"],
    [baseline.measured_wall_ns, baseline.measured_wall_citation, "ns", "wall time"],
  ];
  for (const [value, citation, unit, label] of measurements) {
    const expected = `baseline is ${groupDigits(value)} ${unit}`;
    if (citation !== expected)
      errors.push(
        `baseline: the recorded ${label} citation is ${JSON.stringify(citation)}, but the value ${value} renders as ${JSON.stringify(expected)}`,
      );
    else if (!prose.includes(citation))
      errors.push(
        `baseline: ${baseline.source_file} does not record the ${label} measurement ${JSON.stringify(citation)}; the restated value is not the locked one`,
      );
  }

  // The locked limits, resolved from the cell's own metric rows rather
  // than matched as loose substrings anywhere in the file.
  const metrics = lockedCellMetrics(lock, baseline.cell_id);
  if (!metrics) {
    errors.push(`baseline: cell ${baseline.cell_id} has no metric rows in ${baseline.source_file}`);
  } else {
    const expectations = [
      ["peak_rss_bytes|max|absolute_max", baseline.locked_peak_rss_absolute_max_bytes, 1],
      [
        "peak_rss_bytes|max|no_regression_percent_max",
        baseline.locked_peak_rss_no_regression_milli_percent,
        1000,
      ],
      ["wall_ns|median|absolute_max", baseline.locked_wall_absolute_max_ns, 1],
      [
        "wall_ns|median|no_regression_percent_max",
        baseline.locked_wall_no_regression_milli_percent,
        1000,
      ],
    ];
    for (const [key, restated, scaleFactor] of expectations) {
      if (!metrics.has(key)) {
        errors.push(`baseline: cell ${baseline.cell_id} declares no ${key} metric`);
        continue;
      }
      const locked = Math.round(metrics.get(key) * scaleFactor);
      if (locked !== restated)
        errors.push(
          `baseline: ${key} is restated as ${restated}, but the locked cell declares ${locked}`,
        );
    }
  }

  // The equivalent-work gate. The corpus is synthesised by this harness, so
  // its bytes ARE the corpus; if they moved, the recorded measurement is no
  // longer a result about this tree and may not be ranked as equivalent.
  if (!repoPathExists(baseline.corpus_source_file)) {
    errors.push(`baseline: corpus source does not resolve: ${baseline.corpus_source_file}`);
  } else {
    const actual = sha256(readRepoFile(baseline.corpus_source_file));
    if (actual !== baseline.corpus_sha256)
      errors.push(
        `baseline: corpus digest recomputes to ${actual}, not the pinned ${baseline.corpus_sha256}; the recorded measurement is not equivalent work on this tree`,
      );
  }
  if (
    baseline.recorded_measurement_document_retained &&
    !repoPathExists(baseline.recorded_measurement_document)
  )
    errors.push(
      `baseline: recorded_measurement_document is declared retained but does not resolve: ${baseline.recorded_measurement_document}`,
    );
  return errors;
}

function allocationErrors(catalog) {
  const errors = [];
  const rows = catalog.allocation_class || [];
  const declaredMetrics = new Map((catalog.metric_row || []).map((row) => [row.id, row]));
  const ids = new Set();
  const owners = new Map();
  const ownerships = new Set();
  const coveredFields = new Map();
  for (const row of rows) {
    if (ids.has(row.id)) errors.push(`allocation class: duplicate id ${row.id}`);
    ids.add(row.id);
    ownerships.add(row.ownership);
    if (owners.has(row.charge_owner))
      errors.push(
        `allocation class ${row.id}: charge owner ${row.charge_owner} is already the sole owner of ${owners.get(row.charge_owner)}; a shared owner would double-charge`,
      );
    owners.set(row.charge_owner, row.id);
    errors.push(...anchorErrors(row.charge_owner, `allocation class ${row.id} charge_owner`));
    for (const producer of row.producers || [])
      errors.push(...anchorErrors(producer, `allocation class ${row.id} producer`));
    for (const consumer of row.consumers || [])
      errors.push(...anchorErrors(consumer, `allocation class ${row.id} consumer`));
    for (const field of row.covers_fields || []) {
      if (!coveredFields.has(field)) coveredFields.set(field, new Set());
      coveredFields.get(field).add(row.id);
    }
    for (const metric of row.metric_rows || [])
      if (!declaredMetrics.has(metric))
        errors.push(`allocation class ${row.id}: metric row ${metric} is not declared`);
    if (row.current_bound_kind === "retained_bytes" && row.current_bound_value < 1)
      errors.push(`allocation class ${row.id}: a retained_bytes bound needs a positive byte value`);
    if (row.current_bound_kind === "entry_count" && row.current_bound_value < 1)
      errors.push(`allocation class ${row.id}: an entry_count bound needs a positive count`);

    // Observability has to match the rows actually cited. A class may not
    // claim its charge is instrumented by citing an activity counter, and
    // it may not claim it is uninstrumented while citing sites.
    const cited = row.metric_rows || [];
    if (row.charge_observability === "uninstrumented" && cited.length > 0)
      errors.push(
        `allocation class ${row.id}: an uninstrumented class cites ${cited.length} metric rows; cite none or record it as proxy_instrumented`,
      );
    if (row.charge_observability !== "uninstrumented" && cited.length === 0)
      errors.push(
        `allocation class ${row.id}: ${row.charge_observability} requires at least one cited metric row`,
      );
    if (row.charge_observability === "charge_instrumented") {
      const reports = cited
        .map((id) => declaredMetrics.get(id))
        .filter(
          (metric) =>
            metric &&
            metric.emitter_status === "complete" &&
            (metric.unit === "Bytes" || metric.unit === "Gauge"),
        );
      if (reports.length === 0)
        errors.push(
          `allocation class ${row.id}: charge_instrumented requires a cited Bytes or Gauge row with a complete emitter, and none of its rows qualifies`,
        );
    } else if (row.gap === "none") {
      errors.push(
        `allocation class ${row.id}: a ${row.charge_observability} class must state the observability gap it leaves`,
      );
    }
  }
  for (const ownership of ["cache_owned", "request_active", "externally_pinned"])
    if (!ownerships.has(ownership))
      errors.push(
        `allocation inventory: no class carries ${ownership} ownership, so the budget cannot distinguish it`,
      );

  // The charter's named classes, which live outside the store and would
  // otherwise be removable without anything noticing.
  for (const id of REQUIRED_ALLOCATION_CLASSES)
    if (!ids.has(id))
      errors.push(
        `allocation inventory: the charter requires allocation class ${id}, which this catalog does not declare`,
      );

  // The omission check. The required population is the production field
  // list of every struct the retained-state walk reaches, read from source,
  // so a deleted allocation class leaves its field unaccounted, a new
  // host-lifetime field is unaccounted until the contract covers it, and a
  // subsystem cut out of the walk is refused.
  errors.push(...retainedWalkErrors(catalog, coveredFields));
  return errors;
}

/**
 * Walk the host's retained state from `VerterHost`, following `delegated`
 * dispositions, and require every production field reached to be EITHER
 * charged by an allocation class OR carry exactly one `[[struct_field]]`
 * disposition — never both, never neither.
 */
function retainedWalkErrors(catalog, coveredFields) {
  const errors = [];
  const dispositions = new Map();
  for (const row of catalog.struct_field || []) {
    const key = `${row.struct}.${row.field}`;
    if (dispositions.has(key)) errors.push(`struct field ${key}: declared more than once`);
    dispositions.set(key, row);
  }

  const walked = new Map();
  const queue = [RETAINED_STATE_ROOT];
  while (queue.length > 0) {
    const { struct, file } = queue.shift();
    if (walked.has(struct)) continue;
    let fields;
    try {
      fields = structFields(file, struct);
    } catch (error) {
      errors.push(`retained-state walk: cannot read ${struct}: ${error.message}`);
      walked.set(struct, []);
      continue;
    }
    if (fields.length === 0)
      errors.push(`retained-state walk: ${struct} declares no production fields`);
    walked.set(struct, fields);
    for (const field of fields) {
      const key = `${struct}.${field}`;
      const row = dispositions.get(key);
      const charging = coveredFields.get(key);
      // A class that charges a field charges ALL of it. Splitting one field
      // across sub-region classes turns those classes into a hand-written
      // closed list of what the struct behind it holds, which the walk can
      // no longer check: a region nobody named is charged nowhere. Such a
      // field has to be delegated, so each region is a walked field of its
      // own and an unnamed one is reported.
      if (charging && charging.size > 1) {
        errors.push(
          `struct field ${key}: charged by ${charging.size} allocation classes (${[...charging].join(", ")}); a field split across classes must be delegated so the walk sees every region it holds`,
        );
        continue;
      }
      if (charging && row) {
        errors.push(
          `struct field ${key}: charged by ${[...charging].join(", ")} AND dispositioned ${row.disposition}; it is one or the other`,
        );
        continue;
      }
      if (!charging && !row) {
        errors.push(
          `allocation inventory: ${struct} field ${field} is retained but has no allocation class and no disposition`,
        );
        continue;
      }
      if (row?.disposition === "delegated") {
        const target = delegateTarget(row.delegate || "");
        if (target) queue.push(target);
      }
    }
  }

  for (const struct of REQUIRED_RETAINED_STRUCTS)
    if (!walked.has(struct))
      errors.push(
        `retained-state walk: never reaches ${struct}, so every cache behind it is unaccounted`,
      );

  const observations = new Map((catalog.memory_observation || []).map((row) => [row.id, row]));
  const attributions = new Map();
  for (const [key, row] of dispositions) {
    const fields = walked.get(row.struct);
    if (!fields) {
      errors.push(
        `struct field ${key}: the retained-state walk never reaches ${row.struct}; the disposition is stale`,
      );
      continue;
    }
    if (!fields.includes(row.field)) {
      errors.push(
        `struct field ${key}: ${row.struct} has no such production field; the disposition is stale`,
      );
      continue;
    }
    const allowed = {
      non_retaining: [],
      delegated: ["delegate"],
      shared_reference: ["charged_at"],
      outside_inventory: ["attributed_by"],
    }[row.disposition];
    if (!allowed) continue;
    for (const extra of ["delegate", "charged_at", "attributed_by"]) {
      if (allowed.includes(extra) && row[extra] === undefined)
        errors.push(`struct field ${key}: a ${row.disposition} field must name its ${extra}`);
      if (!allowed.includes(extra) && row[extra] !== undefined)
        errors.push(`struct field ${key}: a ${row.disposition} field may not carry ${extra}`);
    }
    if (
      row.disposition === "delegated" &&
      row.delegate !== undefined &&
      !delegateTarget(row.delegate)
    )
      errors.push(`struct field ${key}: delegate ${row.delegate} is not a path#Struct anchor`);
    // A shared reference is charged nothing HERE because its payload is
    // charged at exactly one other field. That field has to actually be
    // charged, or the payload disappears in the hand-off.
    if (
      row.disposition === "shared_reference" &&
      row.charged_at !== undefined &&
      !coveredFields.has(row.charged_at)
    )
      errors.push(
        `struct field ${key}: shared reference to ${row.charged_at}, which no allocation class charges; the payload would be counted nowhere`,
      );
    // Only the file authority sits outside the semantic inventory, and only
    // because a live observation attributes its bytes separately.
    if (row.disposition === "outside_inventory" && row.attributed_by !== undefined) {
      const observation = observations.get(row.attributed_by);
      if (!observation || observation.coverage !== "workspace")
        errors.push(
          `struct field ${key}: outside_inventory must be attributed to a workspace memory observation, not ${row.attributed_by}`,
        );
      // One observation reports one authority's bytes. Letting a second
      // field lean on it would move that field's bytes out of the budget
      // with nothing reporting them.
      if (attributions.has(row.attributed_by))
        errors.push(
          `struct field ${key}: ${row.attributed_by} already attributes ${attributions.get(row.attributed_by)}; one observation cannot excuse two fields`,
        );
      else attributions.set(row.attributed_by, key);
    }
  }

  for (const [key, classes] of coveredFields) {
    const dot = key.indexOf(".");
    const struct = key.slice(0, dot);
    const field = key.slice(dot + 1);
    const fields = walked.get(struct);
    if (!fields)
      errors.push(
        `allocation inventory: ${[...classes].join(", ")} covers ${key}, but the retained-state walk never reaches ${struct}`,
      );
    else if (!fields.includes(field))
      errors.push(
        `allocation inventory: an allocation class covers ${key}, which ${struct} no longer has`,
      );
  }
  return errors;
}

function metricErrors(catalog) {
  const errors = [];
  let sites;
  try {
    sites = declaredWorkSites();
  } catch (error) {
    return [`metric rows: cannot read the audit site schema: ${error.message}`];
  }
  if (sites.size === 0) return ["metric rows: the audit site schema declared no sites"];
  const ids = new Set();
  for (const row of catalog.metric_row || []) {
    if (ids.has(row.id)) errors.push(`metric row: duplicate id ${row.id}`);
    ids.add(row.id);
    if (!sites.has(row.id)) {
      errors.push(`metric row ${row.id}: no such instrumentation site exists`);
      continue;
    }
    const site = sites.get(row.id);
    if (site.unit !== row.unit)
      errors.push(
        `metric row ${row.id}: declared unit ${row.unit} is not the site's unit ${site.unit}`,
      );
    const anchorProblems = anchorErrors(row.emitter_anchor, `metric row ${row.id} emitter_anchor`);
    errors.push(...anchorProblems);
    if (anchorProblems.length === 0 && row.emitter_status !== "absent") {
      // The anchor has to be where the site is actually raised, not merely a
      // file that exists. Otherwise a row could keep pointing at a plausible
      // module long after the emitter moved out of it.
      // Match the variant where it is RAISED — as the subject of an
      // attribution macro, or as an explicit `WorkSite::` selection — not
      // wherever its name happens to appear. A bare substring would accept
      // an unrelated `<Variant>Record` type as proof of an emitter.
      const raised = new RegExp(
        String.raw`(?:attribute\w*!\(\s*${site.variant}\b|WorkSite::${site.variant}\b)`,
        "u",
      );
      const emitters = rustSourcesUnder(row.emitter_anchor).filter((file) =>
        raised.test(fs.readFileSync(file, "utf8")),
      );
      if (emitters.length === 0)
        errors.push(`metric row ${row.id}: ${row.emitter_anchor} no longer raises ${site.variant}`);
    }
    if (row.emitter_status === "complete" && row.gap !== "none")
      errors.push(`metric row ${row.id}: a complete emitter may not record a gap`);
    if (row.emitter_status !== "complete" && row.gap === "none")
      errors.push(
        `metric row ${row.id}: a ${row.emitter_status} emitter must state the gap it leaves`,
      );
  }
  return errors;
}

function fixturePath(catalog, packageRoot, relative) {
  return path.join(packageRoot, catalog.workload?.fixture_root ?? "", relative);
}

function fixtureErrors(catalog, packageRoot) {
  const errors = [];
  const seen = new Set();
  for (const fixture of catalog.workload_fixture || []) {
    if (seen.has(fixture.path)) errors.push(`workload fixture: duplicate path ${fixture.path}`);
    seen.add(fixture.path);
    const absolute = fixturePath(catalog, packageRoot, fixture.path);
    if (!fs.existsSync(absolute)) {
      errors.push(`workload fixture: does not resolve: ${fixture.path}`);
      continue;
    }
    const actual = sha256(fs.readFileSync(absolute));
    if (actual !== fixture.sha256)
      errors.push(
        `workload fixture ${fixture.path}: digest recomputes to ${actual}, not the pinned ${fixture.sha256}`,
      );
  }
  const carriers = new Set(
    (catalog.workload_fixture || [])
      .filter((fixture) => fixture.role === "carrier")
      .map((fixture) => fixture.carrier),
  );
  for (const carrier of ["vue", "svelte"])
    if (!carriers.has(carrier))
      errors.push(`workload fixtures: the ${carrier} carrier is not covered`);
  for (const role of ["carrier", "module", "oversize_carrier"])
    if (!(catalog.workload_fixture || []).some((fixture) => fixture.role === role))
      errors.push(`workload fixtures: no ${role} fixture is declared`);

  // The oversize template must actually carry the expansion machinery its
  // materialization rule describes, or the boundary input is fiction.
  const materialization = catalog.workload_materialization || {};
  const oversize = (catalog.workload_fixture || []).find(
    (fixture) => fixture.path === materialization.oversize_template,
  );
  if (!oversize)
    errors.push(
      `workload materialization: oversize_template ${materialization.oversize_template} is not a declared fixture`,
    );
  else if (oversize.role !== "oversize_carrier")
    errors.push("workload materialization: oversize_template is not the oversize_carrier fixture");
  else {
    const text = fs.readFileSync(fixturePath(catalog, packageRoot, oversize.path), "utf8");
    for (const [key, label] of [
      ["oversize_marker_begin", "begin marker"],
      ["oversize_marker_end", "end marker"],
      ["oversize_ordinal_placeholder", "ordinal placeholder"],
    ])
      if (!text.includes(materialization[key]))
        errors.push(
          `workload materialization: the oversize template does not contain the declared ${label} ${JSON.stringify(materialization[key])}`,
        );
    if (materialization.oversize_size_claim !== "unmeasured")
      errors.push(
        "workload materialization: the produced entry size has not been measured, so oversize_size_claim may only be recorded unmeasured",
      );
  }
  return errors;
}

/**
 * Edit deltas are exact byte substitutions, so each has to still apply
 * unambiguously to the frozen template it was authored against.
 */
function editDeltaErrors(catalog, packageRoot) {
  const errors = [];
  const fixtures = new Map((catalog.workload_fixture || []).map((row) => [row.path, row]));
  const ids = new Set();
  const templatesWithDelta = new Set();
  for (const delta of catalog.workload_edit_delta || []) {
    if (ids.has(delta.id)) errors.push(`edit delta: duplicate id ${delta.id}`);
    ids.add(delta.id);
    const fixture = fixtures.get(delta.template);
    if (!fixture) {
      errors.push(`edit delta ${delta.id}: template ${delta.template} is not a declared fixture`);
      continue;
    }
    if (fixture.health !== "healthy")
      errors.push(`edit delta ${delta.id}: a malformed fixture is never edited`);
    templatesWithDelta.add(delta.template);
    const absolute = fixturePath(catalog, packageRoot, delta.template);
    if (!fs.existsSync(absolute)) continue;
    const text = fs.readFileSync(absolute, "utf8");
    const occurrences = text.split(delta.find).length - 1;
    if (occurrences === 0)
      errors.push(
        `edit delta ${delta.id}: its find text does not occur in ${delta.template}, so the edit cannot be applied`,
      );
    else if (occurrences > 1)
      errors.push(
        `edit delta ${delta.id}: its find text occurs ${occurrences} times in ${delta.template}, so neither the edit nor its inverse is unambiguous`,
      );
    if (delta.find === delta.replace)
      errors.push(`edit delta ${delta.id}: replacing text with itself is not an edit`);
    if (text.includes(delta.replace))
      errors.push(
        `edit delta ${delta.id}: its replacement text is already present in ${delta.template}, so the revert would not restore the original bytes`,
      );
  }
  // Every editable template needs at least one delta, or the expander
  // cannot produce an edit for an instance of it.
  for (const fixture of catalog.workload_fixture || []) {
    if (fixture.health !== "healthy" || fixture.role === "oversize_carrier") continue;
    if (!templatesWithDelta.has(fixture.path))
      errors.push(`edit delta: template ${fixture.path} has no declared edit`);
  }
  return errors;
}

function readSetting(config, dotted) {
  let cursor = config;
  for (const segment of dotted.split(".")) {
    if (cursor === null || typeof cursor !== "object" || !Object.hasOwn(cursor, segment))
      return undefined;
    cursor = cursor[segment];
  }
  return cursor;
}

/**
 * Configuration deltas name an exact setting with exact values, and the
 * baseline value they name has to be the one the materialized projects
 * actually start from.
 */
function configurationDeltaErrors(catalog) {
  const errors = [];
  const raw = catalog.workload_materialization?.project_config_baseline;
  let baselineConfig;
  try {
    baselineConfig = JSON.parse(raw ?? "");
  } catch (error) {
    return [
      `workload materialization: project_config_baseline is not valid JSON: ${error.message}`,
    ];
  }
  // TypeScript include has no brace expansion, so multi-extension coverage
  // is separate entries. A brace pattern would silently own nothing.
  for (const entry of baselineConfig.include ?? [])
    if (entry.includes("{"))
      errors.push(
        `workload materialization: include entry ${entry} uses brace expansion, which TypeScript does not perform; carrier extensions need separate entries`,
      );
  const ids = new Set();
  for (const delta of catalog.workload_configuration_delta || []) {
    if (ids.has(delta.id)) errors.push(`configuration delta: duplicate id ${delta.id}`);
    ids.add(delta.id);
    const actual = readSetting(baselineConfig, delta.setting);
    if (actual === undefined) {
      errors.push(
        `configuration delta ${delta.id}: setting ${delta.setting} does not exist in the materialized baseline configuration`,
      );
      continue;
    }
    let declared;
    let applied;
    try {
      declared = JSON.parse(delta.baseline_value);
      applied = JSON.parse(delta.applied_value);
    } catch (error) {
      errors.push(`configuration delta ${delta.id}: values are not valid JSON: ${error.message}`);
      continue;
    }
    if (JSON.stringify(actual) !== JSON.stringify(declared))
      errors.push(
        `configuration delta ${delta.id}: declares the baseline value ${delta.baseline_value}, but the materialized configuration holds ${JSON.stringify(actual)}`,
      );
    if (JSON.stringify(applied) === JSON.stringify(declared))
      errors.push(`configuration delta ${delta.id}: applying the baseline value changes nothing`);
  }
  return errors;
}

function workloadErrors(catalog, packageRoot) {
  const errors = [];
  const workload = catalog.workload || {};
  if (JSON.stringify(workload.manifest_columns) !== JSON.stringify(MANIFEST_COLUMNS))
    errors.push("workload: declared manifest columns are not the expander's column order");

  const expanderAbsolute = path.join(packageRoot, workload.expander || "");
  if (!workload.expander || !fs.existsSync(expanderAbsolute)) {
    errors.push(`workload: expander does not resolve: ${workload.expander}`);
    return errors;
  }
  const expanderDigest = sha256(fs.readFileSync(expanderAbsolute));
  if (expanderDigest !== workload.expander_sha256)
    errors.push(
      `workload: expander digest recomputes to ${expanderDigest}, not the pinned ${workload.expander_sha256}`,
    );

  let actions;
  try {
    actions = expandWorkload(workloadSpec(catalog));
  } catch (error) {
    return [...errors, `workload: expansion failed: ${error.message}`];
  }

  const manifest = serializeManifest(actions);
  const manifestDigest = sha256(Buffer.from(manifest, "utf8"));
  if (manifestDigest !== workload.manifest_sha256)
    errors.push(
      `workload: manifest digest recomputes to ${manifestDigest}, not the pinned ${workload.manifest_sha256}`,
    );

  if (actions.length !== workload.total_actions)
    errors.push(
      `workload: expansion produced ${actions.length} actions, not the declared ${workload.total_actions}`,
    );
  if (actions.length < 10000)
    errors.push(`workload: ${actions.length} actions is below the contract minimum of 10000`);

  errors.push(...coverageErrors(catalog, actions));
  errors.push(...trancheErrors(catalog, actions));
  return errors;
}

export function coverageErrors(catalog, actions) {
  const errors = [];
  const covered = new Set(actions.map((action) => action.class));
  const fixtures = new Map((catalog.workload_fixture || []).map((row) => [row.path, row]));
  const identities = new Set();
  for (const action of actions) {
    // Kind-driven coverage first: the boundaries that are properties of
    // the ACTION rather than of a fixture must be observed even for the
    // actions that name no template.
    if (action.kind === "close_project" && action.result_class === "closed_with_active_readers")
      covered.add("project_close_with_active_readers");
    if (action.kind === "request_cancelled" && action.cancel_at === "none")
      errors.push(`workload action ${action.seq}: a cancelled request must name its cancel point`);
    if (action.query !== "none") identities.add(action.query);

    if (action.template === "none") {
      if (action.instance !== "none")
        errors.push(`workload action ${action.seq}: an instance without a template`);
      continue;
    }
    const fixture = fixtures.get(action.template);
    if (!fixture) {
      errors.push(`workload action ${action.seq}: template ${action.template} is not declared`);
      continue;
    }
    // Instances reproduce the template's relative layout, so a request is
    // a genuine cross-file resolution rather than an isolated file.
    if (!action.instance.includes(`/${action.template.slice(0, action.template.indexOf("/"))}/`))
      errors.push(
        `workload action ${action.seq}: instance ${action.instance} does not preserve the template's directory layout`,
      );
    if (fixture.carrier === "vue") covered.add("vue_carrier");
    if (fixture.carrier === "svelte") covered.add("svelte_carrier");

    // Only requestable roles are ever requested. A shared module is an
    // edit target; requesting one would not be executable.
    const isRequest = action.kind.startsWith("request_");
    if (isRequest && fixture.role === "module")
      errors.push(
        `workload action ${action.seq}: ${action.kind} targets a shared module, which the request API does not accept`,
      );
    const malformedExpected = action.kind === "request_failed_construction";
    if (!malformedExpected && fixture.health === "malformed")
      errors.push(
        `workload action ${action.seq}: ${action.kind} must not run against a malformed fixture`,
      );
    if (malformedExpected && fixture.health !== "malformed")
      errors.push(
        `workload action ${action.seq}: failed construction must run against a malformed fixture`,
      );
    if (action.kind === "request_oversized" && fixture.role !== "oversize_carrier")
      errors.push(
        `workload action ${action.seq}: the oversized-entry boundary must run against the oversize input`,
      );
  }
  const minimum = catalog.workload?.min_distinct_query_identities ?? 0;
  if (identities.size < minimum)
    errors.push(
      `workload: ${identities.size} distinct query identities is below the declared minimum ${minimum}; same-key repetition cannot satisfy this workload`,
    );
  else covered.add("high_cardinality_key");

  // Concurrent project sessions: more than one open at once in a tranche.
  const openDepth = new Map();
  for (const action of actions) {
    const depth = openDepth.get(action.tranche) ?? 0;
    if (action.kind === "open_project") {
      const next = depth + 1;
      openDepth.set(action.tranche, next);
      if (next > 1) covered.add("concurrent_project_sessions");
    } else if (action.kind === "close_project") openDepth.set(action.tranche, depth - 1);
  }

  // The required population is the contract's, not the catalog's.
  const declared = new Map((catalog.workload_class || []).map((row) => [row.id, row]));
  for (const id of REQUIRED_LIFECYCLE_CLASSES) {
    const row = declared.get(id);
    if (!row) {
      errors.push(
        `workload: the resource contract requires lifecycle class ${id}, which this catalog does not declare`,
      );
      continue;
    }
    if (row.required !== true)
      errors.push(
        `workload: lifecycle class ${id} is required by the resource contract and may not be declared optional`,
      );
    if (!covered.has(id))
      errors.push(`workload: required lifecycle class ${id} is not covered by the manifest`);
  }
  // Anything the catalog additionally elects to require is checked too,
  // without re-reporting the contract-required ids above.
  for (const row of catalog.workload_class || [])
    if (row.required && !REQUIRED_LIFECYCLE_CLASSES.includes(row.id) && !covered.has(row.id))
      errors.push(`workload: required lifecycle class ${row.id} is not covered by the manifest`);
  return errors;
}

function multiset(values) {
  return JSON.stringify([...values].sort());
}

/**
 * The project tree an instance path lives in:
 * `{instance_root}/t{tranche}/{project}`, the first three segments of the
 * declared instance-path rule.
 */
function projectTree(instance) {
  return instance.split("/").slice(0, 3).join("/");
}

/**
 * Per-tranche control restoration and the ordering invariants the
 * lifecycle boundaries depend on.
 *
 * Every tranche must sample exactly once, checkpoint exactly once as its
 * final action, and hand back the same live set it started from: every
 * edit reverted ON THE INSTANCE IT WAS APPLIED TO, every applied
 * configuration delta reverted ON THE PROJECT IT WAS APPLIED TO, every
 * pinned result released, every opened project closed.
 */
export function trancheErrors(catalog, actions) {
  const errors = [];
  const interval = catalog.measurement?.sampling_interval_actions;
  const byTranche = new Map();
  for (const action of actions) {
    if (!byTranche.has(action.tranche)) byTranche.set(action.tranche, []);
    byTranche.get(action.tranche).push(action);
  }
  for (const [tranche, rows] of byTranche) {
    const at = (label) => `workload tranche ${tranche}: ${label}`;
    if (rows.length !== interval)
      errors.push(
        at(`${rows.length} actions does not match the declared sampling interval ${interval}`),
      );
    if (rows.filter((row) => row.kind === "sample").length !== 1)
      errors.push(at("expected exactly one sample"));
    if (rows.filter((row) => row.kind === "control_checkpoint").length !== 1)
      errors.push(at("expected exactly one control checkpoint"));
    if (rows.at(-1)?.kind !== "control_checkpoint")
      errors.push(at("the control checkpoint must be the tranche's final action"));

    // Edits and reverts pair on (delta, instance), and a revert may not
    // precede the edit it undoes.
    const edits = rows.filter((row) => row.kind === "edit").map((row) => row.delta.slice(5));
    const reverts = rows.filter((row) => row.kind === "revert").map((row) => row.delta.slice(7));
    if (multiset(edits) !== multiset(reverts))
      errors.push(at("the edits it applies are not exactly the edits it reverts"));
    const applyAt = new Map();
    for (const row of rows) {
      if (row.kind === "edit") applyAt.set(row.delta.slice(5), row.seq);
      if (row.kind === "revert") {
        const key = row.delta.slice(7);
        if (!applyAt.has(key))
          errors.push(at(`the revert at ${row.seq} has no preceding edit for ${key}`));
      }
    }

    // Configuration deltas pair on (delta, PROJECT). Reverting a delta on
    // a different project than it was applied to leaves that project
    // configured and un-reverts nothing on the other, which is precisely
    // the state a control checkpoint must never be reached in.
    const configApplied = [];
    const configReverted = [];
    const configAppliedAt = new Map();
    for (const row of rows) {
      if (!row.delta.startsWith("config:")) continue;
      const parts = row.delta.split(":");
      if (parts.length !== 4) {
        errors.push(at(`malformed configuration delta ${row.delta}`));
        continue;
      }
      const [, deltaId, project, action] = parts;
      if (project !== row.project)
        errors.push(
          at(
            `the configuration action at ${row.seq} names project ${project} but is recorded against ${row.project}`,
          ),
        );
      const key = `${deltaId}@${project}`;
      if (action === "apply") {
        configApplied.push(key);
        configAppliedAt.set(key, row.seq);
      } else if (action === "revert") {
        configReverted.push(key);
        if (!configAppliedAt.has(key))
          errors.push(
            at(
              `the configuration revert at ${row.seq} undoes ${deltaId} on ${project}, which this tranche never applied there`,
            ),
          );
      } else errors.push(at(`unknown configuration action ${action}`));
    }
    if (multiset(configApplied) !== multiset(configReverted))
      errors.push(
        at(
          "the configuration deltas it applies are not exactly the ones it reverts, on the same projects",
        ),
      );

    const held = rows.filter((row) => row.kind === "hold_result").map((row) => row.query);
    const released = rows.filter((row) => row.kind === "release_result").map((row) => row.query);
    if (multiset(held) !== multiset(released))
      errors.push(at("the results it pins are not exactly the results it releases"));

    const opened = rows.filter((row) => row.kind === "open_project").map((row) => row.project);
    const closed = rows.filter((row) => row.kind === "close_project").map((row) => row.project);
    if (multiset(opened) !== multiset(closed))
      errors.push(at("the projects it opens are not exactly the projects it closes"));

    errors.push(...overlapGroupErrors(tranche, rows));
    errors.push(...postMutationErrors(tranche, rows));
  }
  return errors;
}

/**
 * An overlapping-request group is only evidence of a singleflight join
 * when its members demand ONE key that nothing has yet constructed, with
 * no completion between them and exactly one leader.
 */
export function overlapGroupErrors(tranche, rows) {
  const errors = [];
  const at = (label) => `workload tranche ${tranche}: ${label}`;
  const groups = new Map();
  for (const row of rows) {
    if (row.overlap_group === "none") continue;
    if (!groups.has(row.overlap_group)) groups.set(row.overlap_group, []);
    groups.get(row.overlap_group).push(row);
  }
  if (groups.size === 0) errors.push(at("declares no overlapping-request group"));
  for (const [group, members] of groups) {
    if (members.length < 2) {
      errors.push(at(`overlap group ${group} has one member, so nothing overlaps`));
      continue;
    }
    const keys = new Set(members.map((row) => row.query));
    if (keys.size !== 1)
      errors.push(
        at(
          `overlap group ${group} spans ${keys.size} keys; concurrent demand on different keys collapses onto nothing`,
        ),
      );
    const leaders = members.filter((row) => row.cache_disposition === "must_construct");
    const joiners = members.filter((row) => row.cache_disposition === "join_inflight");
    if (leaders.length !== 1)
      errors.push(
        at(`overlap group ${group} declares ${leaders.length} leaders, expected exactly one`),
      );
    if (joiners.length !== members.length - 1)
      errors.push(at(`overlap group ${group} has members that neither lead nor join`));
    const first = members[0].seq;
    if (members.some((row, index) => row.seq !== first + index))
      errors.push(
        at(
          `overlap group ${group} is not contiguous, so another action completes between its members and they are not concurrent`,
        ),
      );
    // The subject must be genuinely uncomputed: no earlier action in the
    // tranche may have requested it.
    const key = members[0].query;
    const earlier = rows.filter((row) => row.seq < first && row.query === key);
    if (earlier.length > 0)
      errors.push(
        at(
          `overlap group ${group} races on ${key}, which action ${earlier[0].seq} already requested; the group would join a completed result rather than a construction`,
        ),
      );
  }
  return errors;
}

/**
 * A post-mutation request is only evidence of invalidation when its
 * subject really is invalidated: the edit must precede it, must not have
 * been reverted yet, and must reach the requested carrier — itself, or a
 * shared module that carrier imports from the same project.
 */
export function postMutationErrors(tranche, rows) {
  const errors = [];
  const at = (label) => `workload tranche ${tranche}: ${label}`;
  const editedAt = new Map();
  const revertedAt = new Map();
  for (const row of rows) {
    if (row.kind === "edit") editedAt.set(row.instance, row.seq);
    if (row.kind === "revert") revertedAt.set(row.instance, row.seq);
  }
  let seen = 0;
  for (const row of rows) {
    if (row.kind !== "request_after_edit") continue;
    seen += 1;
    if (!row.delta.startsWith("after:")) {
      errors.push(at(`the post-mutation request at ${row.seq} names no edited subject`));
      continue;
    }
    const target = row.delta.slice("after:".length);
    const edited = editedAt.get(target);
    if (edited === undefined) {
      errors.push(
        at(
          `the post-mutation request at ${row.seq} follows an edit to ${target} that never happened`,
        ),
      );
      continue;
    }
    if (edited > row.seq)
      errors.push(
        at(`the post-mutation request at ${row.seq} precedes the edit it is supposed to follow`),
      );
    const reverted = revertedAt.get(target);
    if (reverted !== undefined && reverted < row.seq)
      errors.push(
        at(
          `the post-mutation request at ${row.seq} follows the revert of ${target}, so nothing is invalidated`,
        ),
      );
    if (row.cache_disposition !== "must_recompute")
      errors.push(
        at(
          `the post-mutation request at ${row.seq} declares ${row.cache_disposition}; a request whose subject was just invalidated must recompute`,
        ),
      );
    // Invalidation has to actually reach the requested carrier: the same
    // file, or a shared module inside the same project tree, which every
    // carrier in that tree imports from.
    const sameFile = row.instance === target;
    const sameProject = projectTree(target) === projectTree(row.instance);
    if (!sameFile && !sameProject)
      errors.push(
        at(
          `the post-mutation request at ${row.seq} requests ${row.instance}, which the edit to ${target} does not reach`,
        ),
      );
  }
  if (seen === 0) errors.push(at("declares no post-mutation request"));
  return errors;
}

/** The binding fields a `required` lane names, and only a `required` lane. */
const REQUIRED_LANE_FIELDS = Object.freeze(["gate_profile", "ci_job", "ci_filter"]);

/** Command kinds that ARE the contract's enforcement, so must run in a required lane. */
const ENFORCING_COMMAND_KINDS = Object.freeze(["validate", "negative_control"]);

/**
 * The command rows, and the bindings behind every `required` lane.
 *
 * A declared binding is resolved, never trusted: the gate profile has to
 * exist and list the command verbatim, the CI job has to exist, issue the
 * whole command line and be gated on the declared trigger filter. Then
 * every repository path this run opened has to be covered by that filter,
 * or a change to it could merge with the enforcing job skipped.
 */
function commandErrors(catalog, packageRoot, workflowFile) {
  const errors = [];
  const rows = catalog.command || [];
  const ids = new Set();
  for (const row of rows) {
    if (ids.has(row.id)) errors.push(`command: duplicate id ${row.id}`);
    ids.add(row.id);
    if (row.bound && row.lane === "unbound")
      errors.push(`command ${row.id}: a bound command cannot declare an unbound lane`);
    if (!row.bound && row.lane !== "unbound")
      errors.push(`command ${row.id}: an unbound command must declare the unbound lane`);
    for (const field of REQUIRED_LANE_FIELDS) {
      if (row.lane === "required" && row[field] === undefined)
        errors.push(`command ${row.id}: a required lane must name its ${field}`);
      if (row.lane !== "required" && row[field] !== undefined)
        errors.push(`command ${row.id}: only a required lane names a ${field}`);
    }
    if (ENFORCING_COMMAND_KINDS.includes(row.kind) && row.bound && row.lane !== "required")
      errors.push(
        `command ${row.id}: a ${row.kind} command must run in a required lane; a ${row.lane} one enforces nothing`,
      );
  }
  const required = rows.filter((row) => row.bound && row.lane === "required");
  if (!required.some((row) => row.kind === "validate"))
    errors.push("commands: no bound validate command is declared in a required lane");
  if (!required.some((row) => row.kind === "negative_control"))
    errors.push("commands: no bound negative-control command is declared in a required lane");
  if (!rows.some((row) => row.kind === "build" && row.bound))
    errors.push("commands: no bound build command is declared");
  if (!rows.some((row) => row.kind === "run")) errors.push("commands: no run command is declared");
  if (required.length === 0) return errors;

  let profiles = null;
  try {
    profiles = readToml(path.join(packageRoot, GATE_PROFILES_RELATIVE)).profile || [];
  } catch (error) {
    errors.push(`commands: cannot read ${GATE_PROFILES_RELATIVE}: ${error.message}`);
  }
  let workflow = null;
  try {
    workflow = fs.readFileSync(workflowFile, "utf8");
    const relative = repoRelative(workflowFile);
    if (relative) recordOpened(relative);
  } catch (error) {
    errors.push(`commands: cannot read the CI workflow: ${error.message}`);
  }
  const jobs = workflow === null ? new Map() : workflowJobs(workflow);
  const filters = new Map();

  for (const row of required) {
    const where = `command ${row.id}`;
    if (profiles && row.gate_profile !== undefined) {
      const profile = profiles.find((entry) => entry.id === row.gate_profile);
      if (!profile) errors.push(`${where}: gate profile ${row.gate_profile} is not declared`);
      else if (!(profile.final || []).includes(row.command))
        errors.push(
          `${where}: gate profile ${row.gate_profile} does not run ${JSON.stringify(row.command)} in its final list`,
        );
    }
    if (workflow === null || row.ci_job === undefined || row.ci_filter === undefined) continue;
    const body = jobs.get(row.ci_job);
    if (body === undefined) {
      errors.push(`${where}: ${row.ci_job} is not a job of the CI workflow`);
      continue;
    }
    // The whole line, not containment: a declaration that is a prefix of
    // what the job issues would hide the arguments that decide what runs.
    const line = laneCommandLine(body, row.command);
    if (line === null)
      errors.push(`${where}: job ${row.ci_job} does not run ${JSON.stringify(row.command)}`);
    else if (line !== row.command)
      errors.push(
        `${where}: job ${row.ci_job} issues ${JSON.stringify(line)}, not the declared ${JSON.stringify(row.command)}`,
      );
    if (!body.includes(`needs.detect-changes.outputs.${row.ci_filter} == 'true'`))
      errors.push(
        `${where}: job ${row.ci_job} is not gated on the ${row.ci_filter} trigger filter it declares`,
      );
    if (!filters.has(row.ci_filter))
      filters.set(row.ci_filter, triggerPaths(workflow, row.ci_filter));
  }

  // What the declared filter has to cover is what this run actually read.
  const opened = [...(openedRepoPaths ?? [])].sort();
  for (const [filter, patterns] of filters) {
    if (!patterns || patterns.length === 0) {
      errors.push(`commands: the CI workflow declares no ${filter} trigger filter`);
      continue;
    }
    for (const relative of opened)
      if (!triggerCovers(patterns, relative))
        errors.push(
          `trigger coverage: this validation reads ${relative}, which no ${filter} trigger pattern covers, so a change to it would not run the job that enforces this contract`,
        );
  }
  return errors;
}

/**
 * Validate an already-parsed catalog against an already-parsed schema.
 * Exported separately from the file-reading entry point so the negative
 * controls can mutate an in-memory catalog and prove the mutation applied
 * before asserting the refusal.
 *
 * `workflowFile` is the CI workflow the required lanes resolve against;
 * the negative controls point it at a mutated copy.
 */
export function validateSemanticMemoryBudgetModel(
  catalog,
  schema,
  validateSchemaObject,
  packageRoot = PACKAGE_ROOT,
  { workflowFile = path.join(REPO_ROOT, CI_WORKFLOW) } = {},
) {
  openedRepoPaths = new Set();
  try {
    // The package itself is read too: this catalog, its schema and
    // fixtures, the gate profiles and the tools implementing the checks.
    const packageRelative = repoRelative(packageRoot);
    if (packageRelative) recordOpened(packageRelative);
    const errors = [...validateSchemaObject(catalog, schema, "catalogs.semantic-memory-budget")];
    errors.push(...budgetErrors(catalog));
    errors.push(...limitDerivationErrors(catalog));
    errors.push(...memoryObservationErrors(catalog));
    errors.push(...pressureSignalErrors(catalog));
    errors.push(...scaleErrors(catalog));
    errors.push(...baselineErrors(catalog));
    errors.push(...allocationErrors(catalog));
    errors.push(...metricErrors(catalog));
    errors.push(...fixtureErrors(catalog, packageRoot));
    errors.push(...editDeltaErrors(catalog, packageRoot));
    errors.push(...configurationDeltaErrors(catalog));
    errors.push(...workloadErrors(catalog, packageRoot));
    // Last, so the trigger coverage it checks sees every path read above.
    errors.push(...commandErrors(catalog, packageRoot, workflowFile));
    return errors;
  } finally {
    openedRepoPaths = null;
  }
}

export function loadCatalog(packageRoot = PACKAGE_ROOT) {
  return readToml(path.join(packageRoot, CATALOG_RELATIVE));
}

export function loadSchema(packageRoot = PACKAGE_ROOT) {
  return JSON.parse(fs.readFileSync(path.join(packageRoot, "schemas", SCHEMA_NAME), "utf8"));
}

/** Expand and serialize the frozen manifest for the current catalog. */
export function emitManifest(packageRoot = PACKAGE_ROOT) {
  return serializeManifest(expandWorkload(workloadSpec(loadCatalog(packageRoot))));
}

/**
 * File-reading entry point. `validateSchemaObject` is injected so this
 * module does not import the program-DAG library that calls it.
 */
export function validateSemanticMemoryBudget(validateSchemaObject, packageRoot = PACKAGE_ROOT) {
  let catalog;
  let schema;
  try {
    catalog = loadCatalog(packageRoot);
    schema = loadSchema(packageRoot);
  } catch (error) {
    return [`semantic memory budget: ${error.message}`];
  }
  return validateSemanticMemoryBudgetModel(catalog, schema, validateSchemaObject, packageRoot);
}
