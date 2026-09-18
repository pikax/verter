#!/usr/bin/env node
// Validation for the public product-surface capability and performance catalog.
//
// Citation-integrity check over a contract document. It re-resolves locked
// cells, advertised docs/package entrypoints, executable command bindings
// and the ratification digest. It implements no product behaviour.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  CI_WORKFLOW,
  importedModules,
  laneCommandLine,
  triggerCovers,
  triggerPaths,
  workflowJobs,
} from "./closure-register.mjs";
import { readToml } from "./toml.mjs";

export const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = path.resolve(PACKAGE_ROOT, "..", "..");
export const CATALOG_RELATIVE = "catalogs/product-surface-catalog.toml";
export const SCHEMA_NAME = "product-surface-catalog.schema.json";
const GATE_PROFILES_RELATIVE = "catalogs/gate-profiles.toml";
const DAG_ROOT = path.join(PACKAGE_ROOT, "authority", "dag");

export const REQUIRED_SURFACE_IDS = Object.freeze([
  "vue.direct.compile.raw",
  "vue.direct.compile.css",
  "vue.managed.compile.raw",
  "vue.managed.compile.css",
  "vue.css.semantics",
  "svelte.direct.compile.raw",
  "svelte.direct.compile.css",
  "svelte.managed.compile.raw",
  "svelte.managed.compile.css",
  "svelte.css.semantics",
  "vue.tsc.project_check",
  "vue.tsc.correctness_wall",
  "svelte.tsc.project_check",
  "svelte.tsc.correctness_wall",
  "vue.component_meta.scalar",
  "vue.component_meta.batch",
  "svelte.component_meta.scalar",
  "svelte.component_meta.batch",
  "vue.host_lint",
  "svelte.host_lint",
  "vue.language_service.typing",
  "vue.language_service.edit",
  "svelte.language_service.typing",
  "svelte.language_service.edit",
]);

export const REQUIRED_ROW_KINDS = Object.freeze(["wall", "allocation", "process_tree_rss"]);
export const PRODUCT_ROOTS = Object.freeze([
  ["@verter/typeinfo", "packages/typeinfo/package.json"],
  ["@verter/component-meta", "packages/component-meta/package.json"],
  ["@verter/unplugin", "packages/unplugin/package.json"],
  ["@verter/nuxt", "packages/nuxt/package.json"],
  ["verter-lsp", "packages/verter-lsp/package.json"],
  ["verter-mcp", "packages/verter-mcp/package.json"],
  ["verter-tsc", "packages/verter-tsc/package.json"],
  ["verter-vscode", "packages/vue-vscode/package.json"],
]);
export const INDEX_FEATURES = Object.freeze([
  "Rust-Powered Compilation",
  "Full TypeScript Safety",
  "Universal Bundler Plugin",
  "Rust LSP Server",
  "VS Code Extension",
  "Built-in Diagnostics",
]);

const REQUIRED_LANE_FIELDS = ["gate_profile", "ci_job", "ci_filter"];
const ENFORCING_COMMAND_KINDS = ["validate", "negative_control"];
const STABILITY = new Set(["stable", "preview", "experimental-unranked"]);

let openedRepoPaths = null;

function recordOpened(relative) {
  if (openedRepoPaths) openedRepoPaths.add(relative);
}

function repoRelative(absolute) {
  const relative = path.relative(REPO_ROOT, absolute);
  if (!relative || path.isAbsolute(relative) || relative.startsWith("..")) return null;
  return relative.split(path.sep).join("/");
}

function recordOpenedAt(absolute) {
  const relative = repoRelative(absolute);
  if (relative) recordOpened(relative);
}

function recordModuleClosure(absolute) {
  const relative = repoRelative(absolute);
  if (!relative) return;
  recordOpened(relative);
  for (const imported of importedModules(REPO_ROOT, relative))
    if (fs.existsSync(path.join(REPO_ROOT, imported))) recordOpened(imported);
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

export function lockDigest(catalog) {
  const payload = {
    surfaces: [...(catalog.surface || [])]
      .map((s) => ({
        id: s.id,
        stability_class: s.stability_class,
        command_id: s.command_id,
        removing_node: s.removing_node || "",
      }))
      .sort((a, b) => a.id.localeCompare(b.id)),
    rows: [...(catalog.controlling_row || [])]
      .map((r) => ({
        surface_id: r.surface_id,
        kind: r.kind,
        metric: r.metric,
        comparison: r.comparison,
        limit: r.limit,
        source: r.source,
        cell_id: r.cell_id || "",
      }))
      .sort((a, b) => `${a.surface_id}|${a.kind}`.localeCompare(`${b.surface_id}|${b.kind}`)),
  };
  return crypto.createHash("sha256").update(JSON.stringify(payload)).digest("hex");
}

function dagNodeIds(packageRoot) {
  const ids = new Set();
  for (const name of fs.readdirSync(DAG_ROOT)) {
    if (!name.endsWith(".toml")) continue;
    const file = path.join(DAG_ROOT, name);
    recordOpenedAt(file);
    const model = readToml(file);
    for (const node of model.node || []) ids.add(node.id);
  }
  return ids;
}

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
    const profilesFile = path.join(packageRoot, GATE_PROFILES_RELATIVE);
    profiles = readToml(profilesFile).profile || [];
    recordOpenedAt(profilesFile);
  } catch (error) {
    errors.push(`commands: cannot read ${GATE_PROFILES_RELATIVE}: ${error.message}`);
  }
  let workflow = null;
  try {
    workflow = fs.readFileSync(workflowFile, "utf8");
    recordOpenedAt(workflowFile);
  } catch (error) {
    errors.push(`commands: cannot read the CI workflow: ${error.message}`);
  }
  const jobs = workflow === null ? new Map() : workflowJobs(workflow);
  const filters = new Map();

  for (const row of required) {
    const where = `command ${row.id}`;
    for (const token of row.command.split(/\s+/u))
      if (token.endsWith(".mjs") && fs.existsSync(path.join(REPO_ROOT, token)))
        recordModuleClosure(path.join(REPO_ROOT, token));
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

function surfaceErrors(catalog, nodeIds, deliveredTerminals) {
  const errors = [];
  const rows = catalog.surface || [];
  const ids = new Set();
  const commands = new Map((catalog.command || []).map((c) => [c.id, c]));
  for (const row of rows) {
    if (ids.has(row.id))
      errors.push(
        `surface: duplicate id ${row.id}; two catalogs or a second class authority is rejected`,
      );
    ids.add(row.id);
    if (!STABILITY.has(row.stability_class)) errors.push(`surface ${row.id}: missing class`);
    if (!row.command_id) errors.push(`surface ${row.id}: missing command`);
    else if (!commands.has(row.command_id))
      errors.push(`surface ${row.id}: command ${row.command_id} is not declared`);
    else if (row.stability_class === "stable" && !commands.get(row.command_id).command)
      errors.push(
        `surface ${row.id}: a surface with no executable command cannot hold the stable class`,
      );
    if (row.stability_class !== "stable") {
      if (!row.removing_node)
        errors.push(`surface ${row.id}: a non-stable class requires a removing node`);
      else if (!nodeIds.has(row.removing_node))
        errors.push(`surface ${row.id}: removing node ${row.removing_node} is not a DAG node`);
      if (!row.replacement_slo)
        errors.push(`surface ${row.id}: a non-stable class requires a replacement SLO`);
    }
    if (row.owning_terminal && !nodeIds.has(row.owning_terminal))
      errors.push(`surface ${row.id}: owning terminal ${row.owning_terminal} is not a DAG node`);
    if (row.stability_class === "stable" && deliveredTerminals && row.owning_terminal) {
      if (deliveredTerminals.get(row.owning_terminal) !== "implemented")
        errors.push(
          `surface ${row.id}: a proposed class would describe a surface as stable that its owning terminal has not yet delivered`,
        );
    }
  }
  for (const required of REQUIRED_SURFACE_IDS)
    if (!ids.has(required))
      errors.push(`surface: required advertised surface ${required} is absent`);
  return errors;
}

function controllingRowErrors(catalog, lockText) {
  const errors = [];
  const surfaces = new Map((catalog.surface || []).map((s) => [s.id, s]));
  const rows = catalog.controlling_row || [];
  const ids = new Set();
  const bySurface = new Map();
  for (const row of rows) {
    if (ids.has(row.id)) errors.push(`controlling_row: duplicate id ${row.id}`);
    ids.add(row.id);
    if (!surfaces.has(row.surface_id))
      errors.push(`controlling_row ${row.id}: unknown surface ${row.surface_id}`);
    const kinds = bySurface.get(row.surface_id) || new Set();
    if (kinds.has(row.kind))
      errors.push(`surface ${row.surface_id}: a second ${row.kind} controlling row is rejected`);
    kinds.add(row.kind);
    bySurface.set(row.surface_id, kinds);
    if (!Number.isSafeInteger(row.limit) || row.limit < 1)
      errors.push(`controlling_row ${row.id}: limit must be a finite integer`);
    if (row.comparison === "no_regression_milli_percent" && row.limit < 1)
      errors.push(`controlling_row ${row.id}: a statistical metric carries no implied 0.0% bound`);
    if (row.source === "performance-gates") {
      if (!row.cell_id)
        errors.push(`controlling_row ${row.id}: a performance-gates row must name its cell_id`);
      else {
        const metrics = lockedCellMetrics(lockText, row.cell_id);
        if (!metrics)
          errors.push(`controlling_row ${row.id}: locked cell ${row.cell_id} does not exist`);
        else {
          const key = `${row.metric}|${row.statistic}|${row.comparison === "no_regression_milli_percent" ? "no_regression_percent_max" : row.comparison}`;
          if (!metrics.has(key))
            errors.push(
              `controlling_row ${row.id}: cell ${row.cell_id} has no ${key} metric; a locked cell must not change to accommodate a row`,
            );
          else {
            const locked =
              row.comparison === "no_regression_milli_percent"
                ? Math.round(metrics.get(key) * 1000)
                : metrics.get(key);
            if (locked !== row.limit)
              errors.push(
                `controlling_row ${row.id}: limit ${row.limit} does not match locked ${key} = ${locked}; a locked cell must not change to accommodate a row`,
              );
          }
        }
      }
    } else if (row.cell_id) {
      errors.push(
        `controlling_row ${row.id}: a catalog-extension row must not name a locked cell_id`,
      );
    } else if (!row.derivation) {
      errors.push(`controlling_row ${row.id}: a catalog-extension row must state its derivation`);
    }
    if (row.kind === "process_tree_rss" && !/allocator/u.test(row.method))
      errors.push(`controlling_row ${row.id}: RSS method must include allocator slack`);
  }
  for (const surface of catalog.surface || []) {
    const kinds = bySurface.get(surface.id) || new Set();
    for (const kind of REQUIRED_ROW_KINDS)
      if (!kinds.has(kind)) {
        if (surface.stability_class === "stable")
          errors.push(`surface ${surface.id}: a stable class without a ${kind} limit is refused`);
        else errors.push(`surface ${surface.id}: missing controlling ${kind} row`);
      }
  }
  return errors;
}

function advertisedErrors(catalog) {
  const errors = [];
  const surfaces = new Set((catalog.surface || []).map((s) => s.id));
  const rows = catalog.advertised_entrypoint || [];
  const mappedPackages = new Set();
  const mappedFeatures = new Set();
  for (const row of rows) {
    if (!surfaces.has(row.surface_id))
      errors.push(
        `advertised_entrypoint ${row.path}: surface ${row.surface_id} is absent from the catalog`,
      );
    if (!repoPathExists(row.path))
      errors.push(`advertised_entrypoint: path does not resolve: ${row.path}`);
    else {
      const text = readRepoFile(row.path).toString("utf8");
      if (!text.includes(row.needle))
        errors.push(
          `advertised_entrypoint ${row.path}: needle ${JSON.stringify(row.needle)} is absent`,
        );
    }
    for (const [name, pkgPath] of PRODUCT_ROOTS) if (row.path === pkgPath) mappedPackages.add(name);
    for (const feature of INDEX_FEATURES)
      if (row.path === "docs/index.md" && row.needle === feature) mappedFeatures.add(feature);
  }
  for (const [name, pkgPath] of PRODUCT_ROOTS) {
    if (!mappedPackages.has(name))
      errors.push(`advertised package entrypoint ${name} (${pkgPath}) is absent from the catalog`);
  }
  for (const feature of INDEX_FEATURES)
    if (!mappedFeatures.has(feature))
      errors.push(
        `advertised documentation surface ${JSON.stringify(feature)} is absent from the catalog`,
      );
  return errors;
}

function baselineErrors(catalog, lockText) {
  const errors = [];
  const baseline = catalog.baseline || {};
  if (!lockText.includes('status = "LOCKED"')) errors.push("baseline: lock status is not LOCKED");
  for (const [needle, label] of [
    [`baseline_sha = "${baseline.baseline_sha}"`, "baseline sha"],
    [`baseline_tree = "${baseline.baseline_tree}"`, "baseline tree"],
    [`class = "${baseline.runner_class}"`, "runner class"],
    [`memory_bytes = ${baseline.runner_memory_bytes}`, "runner memory"],
  ])
    if (!lockText.includes(needle))
      errors.push(`baseline: ${label} is not cited by performance-gates.toml`);
  if ((catalog.measurement || {}).no_regression_floor_milli_percent < 1)
    errors.push("measurement: a statistical metric carries no implied 0.0% bound");
  if (!(catalog.measurement || {}).rss_includes_allocator_overhead)
    errors.push("measurement: RSS must include allocator overhead");
  if (!(catalog.measurement || {}).owned_bytes_excludes_allocator_overhead)
    errors.push("measurement: owned bytes must exclude allocator overhead");
  return errors;
}

export function validateProductSurfaceCatalogModel(
  catalog,
  schema,
  validateSchemaObject,
  packageRoot = PACKAGE_ROOT,
  { workflowFile = path.join(REPO_ROOT, CI_WORKFLOW) } = {},
) {
  openedRepoPaths = new Set();
  try {
    recordOpenedAt(path.join(packageRoot, CATALOG_RELATIVE));
    recordOpenedAt(path.join(packageRoot, "schemas", SCHEMA_NAME));
    recordModuleClosure(fileURLToPath(import.meta.url));
    const errors = [...validateSchemaObject(catalog, schema, "catalogs.product-surface-catalog")];
    if (catalog.lock_record && catalog.lock_record.digest !== lockDigest(catalog))
      errors.push(
        "lock_record: digest does not match the ratified surface/row set; a limit edited after ratification is refused",
      );
    const lockPath = (catalog.baseline || {}).source_file || "performance-gates.toml";
    let lockText = "";
    if (!repoPathExists(lockPath))
      errors.push(`baseline: cited lock file does not resolve: ${lockPath}`);
    else lockText = readRepoFile(lockPath).toString("utf8");
    const nodeIds = dagNodeIds(packageRoot);
    const ledgerFile = path.join(packageRoot, "authority", "state", "implemented.toml");
    let deliveredTerminals = new Map();
    try {
      const ledger = readToml(ledgerFile);
      recordOpenedAt(ledgerFile);
      for (const [id, record] of Object.entries(ledger.implementation || {}))
        deliveredTerminals.set(id, record.status);
    } catch {
      deliveredTerminals = null;
    }
    errors.push(...baselineErrors(catalog, lockText));
    errors.push(...surfaceErrors(catalog, nodeIds, deliveredTerminals));
    errors.push(...controllingRowErrors(catalog, lockText));
    errors.push(...advertisedErrors(catalog));
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

export function validateProductSurfaceCatalog(validateSchemaObject, packageRoot = PACKAGE_ROOT) {
  return validateProductSurfaceCatalogModel(
    loadCatalog(packageRoot),
    loadSchema(packageRoot),
    validateSchemaObject,
    packageRoot,
  );
}
