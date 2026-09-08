// Validation for the aggregate semantic memory budget and workload contract.
//
// What this validator is, and is not.
//
// It is a CITATION-INTEGRITY and ARITHMETIC check over a contract document.
// It re-resolves the symbols, files and digests the contract cites against
// the working tree, re-derives every stated arithmetic identity, and
// re-expands the frozen workload to confirm it still covers what it claims.
// It is the same discipline the closure-register instrument already applies
// to its own cited fixtures and evidence anchors.
//
// It is NOT an architecture guard. It does not enforce any invariant on the
// code it reads; it only refuses to let this contract go on describing
// symbols, digests or arithmetic that no longer hold. A renamed owner is a
// contract-repin obligation, not a rule violation.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
const AUDIT_SITE_SCHEMA = "crates/verter_audit/src/attribution/schema.rs";

/** Digest form used for every pin in the catalog. */
export function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function readRepoFile(relative) {
  return fs.readFileSync(path.join(REPO_ROOT, relative));
}

function repoPathExists(relative) {
  return fs.existsSync(path.join(REPO_ROOT, relative));
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
  if (!fs.readFileSync(target, "utf8").includes(symbol))
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
      fixture_pool: row.fixture_pool,
      result_class: row.result_class,
      admission_class: row.admission_class,
    };
  return {
    tranches: catalog.workload?.tranches,
    tranche_actions: catalog.workload?.tranche_actions,
    projects_per_tranche: catalog.workload?.projects_per_tranche,
    projects: catalog.workload_project || [],
    fixtures: catalog.workload_fixture || [],
    steps: catalog.workload_step || [],
    configuration_deltas: catalog.workload_configuration_delta || [],
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
  }
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

function baselineErrors(catalog) {
  const errors = [];
  const baseline = catalog.baseline || {};
  if (!repoPathExists(baseline.source_file)) {
    errors.push(`baseline: cited lock file does not resolve: ${baseline.source_file}`);
    return errors;
  }
  const lock = readRepoFile(baseline.source_file).toString("utf8");
  const cited = [
    ['status = "LOCKED"', "lock status"],
    [`baseline_sha = "${baseline.baseline_sha}"`, "baseline sha"],
    [`baseline_tree = "${baseline.baseline_tree}"`, "baseline tree"],
    [`class = "${baseline.runner_class}"`, "runner class"],
    [`memory_bytes = ${baseline.runner_memory_bytes}`, "runner memory"],
    [`id = "${baseline.cell_id}"`, "cell id"],
    [`git-blob:${baseline.corpus_git_blob}`, "corpus blob id"],
    [`sha256:${baseline.corpus_sha256}`, "corpus content digest"],
    [`limit = ${baseline.locked_peak_rss_absolute_max_bytes}`, "locked peak RSS cap"],
  ];
  for (const [needle, label] of cited)
    if (!lock.includes(needle))
      errors.push(`baseline: ${label} is not cited by ${baseline.source_file}`);

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
  const declaredMetrics = new Set((catalog.metric_row || []).map((row) => row.id));
  const ids = new Set();
  const owners = new Map();
  const ownerships = new Set();
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
    for (const metric of row.metric_rows || [])
      if (!declaredMetrics.has(metric))
        errors.push(`allocation class ${row.id}: metric row ${metric} is not declared`);
    if (row.current_bound_kind === "retained_bytes" && row.current_bound_value < 1)
      errors.push(`allocation class ${row.id}: a retained_bytes bound needs a positive byte value`);
    if (row.current_bound_kind === "entry_count" && row.current_bound_value < 1)
      errors.push(`allocation class ${row.id}: an entry_count bound needs a positive count`);
  }
  for (const ownership of ["cache_owned", "request_active", "externally_pinned"])
    if (!ownerships.has(ownership))
      errors.push(
        `allocation inventory: no class carries ${ownership} ownership, so the budget cannot distinguish it`,
      );
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

function fixtureErrors(catalog, packageRoot) {
  const errors = [];
  const seen = new Set();
  for (const fixture of catalog.workload_fixture || []) {
    if (seen.has(fixture.path)) errors.push(`workload fixture: duplicate path ${fixture.path}`);
    seen.add(fixture.path);
    const absolute = path.join(packageRoot, fixture.path);
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
  const carriers = new Set((catalog.workload_fixture || []).map((fixture) => fixture.carrier));
  for (const carrier of ["vue", "svelte"])
    if (!carriers.has(carrier))
      errors.push(`workload fixtures: the ${carrier} carrier is not covered`);
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

function coverageErrors(catalog, actions) {
  const errors = [];
  const covered = new Set(actions.map((action) => action.class));
  const fixtures = new Map(
    (catalog.workload_fixture || []).map((fixture) => [fixture.path, fixture]),
  );
  const identities = new Set();
  for (const action of actions) {
    const fixture = fixtures.get(action.fixture);
    if (!fixture) {
      errors.push(`workload action ${action.seq}: fixture ${action.fixture} is not declared`);
      continue;
    }
    if (fixture.carrier === "vue") covered.add("vue_carrier");
    if (fixture.carrier === "svelte") covered.add("svelte_carrier");
    if (action.query !== "none") identities.add(action.query);
    const malformedExpected = action.kind === "request_failed_construction";
    if (!malformedExpected && fixture.health === "malformed")
      errors.push(
        `workload action ${action.seq}: ${action.kind} must not run against a malformed fixture`,
      );
    if (malformedExpected && fixture.health !== "malformed")
      errors.push(
        `workload action ${action.seq}: failed construction must run against a malformed fixture`,
      );
    if (action.kind === "close_project" && action.result_class === "closed_with_active_readers")
      covered.add("project_close_with_active_readers");
    if (action.kind === "request_cancelled" && action.cancel_at === "none")
      errors.push(`workload action ${action.seq}: a cancelled request must name its cancel point`);
  }
  const minimum = catalog.workload?.min_distinct_query_identities ?? 0;
  if (identities.size < minimum)
    errors.push(
      `workload: ${identities.size} distinct query identities is below the declared minimum ${minimum}; same-key repetition cannot satisfy this workload`,
    );
  else covered.add("high_cardinality_key");

  for (const row of catalog.workload_class || [])
    if (row.required && !covered.has(row.id))
      errors.push(`workload: required lifecycle class ${row.id} is not covered by the manifest`);
  return errors;
}

/**
 * Per-tranche control restoration. Every tranche must sample exactly once,
 * checkpoint exactly once as its final action, and hand back the same live
 * set it started from: every edit reverted, every applied configuration
 * delta reverted, every pinned result released, every opened project
 * closed.
 */
function trancheErrors(catalog, actions) {
  const errors = [];
  const interval = catalog.measurement?.sampling_interval_actions;
  const byTranche = new Map();
  for (const action of actions) {
    if (!byTranche.has(action.tranche)) byTranche.set(action.tranche, []);
    byTranche.get(action.tranche).push(action);
  }
  for (const [tranche, rows] of byTranche) {
    if (rows.length !== interval)
      errors.push(
        `workload tranche ${tranche}: ${rows.length} actions does not match the declared sampling interval ${interval}`,
      );
    const samples = rows.filter((row) => row.kind === "sample");
    const checkpoints = rows.filter((row) => row.kind === "control_checkpoint");
    if (samples.length !== 1)
      errors.push(`workload tranche ${tranche}: expected exactly one sample`);
    if (checkpoints.length !== 1)
      errors.push(`workload tranche ${tranche}: expected exactly one control checkpoint`);
    if (rows.at(-1)?.kind !== "control_checkpoint")
      errors.push(
        `workload tranche ${tranche}: the control checkpoint must be the tranche's final action`,
      );

    const edits = rows.filter((row) => row.kind === "edit").map((row) => row.delta.slice(5));
    const reverts = rows.filter((row) => row.kind === "revert").map((row) => row.delta.slice(7));
    if (JSON.stringify([...edits].sort()) !== JSON.stringify([...reverts].sort()))
      errors.push(
        `workload tranche ${tranche}: the edits it applies are not exactly the edits it reverts`,
      );

    const applied = rows
      .filter((row) => row.delta.endsWith(":apply"))
      .map((row) => row.delta.slice(0, -":apply".length));
    const restored = rows
      .filter((row) => row.delta.endsWith(":revert"))
      .map((row) => row.delta.slice(0, -":revert".length));
    if (JSON.stringify([...applied].sort()) !== JSON.stringify([...restored].sort()))
      errors.push(
        `workload tranche ${tranche}: the configuration deltas it applies are not exactly the ones it reverts`,
      );

    const held = rows.filter((row) => row.kind === "hold_result").map((row) => row.query);
    const released = rows.filter((row) => row.kind === "release_result").map((row) => row.query);
    if (JSON.stringify([...held].sort()) !== JSON.stringify([...released].sort()))
      errors.push(
        `workload tranche ${tranche}: the results it pins are not exactly the results it releases`,
      );

    const opened = rows.filter((row) => row.kind === "open_project").map((row) => row.project);
    const closed = rows.filter((row) => row.kind === "close_project").map((row) => row.project);
    if (JSON.stringify([...opened].sort()) !== JSON.stringify([...closed].sort()))
      errors.push(
        `workload tranche ${tranche}: the projects it opens are not exactly the projects it closes`,
      );
  }
  return errors;
}

function commandErrors(catalog) {
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
  }
  if (!rows.some((row) => row.kind === "validate" && row.bound))
    errors.push("commands: no bound validate command is declared");
  if (!rows.some((row) => row.kind === "negative_control" && row.bound))
    errors.push("commands: no bound negative-control command is declared");
  if (!rows.some((row) => row.kind === "build" && row.bound))
    errors.push("commands: no bound build command is declared");
  if (!rows.some((row) => row.kind === "run")) errors.push("commands: no run command is declared");
  return errors;
}

/**
 * Validate an already-parsed catalog against an already-parsed schema.
 * Exported separately from the file-reading entry point so the negative
 * controls can mutate an in-memory catalog and prove the mutation applied
 * before asserting the refusal.
 */
export function validateSemanticMemoryBudgetModel(
  catalog,
  schema,
  validateSchemaObject,
  packageRoot = PACKAGE_ROOT,
) {
  const errors = [...validateSchemaObject(catalog, schema, "catalogs.semantic-memory-budget")];
  errors.push(...budgetErrors(catalog));
  errors.push(...scaleErrors(catalog));
  errors.push(...baselineErrors(catalog));
  errors.push(...allocationErrors(catalog));
  errors.push(...metricErrors(catalog));
  errors.push(...fixtureErrors(catalog, packageRoot));
  errors.push(...workloadErrors(catalog, packageRoot));
  errors.push(...commandErrors(catalog));
  return errors;
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
