import fs from "node:fs";
import { parseToml, serializeInlineTable } from "./toml.mjs";

// Trusted implementation ledger, schema 2.
//
// Every DAG node is predeclared exactly once under [implementation]:
//
//   "D4" = { status = "pending" }
//   "A0" = { status = "implemented", commit_message = "...", commit_date = "...", pull_request = 100 }
//
// Row status is the complete implementation fact (authority: the
// trusted-implementation-ledger decision). Evidence fields are loose human
// locators; tooling never validates them against Git or GitHub. Pending rows
// carry no evidence. Unknown nodes are invalid. Serialization is canonical
// and deterministic: independent node transitions produce independent
// one-line diffs, which is what makes concurrent landings mechanically
// mergeable.

export const LEDGER_SCHEMA_VERSION = 2;
export const STATUS_PENDING = "pending";
export const STATUS_IMPLEMENTED = "implemented";

export const NODE_ID_PATTERN = /^[A-Z][A-Z0-9-]*$/u;
export const COMMIT_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:\d{2})$/u;

const RECORD_FIELDS = ["status", "commit_message", "commit_date", "pull_request"];

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/**
 * Validate one implementation record. Returns error strings (empty = valid).
 */
export function recordErrors(nodeId, record, location = "implementation ledger") {
  const errors = [];
  const at = `${location}: ${nodeId}`;
  if (!NODE_ID_PATTERN.test(nodeId)) errors.push(`${at}: invalid node id`);
  if (!isPlainObject(record)) {
    errors.push(`${at}: record must be an inline table`);
    return errors;
  }
  for (const key of Object.keys(record))
    if (!RECORD_FIELDS.includes(key)) errors.push(`${at}: unknown field ${key}`);
  if (record.status === STATUS_PENDING) {
    for (const key of Object.keys(record))
      if (key !== "status") errors.push(`${at}: pending rows carry no ${key}`);
  } else if (record.status === STATUS_IMPLEMENTED) {
    if (typeof record.commit_message !== "string" || record.commit_message.length === 0)
      errors.push(`${at}: implemented rows require commit_message`);
    if (typeof record.commit_date !== "string" || !COMMIT_DATE_PATTERN.test(record.commit_date))
      errors.push(`${at}: implemented rows require a timezone-bearing commit_date`);
    if (
      record.pull_request !== undefined &&
      (!Number.isSafeInteger(record.pull_request) || record.pull_request < 1)
    )
      errors.push(`${at}: pull_request must be a positive integer`);
  } else {
    errors.push(`${at}: status must be "pending" or "implemented"`);
  }
  return errors;
}

/**
 * Structural + optional DAG-membership validation of a parsed ledger
 * document. `knownNodeIds` (Set|null): when given, every known node must be
 * predeclared exactly once and unknown nodes are invalid.
 */
export function ledgerErrors(parsed, { knownNodeIds = null, location = "implementation ledger" } = {}) {
  const errors = [];
  if (!isPlainObject(parsed)) return [`${location}: document must be a table`];
  if (parsed.schema !== LEDGER_SCHEMA_VERSION)
    errors.push(`${location}: schema must be ${LEDGER_SCHEMA_VERSION}`);
  if (!isPlainObject(parsed.implementation)) {
    errors.push(`${location}: missing [implementation] table`);
    return errors;
  }
  for (const [nodeId, record] of Object.entries(parsed.implementation)) {
    errors.push(...recordErrors(nodeId, record, location));
    if (knownNodeIds && !knownNodeIds.has(nodeId))
      errors.push(`${location}: unknown node ${nodeId}`);
  }
  if (knownNodeIds)
    for (const nodeId of knownNodeIds)
      if (!Object.hasOwn(parsed.implementation, nodeId))
        errors.push(`${location}: missing predeclared node ${nodeId}`);
  return errors;
}

/** Parse ledger text into the raw document model. Throws on TOML garbage. */
export function parseLedgerText(text) {
  const parsed = parseToml(text);
  const structural = ledgerErrors(parsed);
  if (structural.length) throw new Error(structural.join("; "));
  return parsed;
}

export function readLedgerFile(file) {
  const text = fs.readFileSync(file, "utf8");
  try {
    return { text, parsed: parseLedgerText(text) };
  } catch (error) {
    throw new Error(`${file}: ${error.message}`);
  }
}

/**
 * Legacy-shaped implemented rows derived from the implementation table:
 * [{ node_id, commit_message, commit_date, pull_request? }], sorted by
 * node_id. This is the in-memory view deriveState and githubctl consume.
 */
export function implementedRows(parsed) {
  const rows = [];
  for (const [nodeId, record] of Object.entries(parsed.implementation || {})) {
    if (!isPlainObject(record) || record.status !== STATUS_IMPLEMENTED) continue;
    const row = {
      node_id: nodeId,
      commit_message: record.commit_message,
      commit_date: record.commit_date,
    };
    if (record.pull_request !== undefined) row.pull_request = record.pull_request;
    rows.push(row);
  }
  return rows.sort((left, right) => (left.node_id < right.node_id ? -1 : left.node_id > right.node_id ? 1 : 0));
}

function canonicalRecord(record) {
  const out = { status: record.status };
  if (record.status === STATUS_IMPLEMENTED) {
    out.commit_message = record.commit_message;
    out.commit_date = record.commit_date;
    if (record.pull_request !== undefined) out.pull_request = record.pull_request;
  }
  return out;
}

const HEADER = `# Trusted implementation ledger (schema 2). Every DAG node is predeclared
# exactly once under [implementation]. status = "pending" carries no evidence;
# transitioning the node's single line to status = "implemented" (with
# commit_message, commit_date, and optionally pull_request) is the complete
# implementation fact. Evidence fields are loose human locators; tooling never
# resolves or validates them against Git, content, ancestry, or GitHub.
# Serialization is canonical: nodes sorted by id, one line per node, so
# independent transitions merge mechanically. [[github_issue]] rows map
# node_id to gh_issue; sync_to_github is one-way-refresh policy only and never
# affects readiness. [[github_train_issue]] rows map train parents.`;

/** Serialize the canonical, deterministic ledger text. */
export function serializeLedger(parsed) {
  const lines = [`schema = ${LEDGER_SCHEMA_VERSION}`, "", HEADER, "", "[implementation]", ""];
  const ids = Object.keys(parsed.implementation || {}).sort();
  for (const nodeId of ids) {
    const record = canonicalRecord(parsed.implementation[nodeId]);
    lines.push(`"${nodeId}" = ${serializeInlineTable(record)}`);
  }
  const issues = [...(parsed.github_issue || [])].sort((a, b) =>
    a.node_id < b.node_id ? -1 : a.node_id > b.node_id ? 1 : 0,
  );
  for (const row of issues) {
    lines.push(
      "",
      "[[github_issue]]",
      `node_id = ${JSON.stringify(row.node_id)}`,
      `gh_issue = ${row.gh_issue}`,
      `sync_to_github = ${row.sync_to_github === true}`,
    );
  }
  const trains = [...(parsed.github_train_issue || [])].sort((a, b) =>
    a.train < b.train ? -1 : a.train > b.train ? 1 : 0,
  );
  for (const row of trains) {
    lines.push("", "[[github_train_issue]]", `train = ${JSON.stringify(row.train)}`, `gh_issue = ${row.gh_issue}`);
  }
  return `${lines.join("\n")}\n`;
}

function cloneParsed(parsed) {
  return structuredClone(parsed);
}

/**
 * Transition a pending node to implemented. Idempotent for identical
 * evidence; fails closed on unknown nodes and on conflicting evidence.
 * Returns a new document model.
 */
export function transitionToImplemented(parsed, nodeId, { commitMessage, commitDate, pullRequest }) {
  const next = cloneParsed(parsed);
  const record = next.implementation?.[nodeId];
  if (!record) throw new Error(`implementation ledger: unknown node ${nodeId}`);
  const replacement = {
    status: STATUS_IMPLEMENTED,
    commit_message: commitMessage,
    commit_date: commitDate,
    ...(pullRequest === undefined ? {} : { pull_request: pullRequest }),
  };
  const problems = recordErrors(nodeId, replacement);
  if (problems.length) throw new Error(problems.join("; "));
  if (record.status === STATUS_IMPLEMENTED) {
    const same =
      record.commit_message === replacement.commit_message &&
      record.commit_date === replacement.commit_date &&
      (record.pull_request ?? null) === (replacement.pull_request ?? null);
    if (!same)
      throw new Error(
        `implementation ledger: ${nodeId} is already implemented with different evidence`,
      );
  }
  next.implementation[nodeId] = replacement;
  return next;
}

/** Update evidence fields on an already-implemented node (finalization). */
export function setEvidence(parsed, nodeId, { commitMessage, commitDate, pullRequest }) {
  const next = cloneParsed(parsed);
  const record = next.implementation?.[nodeId];
  if (!record) throw new Error(`implementation ledger: unknown node ${nodeId}`);
  if (record.status !== STATUS_IMPLEMENTED)
    throw new Error(`implementation ledger: ${nodeId} is not implemented`);
  const replacement = {
    status: STATUS_IMPLEMENTED,
    commit_message: commitMessage ?? record.commit_message,
    commit_date: commitDate ?? record.commit_date,
  };
  const pr = pullRequest === undefined ? record.pull_request : pullRequest;
  if (pr !== undefined) replacement.pull_request = pr;
  const problems = recordErrors(nodeId, replacement);
  if (problems.length) throw new Error(problems.join("; "));
  next.implementation[nodeId] = replacement;
  return next;
}

/** Deliberately mark a node unimplemented again (operator correction). */
export function markPending(parsed, nodeId) {
  const next = cloneParsed(parsed);
  if (!next.implementation?.[nodeId])
    throw new Error(`implementation ledger: unknown node ${nodeId}`);
  next.implementation[nodeId] = { status: STATUS_PENDING };
  return next;
}

/** Predeclare any known nodes missing from the table (as pending). */
export function predeclareMissing(parsed, knownNodeIds) {
  const next = cloneParsed(parsed);
  next.implementation ||= {};
  for (const nodeId of knownNodeIds)
    if (!Object.hasOwn(next.implementation, nodeId))
      next.implementation[nodeId] = { status: STATUS_PENDING };
  return next;
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (isPlainObject(value)) {
    const keys = Object.keys(value).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function threeWayMap({ base, ours, theirs, describe }) {
  const conflicts = [];
  const merged = new Map();
  const keys = new Set([...base.keys(), ...ours.keys(), ...theirs.keys()]);
  for (const key of [...keys].sort()) {
    const b = base.has(key) ? stableStringify(base.get(key)) : undefined;
    const o = ours.has(key) ? stableStringify(ours.get(key)) : undefined;
    const t = theirs.has(key) ? stableStringify(theirs.get(key)) : undefined;
    let winnerMap;
    if (o === t) winnerMap = ours;
    else if (o === b) winnerMap = theirs;
    else if (t === b) winnerMap = ours;
    else {
      conflicts.push(`${describe} ${key}: both sides changed incompatibly`);
      continue;
    }
    // A key absent from the winning side was deleted there — leave it out.
    if (winnerMap.has(key)) merged.set(key, winnerMap.get(key));
  }
  return { conflicts, merged };
}

/**
 * Deterministic semantic 3-way merge of ledger texts. Independent node
 * transitions merge mechanically ("latest main + candidate transition");
 * incompatible changes to the same node fail closed. Never guesses.
 *
 * Returns { ok: true, text } or { ok: false, conflicts: string[] }.
 */
export function mergeLedgerTexts({ base, ours, theirs }) {
  let parsedBase;
  let parsedOurs;
  let parsedTheirs;
  try {
    parsedBase = parseLedgerText(base);
    parsedOurs = parseLedgerText(ours);
    parsedTheirs = parseLedgerText(theirs);
  } catch (error) {
    return { ok: false, conflicts: [`unparseable ledger: ${error.message}`] };
  }
  const conflicts = [];

  const impl = threeWayMap({
    base: new Map(Object.entries(parsedBase.implementation)),
    ours: new Map(Object.entries(parsedOurs.implementation)),
    theirs: new Map(Object.entries(parsedTheirs.implementation)),
    describe: "node",
  });
  conflicts.push(...impl.conflicts);

  const issueMapOf = (parsed) => new Map((parsed.github_issue || []).map((row) => [row.node_id, row]));
  const issues = threeWayMap({
    base: issueMapOf(parsedBase),
    ours: issueMapOf(parsedOurs),
    theirs: issueMapOf(parsedTheirs),
    describe: "github_issue",
  });
  conflicts.push(...issues.conflicts);

  const trainMapOf = (parsed) => new Map((parsed.github_train_issue || []).map((row) => [row.train, row]));
  const trains = threeWayMap({
    base: trainMapOf(parsedBase),
    ours: trainMapOf(parsedOurs),
    theirs: trainMapOf(parsedTheirs),
    describe: "github_train_issue",
  });
  conflicts.push(...trains.conflicts);

  if (conflicts.length) return { ok: false, conflicts };

  const mergedDoc = {
    schema: LEDGER_SCHEMA_VERSION,
    implementation: Object.fromEntries(impl.merged),
    github_issue: [...issues.merged.values()],
    github_train_issue: [...trains.merged.values()],
  };
  // Re-validate uniqueness of gh_issue numbers across both mapping tables.
  const seenIssues = new Set();
  for (const row of [...mergedDoc.github_issue, ...mergedDoc.github_train_issue]) {
    if (seenIssues.has(row.gh_issue))
      return { ok: false, conflicts: [`duplicate gh_issue ${row.gh_issue} after merge`] };
    seenIssues.add(row.gh_issue);
  }
  const structural = ledgerErrors(mergedDoc);
  if (structural.length) return { ok: false, conflicts: structural };
  return { ok: true, text: serializeLedger(mergedDoc) };
}
