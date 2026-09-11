// Deterministic expander for the fixed aggregate-memory churn workload.
//
// The budget catalog holds the SPEC (projects, frozen source templates,
// the per-tranche materialization rule, the per-tranche step order, the
// exact edit and configuration deltas, cancellation points); this module
// is the pure function that turns that spec into the ordered action
// manifest the long-churn runner executes. Nothing here reads the
// filesystem and nothing here is random: the same spec always expands to
// the same bytes, which is what lets the catalog pin the manifest by
// digest instead of committing ~1 MiB of generated rows.
//
// That pinning form is not new here. performance-gates.toml already pins
// its measurement corpus by the digest of the harness that synthesises it
// ("the harness source IS the corpus definition"), for the same reason: a
// digest that a different manifest cannot satisfy freezes the manifest
// exactly, and keeps the reviewable surface the spec rather than its
// expansion.
//
// EXECUTABILITY IS THE DESIGN CONSTRAINT. Every request names a concrete
// materialized carrier file, because the request API takes a canonical
// file and nothing else. High key cardinality therefore comes from
// MATERIALIZING many distinct carrier files out of a few frozen
// templates -- the same technique the measurement corpus uses -- never
// from a synthetic symbol axis no API accepts. Requests draw only from the
// carrier pool; shared modules are edit targets whose edits invalidate the
// carriers that import them, and are never themselves requested.

/** Column order of the serialized manifest. Part of the pinned bytes. */
export const MANIFEST_COLUMNS = [
  "seq",
  "tranche",
  "cycle",
  "kind",
  "class",
  "project",
  "template",
  "instance",
  "query",
  "delta",
  "cancel_at",
  "overlap_group",
  "result_class",
  "cache_disposition",
];

/** Sentinel for a column an action kind does not carry. */
export const NONE = "none";

/**
 * The two alternating per-tranche cycles. Cycle b exists to reach the
 * "closed project with active readers" boundary, which a single
 * hold/release/close order can never produce.
 */
export const CYCLES = ["a", "b"];

/**
 * Cache dispositions. These describe what the CACHE is permitted or
 * required to do; they are deliberately separate from `result_class`,
 * which is the semantic outcome and is invariant under pressure.
 */
export const DISPOSITIONS = new Set([
  // No valid entry can exist for this key yet; this call constructs it.
  "must_construct",
  // Collapses onto a concurrent in-flight construction of the same key.
  "join_inflight",
  // Reuses a valid retained entry when one exists, otherwise recomputes.
  // Both outcomes must produce the same result.
  "reuse_if_valid",
  // A preceding mutation invalidated any prior entry for this key; serving
  // a pre-mutation entry is a defect, not a cache preference.
  "must_recompute",
  // Complete result, admitted iff it fits the active mode's per-entry cap;
  // otherwise returned complete and uncached.
  "admit_if_within_entry_cap",
  // Never admitted.
  "no_admission",
]);

const EDIT_PREFIX = "edit:";
const REVERT_PREFIX = "revert:";
const CONFIG_PREFIX = "config:";

function pick(list, index) {
  return list[index % list.length];
}

function pad(value, width) {
  return String(value).padStart(width, "0");
}

/**
 * `vue/card.vue` becomes `{ dir: "vue", stem: "card", ext: ".vue" }`.
 *
 * Template paths are relative to the catalog's `fixture_root`, and the
 * instance tree reproduces that relative layout verbatim, so a template's
 * `../shared/props-base` import resolves identically in every instance.
 */
function templateParts(templatePath) {
  const slash = templatePath.lastIndexOf("/");
  const dir = slash === -1 ? "" : templatePath.slice(0, slash);
  const base = templatePath.slice(slash + 1);
  const dot = base.lastIndexOf(".");
  return { dir, stem: base.slice(0, dot), ext: base.slice(dot) };
}

/**
 * The per-tranche materialized instance pool.
 *
 * The runner materializes exactly these paths by copying the frozen
 * template bytes (the oversize template additionally expanded by its
 * declared repetition factor). Relative layout is preserved, so the
 * templates' `../shared/...` imports resolve inside every instance tree
 * and each instance is a genuine cross-file resolution rather than an
 * isolated single-file request.
 */
export function trancheInstances(spec, tranche) {
  const root = spec.instance_root;
  const projects = [];
  for (let slot = 0; slot < spec.projects_per_tranche; slot += 1)
    projects.push(pick(spec.projects, tranche * spec.projects_per_tranche + slot).id);

  const byRole = (role, health) =>
    spec.fixtures.filter(
      (fixture) => fixture.role === role && (health === undefined || fixture.health === health),
    );
  const carriers = byRole("carrier", "healthy");
  const modules = byRole("module");
  const malformed = byRole("carrier", "malformed");
  const oversize = byRole("oversize_carrier");

  const instancePath = (project, template, ordinal) => {
    const { dir, stem, ext } = templateParts(template.path);
    const name = ordinal === null ? `${stem}${ext}` : `${stem}-${pad(ordinal, 2)}${ext}`;
    return `${root}/t${pad(tranche, 3)}/${project}/${dir}/${name}`;
  };

  // Requestable carrier instances, round-robined across projects (outer)
  // and templates (inner) so every project holds every template shape.
  const carrierPool = [];
  for (let index = 0; index < spec.carrier_instances_per_tranche; index += 1) {
    const project = projects[index % projects.length];
    const template = carriers[Math.floor(index / projects.length) % carriers.length];
    carrierPool.push({
      project,
      template: template.path,
      instance: instancePath(project, template, index),
    });
  }

  const perProject = (templates) => {
    const out = [];
    for (const project of projects)
      for (const template of templates)
        out.push({
          project,
          template: template.path,
          instance: instancePath(project, template, null),
        });
    return out;
  };

  return {
    projects,
    carriers: carrierPool,
    modules: perProject(modules),
    malformed: perProject(malformed),
    oversize: perProject(oversize),
  };
}

/** The one query form the request API accepts: a canonical carrier file. */
function queryFor(instance) {
  return `component_meta:${instance}`;
}

function countOf(steps, kind) {
  return steps.filter((step) => step.kind === kind).reduce((sum, step) => sum + step.count, 0);
}

/**
 * Expand `spec` into the ordered action list.
 *
 * Throws on any spec that cannot produce a well-formed manifest. A silent
 * truncation would make an omitted lifecycle class look like a passing
 * workload, which is the exact failure this contract has to catch.
 */
export function expandWorkload(spec) {
  const shape = validateSpecShape(spec);
  if (shape.length) throw new Error(`workload spec: ${shape[0]}`);

  const actions = [];
  let seq = 0;

  for (let tranche = 0; tranche < spec.tranches; tranche += 1) {
    const cycle = pick(CYCLES, tranche);
    const steps = spec.steps.filter((step) => step.cycle === cycle);
    const pool = trancheInstances(spec, tranche);

    // Disjoint slices of the carrier pool. Cold requests, overlap-group
    // subjects and cancelled requests never share a subject, so a warm
    // action is genuinely warm and an overlap group genuinely races on a
    // key nothing has yet constructed.
    const coldCount = countOf(steps, "request_cold");
    const overlapGroups = countOf(steps, "request_overlapping") / spec.overlap_group_size;
    const cancelCount = countOf(steps, "request_cancelled");
    const cold = pool.carriers.slice(0, coldCount);
    const overlapSubjects = pool.carriers.slice(coldCount, coldCount + overlapGroups);
    const cancelSubjects = pool.carriers.slice(
      coldCount + overlapGroups,
      coldCount + overlapGroups + cancelCount,
    );

    // Edit targets: every shared-module instance first (so a shared edit
    // invalidates the carriers importing it), then carrier instances taken
    // one template at a time, so every healthy carrier shape — Svelte as
    // well as Vue — goes through edit, post-mutation request and revert.
    const editCount = countOf(steps, "edit");
    const editTargets = [
      ...pool.modules,
      ...carrierEditTargets(cold, editCount - pool.modules.length),
    ].slice(0, editCount);
    const held = [];

    for (const step of steps) {
      for (let index = 0; index < step.count; index += 1) {
        seq += 1;
        const kind = step.kind;
        const descriptor = spec.kinds[kind];
        const action = {
          seq,
          tranche,
          cycle,
          kind,
          class: descriptor.class,
          project: NONE,
          template: NONE,
          instance: NONE,
          query: NONE,
          delta: NONE,
          cancel_at: NONE,
          overlap_group: NONE,
          result_class: descriptor.result_class,
          cache_disposition: descriptor.cache_disposition,
        };

        // The subject carries the project: an instance lives in exactly
        // one project's tree, so project identity is never chosen
        // independently of the thing being acted on.
        const bind = (subject) => {
          action.project = subject.project;
          action.template = subject.template;
          action.instance = subject.instance;
          action.query = queryFor(subject.instance);
        };

        switch (kind) {
          case "open_project":
          case "close_project": {
            action.project = pool.projects[index % pool.projects.length];
            if (kind === "close_project")
              action.result_class =
                cycle === "b" ? "closed_with_active_readers" : "closed_quiescent";
            break;
          }
          case "request_cold": {
            bind(cold[index]);
            break;
          }
          case "request_warm": {
            bind(pick(cold, index));
            break;
          }
          case "request_overlapping": {
            const group = Math.floor(index / spec.overlap_group_size);
            const member = index % spec.overlap_group_size;
            bind(overlapSubjects[group]);
            action.overlap_group = `og${pad(tranche, 3)}.${group}`;
            // Exactly one leader per group constructs; the rest join it.
            action.cache_disposition = member === 0 ? "must_construct" : "join_inflight";
            break;
          }
          case "request_cancelled": {
            bind(cancelSubjects[index]);
            action.cancel_at = pick(spec.cancellation_points, tranche + index).id;
            break;
          }
          case "request_failed_construction": {
            bind(pick(pool.malformed, index));
            break;
          }
          case "request_oversized": {
            bind(pick(pool.oversize, index));
            break;
          }
          case "edit":
          case "revert": {
            const target = editTargets[index];
            bind(target);
            const delta = deltaForTarget(spec, target, tranche, index);
            action.delta = `${kind === "edit" ? EDIT_PREFIX : REVERT_PREFIX}${delta.id}@${target.instance}`;
            break;
          }
          case "request_after_edit": {
            const target = editTargets[index];
            // A carrier edit invalidates itself; a shared-module edit
            // invalidates the carriers importing it, so the request goes
            // to a carrier in the SAME project.
            const subject = isModuleInstance(pool, target)
              ? cold.find((entry) => entry.project === target.project)
              : target;
            bind(subject);
            action.delta = `after:${target.instance}`;
            break;
          }
          case "config_change": {
            // Even ordinals apply; the next odd ordinal reverts the SAME
            // delta on the SAME project, so a tranche never leaves a
            // project configured.
            const pairIndex = Math.floor(index / spec.config_pair_size);
            const applying = index % spec.config_pair_size === 0;
            const delta = pick(spec.configuration_deltas, tranche + pairIndex);
            action.project = pool.projects[pairIndex % pool.projects.length];
            action.delta = `${CONFIG_PREFIX}${delta.id}:${action.project}:${applying ? "apply" : "revert"}`;
            action.result_class = applying ? "configuration_applied" : "configuration_reverted";
            break;
          }
          case "hold_result": {
            const subject = pick(cold, index);
            bind(subject);
            held.push(subject);
            break;
          }
          case "release_result": {
            bind(held[index]);
            break;
          }
          default:
            break;
        }

        actions.push(action);
      }
    }

    if (cold.length === 0) throw new Error(`tranche ${tranche} minted no cold identities`);
    if (editTargets.length === 0) throw new Error(`tranche ${tranche} performed no edit`);
    if (held.length === 0) throw new Error(`tranche ${tranche} pinned no result`);
  }

  return actions;
}

/**
 * `count` distinct cold carrier instances to edit, rotating over the
 * templates the cold pool holds in pool order. The k-th pick takes the
 * template at `slot = k % templates`, and within it the instance at
 * `(slot + round) % instances`: distinct across rounds while a template has
 * an instance per round, and — because the pool alternates projects within a
 * template — alternating projects across templates rather than editing one
 * project's instances only.
 */
function carrierEditTargets(cold, count) {
  const byTemplate = new Map();
  for (const entry of cold) {
    if (!byTemplate.has(entry.template)) byTemplate.set(entry.template, []);
    byTemplate.get(entry.template).push(entry);
  }
  const templates = [...byTemplate.values()];
  const targets = [];
  for (let pick = 0; pick < count; pick += 1) {
    const slot = pick % templates.length;
    const round = Math.floor(pick / templates.length);
    const entries = templates[slot];
    if (round >= entries.length)
      throw new Error(`the cold pool holds too few ${entries[0].template} instances to edit`);
    targets.push(entries[(slot + round) % entries.length]);
  }
  return targets;
}

function isModuleInstance(pool, target) {
  return pool.modules.some((entry) => entry.instance === target.instance);
}

/**
 * The exact byte edit applied to `target`. A delta only applies to the
 * template it was authored against, so the choice is made among the
 * deltas declared for THAT template rather than over all deltas.
 */
function deltaForTarget(spec, target, tranche, index) {
  const applicable = spec.edit_deltas.filter((delta) => delta.template === target.template);
  if (applicable.length === 0)
    throw new Error(`no edit delta is declared for template ${target.template}`);
  return pick(applicable, tranche + index);
}

function validateSpecShape(spec) {
  const errors = [];
  for (const key of [
    "tranches",
    "tranche_actions",
    "projects_per_tranche",
    "carrier_instances_per_tranche",
    "overlap_group_size",
    "config_pair_size",
  ])
    if (!Number.isSafeInteger(spec?.[key]) || spec[key] < 1)
      errors.push(`${key} must be a positive integer`);
  if (typeof spec?.instance_root !== "string" || spec.instance_root.length === 0)
    errors.push("instance_root must be a non-empty string");
  for (const key of [
    "projects",
    "fixtures",
    "steps",
    "configuration_deltas",
    "edit_deltas",
    "cancellation_points",
  ])
    if (!Array.isArray(spec?.[key]) || spec[key].length === 0)
      errors.push(`${key} must be a non-empty array`);
  if (!spec?.kinds || typeof spec.kinds !== "object") errors.push("kinds must be an object");
  if (errors.length) return errors;

  for (const role of ["carrier", "module", "oversize_carrier"])
    if (!spec.fixtures.some((fixture) => fixture.role === role))
      errors.push(`fixtures declare no ${role} member`);
  if (!spec.fixtures.some((f) => f.role === "carrier" && f.health === "healthy"))
    errors.push("fixtures declare no healthy carrier");
  if (!spec.fixtures.some((f) => f.role === "carrier" && f.health === "malformed"))
    errors.push("fixtures declare no malformed carrier");

  for (const cycle of CYCLES) {
    const steps = spec.steps.filter((step) => step.cycle === cycle);
    if (steps.length === 0) {
      errors.push(`cycle ${cycle} declares no steps`);
      continue;
    }
    const total = steps.reduce((sum, step) => sum + step.count, 0);
    if (total !== spec.tranche_actions)
      errors.push(`cycle ${cycle} declares ${total} actions, expected ${spec.tranche_actions}`);
    const overlapping = countOf(steps, "request_overlapping");
    if (overlapping < spec.overlap_group_size || overlapping % spec.overlap_group_size !== 0)
      errors.push(
        `cycle ${cycle} declares ${overlapping} overlapping actions, which is not a whole number of groups of ${spec.overlap_group_size}`,
      );
    const config = countOf(steps, "config_change");
    if (config % spec.config_pair_size !== 0)
      errors.push(
        `cycle ${cycle} declares ${config} configuration actions, which is not a whole number of apply/revert pairs`,
      );
    if (countOf(steps, "edit") !== countOf(steps, "revert"))
      errors.push(`cycle ${cycle} does not revert exactly the edits it applies`);
    if (countOf(steps, "request_after_edit") !== countOf(steps, "edit"))
      errors.push(`cycle ${cycle} does not request every edited subject after its edit`);
    if (countOf(steps, "hold_result") !== countOf(steps, "release_result"))
      errors.push(`cycle ${cycle} does not release exactly the results it pins`);
    // The three request families take disjoint slices of one pool, so the
    // pool has to be able to supply all of them.
    const demanded =
      countOf(steps, "request_cold") +
      overlapping / spec.overlap_group_size +
      countOf(steps, "request_cancelled");
    if (demanded > spec.carrier_instances_per_tranche)
      errors.push(
        `cycle ${cycle} demands ${demanded} distinct carrier instances but only ${spec.carrier_instances_per_tranche} are materialized`,
      );
  }
  for (const step of spec.steps)
    if (!spec.kinds[step.kind]) errors.push(`step names unknown kind ${step.kind}`);
  for (const [kind, descriptor] of Object.entries(spec.kinds))
    if (!DISPOSITIONS.has(descriptor.cache_disposition))
      errors.push(
        `kind ${kind} declares unknown cache disposition ${descriptor.cache_disposition}`,
      );
  return errors;
}

/** Serialize `actions` to the exact bytes the catalog digest is taken over. */
export function serializeManifest(actions) {
  const lines = [MANIFEST_COLUMNS.join("\t")];
  for (const action of actions)
    lines.push(MANIFEST_COLUMNS.map((column) => String(action[column])).join("\t"));
  return `${lines.join("\n")}\n`;
}
