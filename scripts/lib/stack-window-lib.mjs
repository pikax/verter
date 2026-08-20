// Shared model for the rev11 stack-window contract (contracts/stacked-prs.md)
// consumed by BOTH scripts/validate-stack-window.mjs (its own CLI) and
// scripts/validate-program-state.mjs (the composite checkpoint-exception
// check named in AMD-001). One model, two entry points — never a second,
// divergent implementation of "is this a legal ATOMIC_REVIEW window".
//
// Exit/violation-reporting stays in each CLI; everything here is pure
// (parsed TOML in, violation strings out) so both callers and their tests
// exercise the identical rule set.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { TomlError, parseToml } from "./rev11-toml.mjs";

export const SHA_RE = /^[0-9a-f]{40}$/; // full lowercase git object id
export const DIGEST_RE = /^[0-9a-f]{64}$/; // lowercase SHA-256
const PLACEHOLDER_RE = /^REQUIRED_/;

const WINDOW_MODE_ENUM = new Set(["LANDABLE", "ATOMIC_REVIEW"]); // stacked-prs.md 3.1 / 3.2 — closed set
const LAYER_KIND_ENUM = new Set(["mergeable", "NON_MERGEABLE_PRIVATE_LAYER"]); // stacked-prs.md 3.2

const TOP_LEVEL_STRING_FIELDS = [
  "status",
  "mode",
  "stack_id",
  "root_branch",
  "stack_tool",
  "stack_tool_version",
  "landing_mode",
  "owner",
  "evidence_root",
];
// acceptance_block_id is a string too but may legally be "" (LANDABLE) — checked separately.
const TOP_LEVEL_DIGEST_FIELDS = [
  "authority_package_digest",
  "implementation_lock_digest",
  "program_state_basis_digest",
];
const LAYER_REQUIRED_STRING_FIELDS = [
  "layer_id",
  "block_id",
  "kind",
  "branch",
  "base_branch",
  "worktree",
  "worker",
  "ci_state",
  "review_state",
];
// pr_url/notes are legitimately empty before a PR exists / with nothing to
// note (see templates/stack-window.template.toml) — string-typed, not
// required-non-empty.
const LAYER_OPTIONAL_STRING_FIELDS = ["pr_url", "notes"];
// base_sha/base_tree/head_sha/head_tree/patch_digest/generated_digest/evidence_digest
// checked separately (well-formed-or-empty, not-required-non-empty).

function isDigest(s) {
  return typeof s === "string" && DIGEST_RE.test(s);
}
function isSha(s) {
  return typeof s === "string" && SHA_RE.test(s);
}
function isPlaceholder(s) {
  return typeof s === "string" && PLACEHOLDER_RE.test(s);
}

// `mode` here is the CLI thoroughness mode ("template" | "live"), NOT the
// window's own contract `mode` field (LANDABLE | ATOMIC_REVIEW) — same
// template/live split as validate-program-state.mjs: template mode accepts
// an unresolved `REQUIRED_*` placeholder wherever live mode requires the
// real resolved value.
export function validateStackWindowStructure(window, { cliMode, dagClassMap, label }) {
  const v = [];
  const push = (msg) => v.push(msg);

  for (const key of ["schema", "revision"]) {
    if (!(key in window)) push(`${label} is missing required top-level key ${JSON.stringify(key)}`);
  }
  for (const key of TOP_LEVEL_STRING_FIELDS) {
    if (typeof window[key] !== "string" || window[key] === "") {
      push(`${label} top-level ${key} is not a non-empty string: ${JSON.stringify(window[key] ?? "")}`);
    }
  }
  if (typeof window.acceptance_block_id !== "string") {
    push(`${label} top-level acceptance_block_id is not a string`);
  }
  if (!Array.isArray(window.shared_writer_surfaces)) {
    push(`${label} top-level shared_writer_surfaces is not an array`);
  }
  if (!Array.isArray(window.integration_commands)) {
    push(`${label} top-level integration_commands is not an array`);
  }
  if (typeof window.notes !== "string") {
    push(`${label} top-level notes is not a string`);
  }

  if (typeof window.mode !== "string" || !WINDOW_MODE_ENUM.has(window.mode)) {
    push(
      `${label} top-level mode ${JSON.stringify(window.mode ?? "")} is outside the declared enum {LANDABLE, ATOMIC_REVIEW} (contracts/stacked-prs.md 3.1/3.2)`,
    );
  }

  if (cliMode === "live" && window.status === "TEMPLATE") {
    push(`${label} live window still carries status = "TEMPLATE"`);
  }

  const resolvedOrPlaceholder = (val) =>
    cliMode === "template" ? isDigest(val) || isPlaceholder(val) : isDigest(val);
  for (const field of TOP_LEVEL_DIGEST_FIELDS) {
    if (!resolvedOrPlaceholder(window[field])) {
      push(
        `${label} top-level ${field} is not a ${cliMode === "template" ? "resolved SHA-256 or REQUIRED_* placeholder" : "resolved 64-char lowercase SHA-256"}: ${JSON.stringify(window[field] ?? "")}`,
      );
    }
  }
  // previous_stack_snapshot_digest is the one field with a THIRD legal value
  // (contracts/stacked-prs.md 2 — "NOT_APPLICABLE ... for the first window").
  {
    const val = window.previous_stack_snapshot_digest;
    const ok =
      val === "NOT_APPLICABLE" || isDigest(val) || (cliMode === "template" && isPlaceholder(val));
    if (!ok) {
      push(
        `${label} top-level previous_stack_snapshot_digest is not "NOT_APPLICABLE", a resolved SHA-256${cliMode === "template" ? ", or a REQUIRED_* placeholder" : ""}: ${JSON.stringify(val ?? "")}`,
      );
    }
  }
  {
    const shaOk =
      cliMode === "template"
        ? isSha(window.root_base_sha) || isPlaceholder(window.root_base_sha)
        : isSha(window.root_base_sha);
    if (!shaOk) {
      push(`${label} top-level root_base_sha is not a resolved 40-char lowercase git object id: ${JSON.stringify(window.root_base_sha ?? "")}`);
    }
    const treeOk =
      cliMode === "template"
        ? isSha(window.root_base_tree) || isPlaceholder(window.root_base_tree)
        : isSha(window.root_base_tree);
    if (!treeOk) {
      push(`${label} top-level root_base_tree is not a resolved 40-char lowercase tree object id: ${JSON.stringify(window.root_base_tree ?? "")}`);
    }
  }

  // stacked-prs.md 4 — "Default maximum: four open review layers per stack.
  // A6 locks a value from two through six ... More than six requires an ADR
  // amendment."
  if (!(Number.isInteger(window.max_open_layers) && window.max_open_layers >= 2 && window.max_open_layers <= 6)) {
    push(
      `${label} top-level max_open_layers ${JSON.stringify(window.max_open_layers ?? "")} is not an integer in [2, 6] (contracts/stacked-prs.md 4)`,
    );
  }

  const layers = Array.isArray(window.layer) ? window.layer : null;
  if (layers === null || layers.length === 0) {
    push(`${label} declares no [[layer]] entries — a stack window must have at least one layer`);
    return v; // nothing further to check without layers
  }
  if (Number.isInteger(window.max_open_layers) && layers.length > window.max_open_layers) {
    push(
      `${label} declares ${layers.length} layer(s), exceeding its own max_open_layers = ${window.max_open_layers} (contracts/stacked-prs.md 4)`,
    );
  }

  const layerIds = new Set();
  const blockIdCounts = new Map();
  const indices = new Set();
  for (const [i, layer] of layers.entries()) {
    const where = `${label} layer[${i}]`;
    for (const field of LAYER_REQUIRED_STRING_FIELDS) {
      if (typeof layer[field] !== "string" || layer[field] === "") {
        push(`${where} ${field} is not a non-empty string: ${JSON.stringify(layer[field] ?? "")}`);
      }
    }
    for (const field of LAYER_OPTIONAL_STRING_FIELDS) {
      if (typeof layer[field] !== "string") {
        push(`${where} ${field} is not a string: ${JSON.stringify(layer[field] ?? "")}`);
      }
    }
    if (typeof layer.layer_id === "string" && layer.layer_id !== "") {
      if (layerIds.has(layer.layer_id)) {
        push(`${label} declares duplicate layer_id ${JSON.stringify(layer.layer_id)} (contracts/stacked-prs.md 3.2 — "unique layer_id values")`);
      }
      layerIds.add(layer.layer_id);
    }
    if (typeof layer.block_id === "string" && layer.block_id !== "") {
      blockIdCounts.set(layer.block_id, (blockIdCounts.get(layer.block_id) ?? 0) + 1);
    }
    if (!Number.isInteger(layer.index) || layer.index < 1) {
      push(`${where} index is not a positive integer: ${JSON.stringify(layer.index ?? "")}`);
    } else {
      if (indices.has(layer.index)) push(`${label} declares duplicate layer index ${layer.index}`);
      indices.add(layer.index);
    }
    if (typeof layer.kind !== "string" || !LAYER_KIND_ENUM.has(layer.kind)) {
      push(
        `${where} kind ${JSON.stringify(layer.kind ?? "")} is outside the declared enum {mergeable, NON_MERGEABLE_PRIVATE_LAYER} (contracts/stacked-prs.md 3.2)`,
      );
    }
    if (typeof layer.pr_number !== "number" || !Number.isInteger(layer.pr_number) || layer.pr_number < 0) {
      push(`${where} pr_number is not a non-negative integer: ${JSON.stringify(layer.pr_number ?? "")}`);
    }
    if (typeof layer.mergeable !== "boolean") {
      push(`${where} mergeable is not a boolean: ${JSON.stringify(layer.mergeable ?? "")}`);
    }
    if (typeof layer.charter_digest !== "string" || !resolvedOrPlaceholder(layer.charter_digest)) {
      push(`${where} charter_digest is not a resolved SHA-256${cliMode === "template" ? " or REQUIRED_* placeholder" : ""}: ${JSON.stringify(layer.charter_digest ?? "")}`);
    }
    // The remaining identity fields legally start empty (PENDING layer, not
    // yet built) — well-formed-or-empty, never required non-empty.
    for (const field of ["base_sha", "head_sha"]) {
      const val = layer[field];
      if (val !== "" && !isSha(val) && !(cliMode === "template" && isPlaceholder(val))) {
        push(`${where} ${field} is not a resolved 40-char lowercase git object id or empty: ${JSON.stringify(val ?? "")}`);
      }
    }
    for (const field of ["base_tree", "head_tree"]) {
      const val = layer[field];
      if (val !== "" && !isSha(val) && !(cliMode === "template" && isPlaceholder(val))) {
        push(`${where} ${field} is not a resolved 40-char lowercase tree object id or empty: ${JSON.stringify(val ?? "")}`);
      }
    }
    for (const field of ["patch_digest", "generated_digest", "evidence_digest"]) {
      const val = layer[field];
      if (val !== "" && !isDigest(val)) {
        push(`${where} ${field} is not a resolved 64-char lowercase SHA-256 or empty: ${JSON.stringify(val ?? "")}`);
      }
    }
  }

  // -- Mode-specific rules (stacked-prs.md 3.1 / 3.2)
  if (window.mode === "LANDABLE") {
    if (window.acceptance_block_id !== "") {
      push(`${label} mode is LANDABLE but acceptance_block_id is non-empty ${JSON.stringify(window.acceptance_block_id)} (contracts/stacked-prs.md 3.1 — "acceptance_block_id is empty")`);
    }
    for (const [blockId, count] of blockIdCounts) {
      if (count > 1) {
        push(`${label} mode is LANDABLE but block_id ${JSON.stringify(blockId)} appears ${count} times (contracts/stacked-prs.md 3.1 — "each block_id appears once")`);
      }
    }
  } else if (window.mode === "ATOMIC_REVIEW") {
    if (typeof window.acceptance_block_id !== "string" || window.acceptance_block_id === "") {
      push(`${label} mode is ATOMIC_REVIEW but acceptance_block_id is empty (contracts/stacked-prs.md 3.2 — "names the sole program block that may become accepted/landed from this window")`);
    } else {
      const acceptanceId = window.acceptance_block_id;
      const mergeableLayers = layers.filter((l) => l.kind === "mergeable");
      const acceptanceMergeable = mergeableLayers.filter((l) => l.block_id === acceptanceId);
      if (mergeableLayers.length !== 1 || acceptanceMergeable.length !== 1) {
        push(
          `${label} mode is ATOMIC_REVIEW but does not have exactly one layer with kind = "mergeable" whose block_id = acceptance_block_id ${JSON.stringify(acceptanceId)} — found ${mergeableLayers.length} mergeable layer(s) (contracts/stacked-prs.md 3.2 — "exactly one final mergeable layer ... becomes the reviewed candidate")`,
        );
      }
      for (const layer of layers) {
        if (layer.block_id === acceptanceId) continue;
        if (layer.kind !== "NON_MERGEABLE_PRIVATE_LAYER") {
          push(
            `${label} layer ${JSON.stringify(layer.layer_id ?? "")} (block_id ${JSON.stringify(layer.block_id ?? "")}) is not the acceptance block but kind is ${JSON.stringify(layer.kind ?? "")}, not NON_MERGEABLE_PRIVATE_LAYER — no intermediate layer may be released or merged to trunk (contracts/stacked-prs.md 3.2)`,
          );
        }
        // "private layers may repeat the acceptance block's block_id as
        // internal checkpoints, or name an explicit foundational-private-
        // checkpoint predecessor such as D1" — a class map (from --dag) lets
        // us check the latter; without one, this narrower rule is skipped
        // (the caller — validate-program-state.mjs's own DAG validation, or
        // this validator's --dag flag — already asserts DAG well-formedness).
        if (dagClassMap && typeof layer.block_id === "string" && layer.block_id !== "") {
          const cls = dagClassMap.get(layer.block_id);
          if (cls !== "foundational-private-checkpoint" && layer.block_id !== acceptanceId) {
            push(
              `${label} layer ${JSON.stringify(layer.layer_id ?? "")} names block_id ${JSON.stringify(layer.block_id)}, whose DAG class is ${JSON.stringify(cls ?? "")} — a private ATOMIC_REVIEW layer must repeat the acceptance block's own id or name a block whose DAG class is "foundational-private-checkpoint" (contracts/stacked-prs.md 3.2)`,
            );
          }
        }
      }
    }
  }

  return v;
}

// -- Composite cross-validation: the mutable ledger (program-state.toml)
// against the immutable snapshot (a validated stack-window file). Named
// `--current-program-state` in contracts/stacked-prs.md 2. `snapshotDigest`
// is the SHA-256 of the fully resolved window file — the immutable
// StackSnapshotId (contracts/stacked-prs.md 2).
export function crossValidateAgainstProgramState({ window, label, snapshotDigest, stateById }) {
  const v = [];
  const layers = Array.isArray(window.layer) ? window.layer : [];
  for (const layer of layers) {
    const blockId = layer.block_id;
    if (typeof blockId !== "string" || blockId === "") continue; // already reported by structural check
    const row = stateById.get(blockId);
    if (!row) {
      v.push(`${label} layer ${JSON.stringify(layer.layer_id ?? "")} names block_id ${JSON.stringify(blockId)}, which does not exist in the program-state ledger`);
      continue;
    }
    if (row.stack_id !== window.stack_id) {
      v.push(
        `${label} block ${blockId} ledger stack_id ${JSON.stringify(row.stack_id ?? "")} does not match window stack_id ${JSON.stringify(window.stack_id)} — the mutable ledger and the immutable snapshot have diverged`,
      );
    }
    if (!(typeof row.stack_snapshot_digest === "string" && row.stack_snapshot_digest === snapshotDigest)) {
      v.push(
        `${label} block ${blockId} ledger stack_snapshot_digest ${JSON.stringify(row.stack_snapshot_digest ?? "")} does not match the SHA-256 of the validated stack-window file (${snapshotDigest}) — the mutable ledger and the immutable snapshot have diverged`,
      );
    }
    if (row.stack_layer !== layer.index) {
      v.push(
        `${label} block ${blockId} ledger stack_layer ${JSON.stringify(row.stack_layer ?? "")} does not match window layer index ${JSON.stringify(layer.index ?? "")} — the mutable ledger and the immutable snapshot have diverged`,
      );
    }
    // contracts/stacked-prs.md 3.2 — "an explicit program checkpoint such as
    // D1 whose PRIVATE_CHECKPOINT state is valid only for the final
    // acceptance block": a NON_MERGEABLE_PRIVATE_LAYER standing for a real
    // program checkpoint (not an internal sublayer of the acceptance block
    // itself) must be recorded in PRIVATE_CHECKPOINT — never landed/accepted
    // independently.
    if (
      layer.kind === "NON_MERGEABLE_PRIVATE_LAYER" &&
      blockId !== window.acceptance_block_id &&
      row.status !== "PRIVATE_CHECKPOINT"
    ) {
      v.push(
        `${label} block ${blockId} is a NON_MERGEABLE_PRIVATE_LAYER checkpoint but ledger status is ${JSON.stringify(row.status ?? "")}, not PRIVATE_CHECKPOINT — a checkpoint layer never lands independently (contracts/stacked-prs.md 3.2)`,
      );
    }
  }
  return v;
}

// Minimal program-state block index — id -> row. Full ledger structural
// validation (duplicate ids, closed status enum, sequencing, ...) stays
// validate-program-state.mjs's job; a cross-validating caller only needs the
// id -> row lookup.
export function buildStateById(state) {
  const map = new Map();
  const blocks = Array.isArray(state.block) ? state.block : [];
  for (const b of blocks) {
    if (typeof b.id !== "string" || b.id === "") continue;
    map.set(b.id, b);
  }
  return map;
}

export function buildDagClassMap(dag) {
  const map = new Map();
  const blocks = Array.isArray(dag.block) ? dag.block : [];
  for (const b of blocks) {
    if (typeof b.id !== "string" || b.id === "") continue;
    map.set(b.id, typeof b.class === "string" ? b.class : "");
  }
  return map;
}

// -- The AMD-001 §2 acceptance rule, as a composite check callable from
// validate-program-state.mjs's PRIVATE_CHECKPOINT-predecessor sequencing
// gate. This is the SOLE model of "does a stack window legalize this
// PRIVATE_CHECKPOINT predecessor" — validate-program-state.mjs must not grow
// a second, parallel notion of the same question.
//
// Returns { ok: true } when the exception is established, or
// { ok: false, problems: string[] } — each problem names its OWN distinct
// cause (missing/unreadable/unparseable window; failed structural
// validation; wrong acceptance_block_id; failed cross-validation, which
// covers a mismatched snapshot digest and a checkpoint layer whose ledger
// row is not PRIVATE_CHECKPOINT).
export function evaluateCheckpointException({ windowPath, predecessorId, successorId, stateById, dagById }) {
  const problems = [];
  let text;
  try {
    text = readFileSync(windowPath, "utf8");
  } catch (err) {
    return { ok: false, problems: [`stack-window file ${windowPath} could not be read: ${err.message}`] };
  }
  let window;
  try {
    window = parseToml(text, windowPath);
  } catch (err) {
    if (err instanceof TomlError) {
      return { ok: false, problems: [`stack-window file ${windowPath}: ${err.message}`] };
    }
    throw err;
  }

  const dagClassMap = new Map();
  if (dagById) {
    for (const [id, b] of dagById) {
      dagClassMap.set(id, typeof b?.class === "string" ? b.class : "");
    }
  }

  const structural = validateStackWindowStructure(window, {
    cliMode: "live",
    dagClassMap,
    label: windowPath,
  });
  problems.push(...structural);

  if (window.mode !== "ATOMIC_REVIEW") {
    problems.push(
      `stack-window ${windowPath} mode is ${JSON.stringify(window.mode ?? "")}, not ATOMIC_REVIEW — a PRIVATE_CHECKPOINT predecessor is legalized only inside the ATOMIC_REVIEW canonical case (contracts/stacked-prs.md 3.2)`,
    );
  }
  if (window.acceptance_block_id !== successorId) {
    problems.push(
      `stack-window ${windowPath} acceptance_block_id is ${JSON.stringify(window.acceptance_block_id ?? "")}, not the successor block ${JSON.stringify(successorId)} (AMD-001 §2 — the exception is granted "ONLY when ... the same validated ATOMIC_REVIEW snapshot whose acceptance_block_id is D2")`,
    );
  }

  const layers = Array.isArray(window.layer) ? window.layer : [];
  const predecessorLayer = layers.find((l) => l.block_id === predecessorId);
  if (!predecessorLayer) {
    problems.push(`stack-window ${windowPath} declares no layer for predecessor block ${JSON.stringify(predecessorId)}`);
  } else if (predecessorLayer.kind !== "NON_MERGEABLE_PRIVATE_LAYER") {
    problems.push(
      `stack-window ${windowPath} layer for predecessor block ${JSON.stringify(predecessorId)} has kind ${JSON.stringify(predecessorLayer.kind ?? "")}, not NON_MERGEABLE_PRIVATE_LAYER — a checkpoint predecessor's layer must never be independently mergeable (contracts/stacked-prs.md 3.2, "no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor")`,
    );
  }

  // Only cross-validate identity binding once the window is at least
  // internally coherent enough to compute a meaningful snapshot digest and
  // name a real predecessor layer — otherwise the cross-check would just
  // restate the structural problems above under a different label.
  if (structural.length === 0 && predecessorLayer) {
    const snapshotDigest = createHash("sha256").update(text).digest("hex");
    const cross = crossValidateAgainstProgramState({ window, label: windowPath, snapshotDigest, stateById });
    problems.push(...cross);
    if (
      predecessorLayer.index !== undefined &&
      window.acceptance_block_id === successorId
    ) {
      const acceptanceLayer = layers.find((l) => l.block_id === successorId && l.kind === "mergeable");
      if (acceptanceLayer && !(predecessorLayer.index < acceptanceLayer.index)) {
        problems.push(
          `stack-window ${windowPath} predecessor layer index ${predecessorLayer.index} is not below acceptance layer index ${acceptanceLayer.index} for block ${JSON.stringify(successorId)}`,
        );
      }
    }
  }

  return problems.length === 0 ? { ok: true } : { ok: false, problems };
}
