// Deterministic expander for the fixed aggregate-memory churn workload.
//
// The budget catalog holds the SPEC (projects, fixtures, per-tranche step
// order, configuration deltas, cancellation points); this module is the
// pure function that turns that spec into the ordered action manifest the
// long-churn runner executes. Nothing here reads the filesystem and
// nothing here is random: the same spec always expands to the same bytes,
// which is what lets the catalog pin the manifest by digest instead of
// committing ~800 KiB of generated rows.
//
// That pinning form is not new here. performance-gates.toml already pins
// its measurement corpus by the digest of the harness that synthesises it
// ("the harness source IS the corpus definition"), for the same reason: a
// digest that a different manifest cannot satisfy freezes the manifest
// exactly, and keeps the reviewable surface the spec rather than its
// expansion.

/** Column order of the serialized manifest. Part of the pinned bytes. */
export const MANIFEST_COLUMNS = [
  "seq",
  "tranche",
  "cycle",
  "kind",
  "class",
  "project",
  "fixture",
  "query",
  "delta",
  "cancel_at",
  "result_class",
  "admission_class",
];

/** Sentinel for a column an action kind does not carry. */
export const NONE = "none";

/**
 * The two alternating per-tranche cycles. Cycle b exists to reach the
 * "closed project with active readers" pressure boundary, which a single
 * hold/release/close order can never produce.
 */
export const CYCLES = ["a", "b"];

const EDIT_PREFIX = "edit:";

function pick(list, index) {
  return list[index % list.length];
}

/**
 * Expand `spec` into the ordered action list.
 *
 * `spec` is the parsed budget-catalog projection:
 *   { tranches, tranche_actions, projects_per_tranche, projects[],
 *     fixtures[], steps[], configuration_deltas[], cancellation_points[],
 *     kinds{} }
 *
 * Throws on any spec that cannot produce a well-formed manifest. A silent
 * truncation would make an omitted lifecycle class look like a passing
 * workload, which is the exact failure this contract has to catch.
 */
export function expandWorkload(spec) {
  const shape = validateSpecShape(spec);
  if (shape.length) throw new Error(`workload spec: ${shape[0]}`);

  const healthy = spec.fixtures.filter((fixture) => fixture.health === "healthy");
  const malformed = spec.fixtures.filter((fixture) => fixture.health === "malformed");
  const actions = [];
  let seq = 0;

  for (let tranche = 0; tranche < spec.tranches; tranche += 1) {
    const cycle = pick(CYCLES, tranche);
    const steps = spec.steps.filter((step) => step.cycle === cycle);
    const projects = [];
    for (let slot = 0; slot < spec.projects_per_tranche; slot += 1)
      projects.push(pick(spec.projects, tranche * spec.projects_per_tranche + slot).id);

    // Per-tranche identity pools. Cold identities are minted fresh every
    // tranche (that is the high-cardinality axis); warm and overlapping
    // requests re-enter the SAME tranche's cold pool, so they are
    // genuinely warm rather than a second cold miss wearing a warm label.
    const cold = [];
    const held = [];
    const edited = [];

    for (const step of steps) {
      for (let index = 0; index < step.count; index += 1) {
        seq += 1;
        const kind = step.kind;
        const descriptor = spec.kinds[kind];
        const pool = descriptor.fixture_pool === "malformed" ? malformed : healthy;
        const fixture = pick(pool, tranche * 7 + index);
        const action = {
          seq,
          tranche,
          cycle,
          kind,
          class: descriptor.class,
          project: pick(projects, index),
          fixture: fixture.path,
          query: NONE,
          delta: NONE,
          cancel_at: NONE,
          result_class: descriptor.result_class,
          admission_class: descriptor.admission_class,
        };

        switch (kind) {
          case "request_cold": {
            action.query = `component_meta:${fixture.path}#sym_${tranche}_${index}`;
            cold.push(action.query);
            break;
          }
          case "request_warm":
          case "request_overlapping": {
            action.query = pick(cold, index);
            action.fixture = fixturePathOfQuery(action.query);
            break;
          }
          case "request_cancelled": {
            action.query = `component_meta:${fixture.path}#cancel_${tranche}_${index}`;
            action.cancel_at = pick(spec.cancellation_points, tranche + index).id;
            break;
          }
          case "request_failed_construction": {
            action.query = `component_meta:${fixture.path}#failed_${tranche}_${index}`;
            break;
          }
          case "edit": {
            action.delta = `${EDIT_PREFIX}${fixture.path}@${tranche}.${index}`;
            edited.push(action.delta);
            break;
          }
          case "revert": {
            const target = pick(edited, index).slice(EDIT_PREFIX.length);
            action.delta = `revert:${target}`;
            action.fixture = target.slice(0, target.lastIndexOf("@"));
            break;
          }
          case "config_change": {
            // Even ordinals apply a declared provider-independent delta;
            // odd ordinals revert the one applied immediately before, so a
            // tranche never leaves configuration state behind it.
            const delta = pick(spec.configuration_deltas, tranche + Math.floor(index / 2));
            const applying = index % 2 === 0;
            action.delta = `config:${delta.id}:${applying ? "apply" : "revert"}`;
            action.result_class = applying ? "configuration_applied" : "configuration_reverted";
            break;
          }
          case "hold_result": {
            action.query = pick(cold, index);
            action.fixture = fixturePathOfQuery(action.query);
            held.push(action.query);
            break;
          }
          case "release_result": {
            action.query = pick(held, index);
            action.fixture = fixturePathOfQuery(action.query);
            break;
          }
          case "open_project":
          case "close_project": {
            action.project = projects[index % projects.length];
            if (kind === "close_project")
              action.result_class =
                cycle === "b" ? "closed_with_active_readers" : "closed_quiescent";
            break;
          }
          default:
            break;
        }

        actions.push(action);
      }
    }

    if (cold.length === 0) throw new Error(`tranche ${tranche} minted no cold identities`);
    if (edited.length === 0) throw new Error(`tranche ${tranche} performed no edit`);
    if (held.length === 0) throw new Error(`tranche ${tranche} pinned no result`);
  }

  return actions;
}

function fixturePathOfQuery(query) {
  return query.slice(query.indexOf(":") + 1, query.lastIndexOf("#"));
}

function validateSpecShape(spec) {
  const errors = [];
  if (!Number.isSafeInteger(spec?.tranches) || spec.tranches < 1)
    errors.push("tranches must be a positive integer");
  if (!Number.isSafeInteger(spec?.tranche_actions) || spec.tranche_actions < 1)
    errors.push("tranche_actions must be a positive integer");
  if (!Number.isSafeInteger(spec?.projects_per_tranche) || spec.projects_per_tranche < 1)
    errors.push("projects_per_tranche must be a positive integer");
  for (const key of [
    "projects",
    "fixtures",
    "steps",
    "configuration_deltas",
    "cancellation_points",
  ])
    if (!Array.isArray(spec?.[key]) || spec[key].length === 0)
      errors.push(`${key} must be a non-empty array`);
  if (!spec?.kinds || typeof spec.kinds !== "object") errors.push("kinds must be an object");
  if (errors.length) return errors;
  for (const cycle of CYCLES) {
    const steps = spec.steps.filter((step) => step.cycle === cycle);
    if (steps.length === 0) {
      errors.push(`cycle ${cycle} declares no steps`);
      continue;
    }
    const total = steps.reduce((sum, step) => sum + step.count, 0);
    if (total !== spec.tranche_actions)
      errors.push(`cycle ${cycle} declares ${total} actions, expected ${spec.tranche_actions}`);
  }
  for (const step of spec.steps)
    if (!spec.kinds[step.kind]) errors.push(`step names unknown kind ${step.kind}`);
  if (!spec.fixtures.some((fixture) => fixture.health === "healthy"))
    errors.push("fixtures declare no healthy member");
  if (!spec.fixtures.some((fixture) => fixture.health === "malformed"))
    errors.push("fixtures declare no malformed member");
  return errors;
}

/** Serialize `actions` to the exact bytes the catalog digest is taken over. */
export function serializeManifest(actions) {
  const lines = [MANIFEST_COLUMNS.join("\t")];
  for (const action of actions)
    lines.push(MANIFEST_COLUMNS.map((column) => String(action[column])).join("\t"));
  return `${lines.join("\n")}\n`;
}
