/**
 * @ai-generated - Honest-operator, trusted-local lifecycle coordination.
 *
 * This module provides local consistency and an append-only audit trail. It does
 * not authenticate the operator, the harness, or the filesystem owner and does
 * not claim rollback resistance after loss or intentional replacement of local
 * state.
 */
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const EFFORTS = ["low", "medium", "high"];
const ROLES = ["implementation", "review", "verification", "confirmation"];
const POLICY_VERSION = "trusted-local-effort/v1";
const ANCHOR_SCHEMA = 1;
export const ARCHITECT_MANDATE = 'best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).';

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function exactJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function assertObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
}

function assertIdentity(value, label) {
  if (typeof value !== "string" || !/^[A-Za-z0-9/][A-Za-z0-9._:@/+\-]{0,255}$/.test(value)) throw new Error(`${label} is invalid`);
}

function assertHex(value, size, label) {
  if (typeof value !== "string" || !new RegExp(`^[0-9a-f]{${size}}$`).test(value)) throw new Error(`${label} must be ${size} lowercase hexadecimal characters`);
}

function effort(value, label) {
  if (!EFFORTS.includes(value)) throw new Error(`${label} must be low, medium, or high`);
  return value;
}

function maxEffort(...values) {
  return EFFORTS[Math.max(...values.map((value) => EFFORTS.indexOf(effort(value, "effort"))))];
}

function nodeSignalTier(node) {
  const highKinds = new Set(["governance", "activation", "cutover", "architecture", "convergence", "release"]);
  const mediumKinds = new Set(["semantic", "implementation", "migration"]);
  const rules = [];
  let tier = "low";
  if (highKinds.has(node.kind)) { tier = "high"; rules.push(`kind:${node.kind}:high`); }
  if (["critical", "high"].includes(node.risk)) { tier = "high"; rules.push(`risk:${node.risk}:high`); }
  if (node.public_api === true) { tier = "high"; rules.push("surface:public:high"); }
  if (node.semantic_authority === true) { tier = "high"; rules.push("authority:semantic:high"); }
  if (node.concurrency_sensitive === true) { tier = "high"; rules.push("concurrency:sensitive:high"); }
  if (tier !== "high" && (mediumKinds.has(node.kind) || node.risk === "medium")) {
    tier = "medium";
    rules.push(node.risk === "medium" ? "risk:medium:medium" : `kind:${node.kind}:medium`);
  }
  return { tier, rules: rules.sort() };
}

export function reviewPolicyForNode(node) {
  assertObject(node, "node");
  const riskBand = nodeSignalTier(node).tier;
  const defaults = riskBand === "high"
    ? ["adversarial", "conformance", node.specialist_review_lens || "context-specific"]
    : riskBand === "medium" ? ["adversarial", "conformance"] : ["adversarial"];
  const reviewLenses = node.review_lenses ? [...node.review_lenses] : defaults;
  if (new Set(reviewLenses).size !== reviewLenses.length || reviewLenses.some((lens) => typeof lens !== "string" || !lens)) throw new Error("review lenses must be distinct non-empty strings");
  if (reviewLenses[0] !== "adversarial") throw new Error("every review policy must lead with the adversarial lens");
  if (riskBand === "high" && (reviewLenses.length !== 3 || reviewLenses[1] !== "conformance")) throw new Error("high-risk review requires adversarial, conformance, and one specialist lens");
  if (riskBand === "medium" && (reviewLenses.length < 1 || reviewLenses.length > 2)) throw new Error("medium-risk review requires one or two lenses");
  if (riskBand === "low" && reviewLenses.length !== 1) throw new Error("low-risk review requires one adversarial lens");
  return {
    risk_band: riskBand,
    review_lenses: reviewLenses,
    confirmation: riskBand === "high" ? "independent-full" : riskBand === "medium" ? "targeted" : "not-required",
  };
}

export function architectPromptFor({ type, nodeId, roundId, roundOrdinal, question = "" }) {
  const questions = {
    "pre-block": "What ruling, if any, is needed before this block can proceed?",
    "round-two-cap": "Should review/fix work continue, and if so what exact additional-round cap applies?",
    "over-five-decomposition": "Should we break this work into smaller independently reviewable sub-subblocks?",
    "architecture-ruling-change": "What durable architecture ruling should govern this proposed change?",
    "conformance-deviation": "Is this potentially beneficial design deviation compatible with the grand design, or should another non-listed course be taken?",
    "landing-confirmation": "Does the immutable target satisfy the architecture required for landing or confirmation?",
  };
  if (!Object.hasOwn(questions, type)) throw new Error(`unknown Architect prompt type ${type}`);
  return {
    schema: 1,
    type: `trusted-local-architect-${type}-prompt`,
    node_id: nodeId,
    round_id: roundId,
    round_ordinal: roundOrdinal,
    provider: "openai",
    tool: "codex",
    model: "gpt-5.6-sol",
    reasoning_effort: "xhigh",
    neutrality: "evaluate the verified evidence without presuming the listed choices exhaust the sound options",
    options_non_exhaustive: true,
    mandate: ARCHITECT_MANDATE,
    question: question || questions[type],
  };
}

export function assessNodeEffort(node, overrides = {}) {
  assertObject(node, "node");
  const signal = nodeSignalTier(node);
  const result = {};
  for (const role of ROLES) {
    const minimum = effort(node[`${role}_effort_min`], `${role} effort minimum`);
    const configuredDefault = effort(node[`${role}_effort_default`] || minimum, `${role} effort default`);
    if (EFFORTS.indexOf(configuredDefault) < EFFORTS.indexOf(minimum)) throw new Error(`${role} effort default is lower than its minimum`);
    const automatic = role === "implementation" ? signal.tier : maxEffort(signal.tier, minimum);
    const selected = maxEffort(minimum, configuredDefault, automatic);
    if (overrides[role] !== undefined && EFFORTS.indexOf(effort(overrides[role], `${role} effort override`)) < EFFORTS.indexOf(selected)) {
      throw new Error(`${role} effort override cannot lower the deterministic floor ${selected}`);
    }
    result[role] = overrides[role] || selected;
  }
  return result;
}

export function effortPolicyFor(node, overrides = {}) {
  const signal = nodeSignalTier(node);
  const effective = assessNodeEffort(node, overrides);
  const minima = {}; const defaults = {};
  for (const role of ROLES) {
    minima[role] = node[`${role}_effort_min`];
    defaults[role] = node[`${role}_effort_default`] || minima[role];
  }
  return { version: POLICY_VERSION, minima, defaults, matched_rules: signal.rules, overrides: { ...overrides }, effective };
}

function safeRelative(value, label) {
  if (typeof value !== "string" || !value || path.isAbsolute(value) || value.includes("\\") || value.split("/").some((part) => !part || part === "." || part === "..")) throw new Error(`${label} is unsafe`);
  return value;
}

function atomicReplace(file, bytes) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temporary = `${file}.tmp-${process.pid}-${crypto.randomBytes(6).toString("hex")}`;
  fs.writeFileSync(temporary, bytes, { flag: "wx", mode: 0o600 });
  fs.renameSync(temporary, file);
}

function atomicCreate(file, bytes) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, bytes, { flag: "wx", mode: 0o600 });
}

function snapshotDirectory(root) {
  if (!fs.existsSync(root)) return [];
  const result = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(file);
      else if (entry.isFile()) result.push([path.relative(root, file), sha256(fs.readFileSync(file))]);
      else throw new Error(`runtime contains unsupported filesystem entry ${file}`);
    }
  };
  walk(root);
  return result;
}

function initialAnchor({ generation = 1, continuity = "known", priorLineage = "", preactivationHistory = null } = {}) {
  const anchor = {
    schema: ANCHOR_SCHEMA,
    assurance: "operator-attested-local-consistency",
    lineage_id: crypto.randomUUID(),
    lineage_generation: generation,
    continuity,
    prior_lineage_id: priorLineage,
    next_event_ordinal: 1,
    next_round_by_node: {},
    runtimes: {},
    leases: {},
    rounds: {},
    reviews: {},
  };
  if (preactivationHistory) {
    if (preactivationHistory.type !== "trusted-local-preactivation-history" || preactivationHistory.acceptance_eligible !== false || preactivationHistory.disposition !== "REJECTED_AUDIT_ONLY" || !Number.isSafeInteger(preactivationHistory.minimum_successor_round_ordinal)) throw new Error("preactivation history seed is invalid");
    anchor.next_round_by_node[preactivationHistory.node_id] = preactivationHistory.minimum_successor_round_ordinal;
    anchor.rounds[preactivationHistory.round_id] = { node_id: preactivationHistory.node_id, ordinal: preactivationHistory.minimum_successor_round_ordinal - 1, lease_id: preactivationHistory.lease_id, status: "REJECTED_AUDIT_ONLY", preactivation: true };
  }
  return anchor;
}

function validateAnchor(anchor) {
  assertObject(anchor, "local lifecycle anchor");
  if (anchor.schema !== ANCHOR_SCHEMA || anchor.assurance !== "operator-attested-local-consistency") throw new Error("local lifecycle anchor schema/assurance mismatch");
  assertIdentity(anchor.lineage_id, "lineage id");
  if (!Number.isSafeInteger(anchor.lineage_generation) || anchor.lineage_generation < 1) throw new Error("lineage generation is invalid");
  if (!["known", "unknown/lost"].includes(anchor.continuity)) throw new Error("lineage continuity is invalid");
  for (const key of ["next_round_by_node", "runtimes", "leases", "rounds", "reviews"]) assertObject(anchor[key], `anchor ${key}`);
  return anchor;
}

function readAnchorFile(anchorPath) {
  return validateAnchor(JSON.parse(fs.readFileSync(anchorPath, "utf8")));
}

function breadcrumbPath(runtimeRoot) {
  return path.join(runtimeRoot, ".trusted-local-lineage.json");
}

function hasRuntimeHistory(runtimeRoot) {
  return fs.existsSync(runtimeRoot) && snapshotDirectory(runtimeRoot).length > 0;
}

function acquireLock(lockPath) {
  fs.mkdirSync(path.dirname(lockPath), { recursive: true });
  try { fs.mkdirSync(lockPath); }
  catch (error) {
    if (error.code === "EEXIST") throw new Error("repo-global lifecycle lock is already held");
    throw error;
  }
  return () => fs.rmSync(lockPath, { recursive: true, force: true });
}

function transactionPath(controlRoot) {
  return path.join(controlRoot, "transaction.json");
}

function applyTransaction(controlRoot, transaction) {
  for (let index = 0; index < transaction.writes.length; index += 1) {
    const write = transaction.writes[index];
    const file = path.resolve(write.path);
    const expected = Buffer.from(write.bytes_base64, "base64");
    if (fs.existsSync(file)) {
      if (sha256(fs.readFileSync(file)) !== write.sha256) {
        if (write.mode === "replace") atomicReplace(file, expected);
        else throw new Error(`transaction destination differs from committed marker: ${file}`);
      }
    } else atomicCreate(file, expected);
    if (process.env.VERTER_TRUSTED_LOCAL_FAILPOINT === `after-write-${index + 1}`) throw new Error(`trusted-local failpoint after-write-${index + 1}`);
  }
  if (process.env.VERTER_TRUSTED_LOCAL_FAILPOINT === "before-anchor") throw new Error("trusted-local failpoint before-anchor");
  atomicReplace(path.join(controlRoot, "anchor.json"), Buffer.from(transaction.anchor_bytes_base64, "base64"));
  if (process.env.VERTER_TRUSTED_LOCAL_FAILPOINT === "after-anchor") throw new Error("trusted-local failpoint after-anchor");
  atomicReplace(path.join(controlRoot, "lineage-sentinel.json"), Buffer.from(exactJson({
    schema: 1,
    last_lineage_id: transaction.next_anchor.lineage_id,
    last_generation: transaction.next_anchor.lineage_generation,
  })));
  fs.unlinkSync(transactionPath(controlRoot));
}

function recoverTransaction(controlRoot) {
  const file = transactionPath(controlRoot);
  if (!fs.existsSync(file)) return;
  const transaction = JSON.parse(fs.readFileSync(file, "utf8"));
  if (transaction.schema !== 1 || transaction.type !== "trusted-local-transaction" || !Array.isArray(transaction.writes)) throw new Error("local transaction marker is invalid; operator recovery is required");
  const anchorBytes = Buffer.from(transaction.anchor_bytes_base64, "base64");
  if (sha256(anchorBytes) !== transaction.anchor_sha256) throw new Error("local transaction marker anchor digest mismatch");
  for (const write of transaction.writes) if (sha256(Buffer.from(write.bytes_base64, "base64")) !== write.sha256) throw new Error("local transaction marker write digest mismatch");
  applyTransaction(controlRoot, transaction);
}

function commit(controlRoot, priorAnchor, nextAnchor, writes) {
  const anchorBytes = Buffer.from(exactJson(nextAnchor));
  const transaction = {
    schema: 1,
    type: "trusted-local-transaction",
    operation_id: `LOCAL-${nextAnchor.next_event_ordinal - 1}-${crypto.randomBytes(8).toString("hex")}`,
    prior_anchor_sha256: sha256(Buffer.from(exactJson(priorAnchor))),
    anchor_sha256: sha256(anchorBytes),
    anchor_bytes_base64: anchorBytes.toString("base64"),
    next_anchor: nextAnchor,
    writes: writes.map(({ file, bytes, mode = "create" }) => ({ path: file, mode, sha256: sha256(bytes), bytes_base64: bytes.toString("base64") })),
  };
  atomicCreate(transactionPath(controlRoot), Buffer.from(exactJson(transaction)));
  if (process.env.VERTER_TRUSTED_LOCAL_FAILPOINT === "after-marker") throw new Error("trusted-local failpoint after-marker");
  applyTransaction(controlRoot, transaction);
}

function validateCandidate(candidate) {
  assertObject(candidate, "candidate");
  assertHex(candidate.sha, 40, "candidate sha");
  assertHex(candidate.tree, 40, "candidate tree");
  if (typeof candidate.ref !== "string" || !candidate.ref.startsWith("refs/heads/")) throw new Error("candidate ref must be a branch ref");
  if (typeof candidate.worktree !== "string" || !path.isAbsolute(candidate.worktree)) throw new Error("candidate worktree must be absolute");
}

function validateNode(node) {
  assertObject(node, "node");
  assertIdentity(node.id, "node id");
  if (!Array.isArray(node.conflict_domains) || !node.conflict_domains.length) throw new Error("node conflict domains are required");
  for (const domain of node.conflict_domains) assertIdentity(domain, "conflict domain");
  assessNodeEffort(node);
  reviewPolicyForNode(node);
}

function packetForLease(lease) {
  return exactJson({
    schema: 1,
    type: "trusted-local-work-packet",
    assurance: "operator-attested-local-consistency",
    lease_id: lease.lease_id,
    round_id: lease.round_id,
    node_id: lease.node_id,
    holder: lease.holder,
    candidate: lease.candidate,
    effort_policy: lease.effort_policy,
    review_policy: lease.review_policy,
    task_briefs: lease.task_briefs,
  });
}

function validatePacket(packetBytes, lease) {
  const packet = JSON.parse(packetBytes);
  if (packet.schema !== 1 || packet.type !== "trusted-local-work-packet" || packet.assurance !== "operator-attested-local-consistency") throw new Error("generated work packet is invalid");
  for (const key of ["lease_id", "round_id", "node_id", "holder"]) if (packet[key] !== lease[key]) throw new Error(`generated work packet ${key} mismatch`);
  if (JSON.stringify(packet.candidate) !== JSON.stringify(lease.candidate) || JSON.stringify(packet.effort_policy) !== JSON.stringify(lease.effort_policy) || JSON.stringify(packet.review_policy) !== JSON.stringify(lease.review_policy) || JSON.stringify(packet.task_briefs) !== JSON.stringify(lease.task_briefs)) throw new Error("generated work packet binding mismatch");
}

function brief(role, lease) {
  return exactJson({
    schema: 1,
    type: "trusted-local-task-brief",
    assurance: "operator-attested-separate-harness-task",
    role,
    task_identity: `${lease.round_id}/${role}`,
    lease_id: lease.lease_id,
    round_id: lease.round_id,
    node_id: lease.node_id,
    candidate: lease.candidate,
    provider: "provider-neutral",
    minimum_effort: lease.effort_policy.minima[role],
    effective_effort: lease.effort_policy.effective[role],
    freshness: "fresh-distinct-task-required",
    review_policy: lease.review_policy,
    worktree_policy: role === "review"
      ? "read-only inspection of the frozen train worktree; a write-enabled reviewer must use a disposable worktree from the frozen SHA"
      : "write only in the task's assigned worktree and report cleanup when it becomes disposable",
    external_status: "terse-honest-summary",
  });
}

function buildLease({ node, candidate, holder, roundOrdinal, reviewCycleOrdinal, runtimeRoot, overrides, lineageGeneration, continuity }) {
  const roundId = continuity === "unknown/lost" ? `${node.id}-L${lineageGeneration}-R${roundOrdinal}` : `${node.id}-R${roundOrdinal}`;
  const leaseId = `${node.id}-${Date.now()}-${crypto.randomBytes(8).toString("hex")}`;
  const effortPolicy = effortPolicyFor(node, overrides);
  const reviewPolicy = reviewPolicyForNode(node);
  const taskBriefs = Object.fromEntries(ROLES.map((role) => [role, `trusted-local/briefs/${roundId}--${role}.json`]));
  return {
    schema: 1,
    type: "trusted-local-lease",
    assurance: "operator-attested-local-consistency",
    lease_id: leaseId,
    round_id: roundId,
    round_ordinal: roundOrdinal,
    review_cycle_ordinal: reviewCycleOrdinal,
    node_id: node.id,
    conflict_domains: [...node.conflict_domains].sort(),
    holder,
    runtime_root: path.resolve(runtimeRoot),
    candidate: { ...candidate },
    effort_policy: effortPolicy,
    review_policy: reviewPolicy,
    review_lenses: reviewPolicy.review_lenses,
    task_briefs: taskBriefs,
    status: "ACTIVE",
    renewed_from: "",
  };
}

export function readLocalAnchor({ controlRoot }) {
  return readAnchorFile(path.join(path.resolve(controlRoot), "anchor.json"));
}

export function reinitializeLocalLifecycle({ controlRoot, operator, reason }) {
  assertIdentity(operator, "operator");
  if (typeof reason !== "string" || reason.trim().length < 8) throw new Error("reinitialization reason is required");
  const root = path.resolve(controlRoot);
  const lockPath = path.join(root, "lifecycle.lock");
  const release = acquireLock(lockPath);
  try {
    recoverTransaction(root);
    const sentinelFile = path.join(root, "lineage-sentinel.json");
    const sentinel = fs.existsSync(sentinelFile) ? JSON.parse(fs.readFileSync(sentinelFile, "utf8")) : { last_generation: 0, last_lineage_id: "" };
    const anchor = initialAnchor({ generation: sentinel.last_generation + 1, continuity: "unknown/lost", priorLineage: sentinel.last_lineage_id || "" });
    anchor.reinitialized_by = operator;
    anchor.reinitialization_reason = reason;
    anchor.reinitialized_at = new Date().toISOString();
    fs.mkdirSync(root, { recursive: true });
    atomicReplace(path.join(root, "anchor.json"), Buffer.from(exactJson(anchor)));
    atomicReplace(sentinelFile, Buffer.from(exactJson({ schema: 1, last_lineage_id: anchor.lineage_id, last_generation: anchor.lineage_generation })));
    return anchor;
  } finally { release(); }
}

export function createLocalLifecycle({ controlRoot, preactivationHistory = null }) {
  const root = path.resolve(controlRoot);
  const anchorPath = path.join(root, "anchor.json");
  const lockPath = path.join(root, "lifecycle.lock");

  function ensureAnchor(runtimeRoot) {
    recoverTransaction(root);
    if (fs.existsSync(anchorPath)) return readAnchorFile(anchorPath);
    if (fs.existsSync(path.join(root, "lineage-sentinel.json")) || (runtimeRoot && fs.existsSync(breadcrumbPath(runtimeRoot)))) throw new Error("trusted-local anchor was lost; explicit operator reinitialization is required");
    const anchor = initialAnchor({ preactivationHistory });
    fs.mkdirSync(root, { recursive: true });
    atomicReplace(anchorPath, Buffer.from(exactJson(anchor)));
    atomicReplace(path.join(root, "lineage-sentinel.json"), Buffer.from(exactJson({ schema: 1, last_lineage_id: anchor.lineage_id, last_generation: anchor.lineage_generation })));
    return anchor;
  }

  function lockedMutation(runtimeRoot, preflight, plan) {
    preflight();
    const release = acquireLock(lockPath);
    try {
      const anchor = ensureAnchor(runtimeRoot);
      const snapshots = Object.fromEntries(Object.keys(anchor.runtimes).map((known) => [known, snapshotDirectory(known)]));
      const planned = plan(structuredClone(anchor));
      for (const [known, snapshot] of Object.entries(snapshots)) if (JSON.stringify(snapshotDirectory(known)) !== JSON.stringify(snapshot)) throw new Error(`registered runtime changed during locked recomputation: ${known}`);
      commit(root, anchor, planned.anchor, planned.writes);
      return planned.result;
    } finally { release(); }
  }

  function admit({ runtimeRoot, node, candidate, holder, effortOverrides = {} }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    const validate = () => { validateNode(node); validateCandidate(candidate); assertIdentity(holder, "lease holder"); assessNodeEffort(node, effortOverrides); };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      const priorRounds = Object.values(anchor.rounds).filter((round) => round.node_id === node.id);
      const unresolvedEscalation = priorRounds.find((round) => round.architect_escalation_required && !round.architect_decision);
      if (unresolvedEscalation) throw new Error(`${node.id} requires the neutral Architect decision after round-two P0/P1 findings`);
      const architectLimit = priorRounds.filter((round) => round.architect_decision).sort((a, b) => (b.review_cycle_ordinal || 0) - (a.review_cycle_ordinal || 0))[0];
      if (architectLimit?.architect_decision.decision === "STOP") throw new Error(`${node.id} was stopped by the neutral Architect`);
      const completedReviewCycles = priorRounds.filter((round) => round.review_recorded === true).length;
      if (architectLimit && completedReviewCycles >= architectLimit.review_cycle_ordinal + architectLimit.architect_decision.additional_round_cap) throw new Error(`${node.id} exceeded the Architect-authorized additional-round cap`);
      for (const lease of Object.values(anchor.leases)) if (lease.status === "ACTIVE") {
        if (lease.node_id === node.id) throw new Error(`active node ${node.id} already has a lease`);
        if (lease.conflict_domains.some((domain) => node.conflict_domains.includes(domain))) throw new Error(`conflict domain already held: ${lease.conflict_domains.filter((domain) => node.conflict_domains.includes(domain)).join(",")}`);
      }
      const ordinal = anchor.next_round_by_node[node.id] || 1;
      const reviewCycleOrdinal = completedReviewCycles + 1;
      const lease = buildLease({ node, candidate, holder, roundOrdinal: ordinal, reviewCycleOrdinal, runtimeRoot: resolvedRuntime, overrides: effortOverrides, lineageGeneration: anchor.lineage_generation, continuity: anchor.continuity });
      const packet = Buffer.from(packetForLease(lease));
      validatePacket(packet, lease); // Packet validation precedes publication of lease or anchor.
      const writes = [];
      const leaseFile = path.join(resolvedRuntime, "trusted-local", "leases", `${lease.lease_id}.json`);
      writes.push({ file: leaseFile, bytes: Buffer.from(exactJson(lease)) });
      writes.push({ file: path.join(resolvedRuntime, "trusted-local", "packets", `${lease.round_id}.json`), bytes: packet });
      for (const role of ROLES) writes.push({ file: path.join(resolvedRuntime, lease.task_briefs[role]), bytes: Buffer.from(brief(role, lease)) });
      writes.push({ file: breadcrumbPath(resolvedRuntime), mode: "replace", bytes: Buffer.from(exactJson({ schema: 1, lineage_id: anchor.lineage_id, lineage_generation: anchor.lineage_generation })) });
      anchor.runtimes[resolvedRuntime] = { lineage_id: anchor.lineage_id, registered_event: anchor.next_event_ordinal };
      anchor.leases[lease.lease_id] = lease;
      anchor.rounds[lease.round_id] = { node_id: node.id, ordinal, review_cycle_ordinal: reviewCycleOrdinal, lease_id: lease.lease_id, status: "ACTIVE" };
      anchor.next_round_by_node[node.id] = ordinal + 1;
      anchor.next_event_ordinal += 1;
      return { anchor, writes, result: lease };
    });
  }

  function close({ runtimeRoot, leaseId, holder, outcome }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    const validate = () => { assertIdentity(leaseId, "lease id"); assertIdentity(holder, "lease holder"); if (!["FIX_REQUIRED", "ABORTED", "RELEASED"].includes(outcome)) throw new Error("closure outcome is invalid"); };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      const lease = anchor.leases[leaseId];
      if (!lease || lease.runtime_root !== resolvedRuntime || lease.holder !== holder || lease.status !== "ACTIVE") throw new Error("closure requires the exact active lease and holder");
      lease.status = outcome;
      anchor.rounds[lease.round_id].status = outcome;
      anchor.next_event_ordinal += 1;
      const closure = { schema: 1, type: "trusted-local-round-closure", round_id: lease.round_id, lease_id: leaseId, node_id: lease.node_id, outcome, closed_by: holder };
      return { anchor, writes: [{ file: path.join(resolvedRuntime, "trusted-local", "round-closures", `${lease.round_id}.json`), bytes: Buffer.from(exactJson(closure)) }], result: closure };
    });
  }

  function renew({ runtimeRoot, leaseId, holder }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    return lockedMutation(resolvedRuntime, () => { assertIdentity(leaseId, "lease id"); assertIdentity(holder, "lease holder"); }, (anchor) => {
      const prior = anchor.leases[leaseId];
      if (!prior || prior.runtime_root !== resolvedRuntime || prior.holder !== holder || prior.status !== "ACTIVE") throw new Error("renewal requires the exact active lease and holder");
      prior.status = "RENEWED";
      const lease = { ...structuredClone(prior), lease_id: `${prior.lease_id}-renew-${anchor.next_event_ordinal}`, status: "ACTIVE", renewed_from: prior.lease_id };
      // Every component other than the renewal identity/status is inherited exactly.
      anchor.leases[lease.lease_id] = lease;
      anchor.rounds[lease.round_id].lease_id = lease.lease_id;
      anchor.next_event_ordinal += 1;
      return { anchor, writes: [{ file: path.join(resolvedRuntime, "trusted-local", "leases", `${lease.lease_id}.json`), bytes: Buffer.from(exactJson(lease)) }], result: lease };
    });
  }

  function finalize({ runtimeRoot, leaseId, holder, candidate }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    const validate = () => { assertIdentity(leaseId, "lease id"); assertIdentity(holder, "lease holder"); validateCandidate(candidate); };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      const lease = anchor.leases[leaseId]; const round = lease && anchor.rounds[lease.round_id];
      if (!lease || lease.runtime_root !== resolvedRuntime || lease.holder !== holder || lease.status !== "ACTIVE" || round?.status !== "ACTIVE" || round.lease_id !== leaseId) throw new Error("finalization requires the current exact active lease and holder");
      if (lease.finalization) throw new Error("candidate is already finalized");
      const reviewTarget = {
        schema: 1,
        type: "trusted-local-frozen-review-target",
        round_id: lease.round_id,
        lease_id: leaseId,
        node_id: lease.node_id,
        candidate_start_sha: lease.candidate.sha,
        candidate_start_tree: lease.candidate.tree,
        candidate_sha: candidate.sha,
        candidate_tree: candidate.tree,
        candidate_ref: candidate.ref,
        frozen_worktree: candidate.worktree,
        mutation_policy: "read-only-until-round-invalidated",
      };
      const reviewTargetSha256 = sha256(Buffer.from(exactJson(reviewTarget)));
      const finalization = { schema: 1, type: "trusted-local-finalization", assurance: "operator-attested-local-consistency", finalization_id: `${lease.round_id}--final`, round_id: lease.round_id, lease_id: leaseId, node_id: lease.node_id, holder, candidate: { ...candidate }, effort_policy: lease.effort_policy, review_target: reviewTarget, review_target_sha256: reviewTargetSha256 };
      lease.finalization = finalization;
      anchor.next_event_ordinal += 1;
      return { anchor, writes: [
        { file: path.join(resolvedRuntime, "trusted-local", "review-targets", `${lease.round_id}.json`), bytes: Buffer.from(exactJson(reviewTarget)) },
        { file: path.join(resolvedRuntime, "trusted-local", "finalizations", `${finalization.finalization_id}.json`), bytes: Buffer.from(exactJson(finalization)) },
      ], result: finalization };
    });
  }

  function accept({ runtimeRoot, roundId, holder }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    return lockedMutation(resolvedRuntime, () => { assertIdentity(roundId, "round id"); assertIdentity(holder, "accepting operator"); }, (anchor) => {
      const round = anchor.rounds[roundId];
      if (!round) throw new Error("acceptance round is unknown");
      const currentOrdinal = (anchor.next_round_by_node[round.node_id] || 1) - 1;
      if (round.ordinal !== currentOrdinal || round.status !== "ACTIVE") throw new Error("acceptance requires the current round");
      const lease = anchor.leases[round.lease_id];
      if (!lease || lease.runtime_root !== resolvedRuntime || lease.holder !== holder) throw new Error("acceptance requires the current exact lease and holder");
      if (!lease.finalization) throw new Error("acceptance requires exact candidate finalization");
      const reviews = Object.values(anchor.reviews).filter((review) => review.round_id === roundId && review.verdict === "PASS");
      if (reviews.length !== lease.review_lenses.length || new Set(reviews.map((review) => review.lens)).size !== lease.review_lenses.length || new Set(reviews.map((review) => review.task_identity)).size !== reviews.length) throw new Error("acceptance requires a clean current-round fresh review set");
      if (!lease.verification || lease.verification.verdict !== "PASS") throw new Error("acceptance requires a current-round verification PASS record");
      if (lease.review_policy.confirmation !== "not-required" && (!lease.confirmation || lease.confirmation.verdict !== "PASS")) throw new Error("acceptance requires the risk-scaled current-round confirmation PASS record");
      round.status = "ACCEPTED"; lease.status = "ACCEPTED"; anchor.next_event_ordinal += 1;
      const receipt = { schema: 1, type: "trusted-local-acceptance", assurance: "operator-attested-local-consistency", round_id: roundId, lease_id: lease.lease_id, node_id: round.node_id, accepted_by: holder };
      return { anchor, writes: [{ file: path.join(resolvedRuntime, "trusted-local", "acceptances", `${roundId}.json`), bytes: Buffer.from(exactJson(receipt)) }], result: receipt };
    });
  }

  function importBytes({ runtimeRoot, source, destination, validate }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    const relative = safeRelative(destination, "import destination");
    const sourceBytes = fs.readFileSync(path.resolve(source)); // exactly one source read
    const preflight = () => { if (typeof validate !== "function") throw new Error("import validator is required"); validate(sourceBytes); };
    const result = lockedMutation(resolvedRuntime, preflight, (anchor) => {
      validate(sourceBytes); // locked recomputation against the same immutable byte buffer
      const destinationFile = path.join(resolvedRuntime, relative);
      if (fs.existsSync(destinationFile)) throw new Error("import destination already exists");
      anchor.runtimes[resolvedRuntime] ||= { lineage_id: anchor.lineage_id, registered_event: anchor.next_event_ordinal };
      anchor.next_event_ordinal += 1;
      return { anchor, writes: [{ file: destinationFile, bytes: sourceBytes }], result: destinationFile };
    });
    return result;
  }

  function recordReview({ runtimeRoot, roundId, leaseId, holder, lens, taskIdentity, provider, model, effort: actualEffort, promptFile, reportFile }) {
    const resolvedRuntime = path.resolve(runtimeRoot);
    const promptBytes = fs.readFileSync(path.resolve(promptFile));
    const reportBytes = fs.readFileSync(path.resolve(reportFile));
    let report;
    const validate = () => {
      for (const [value, label] of [[roundId, "round id"], [leaseId, "lease id"], [holder, "holder"], [lens, "lens"], [taskIdentity, "task identity"], [provider, "provider"], [model, "model"]]) assertIdentity(value, label);
      effort(actualEffort, "actual review effort");
      report = JSON.parse(reportBytes);
      if (report.verdict !== "PASS" && report.verdict !== "FAIL") throw new Error("review report verdict must be PASS or FAIL");
      if (!Array.isArray(report.findings)) throw new Error("review report findings must be an array");
    };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      validate();
      const round = anchor.rounds[roundId]; const lease = anchor.leases[leaseId];
      if (!round || round.status !== "ACTIVE" || round.lease_id !== leaseId || !lease || lease.holder !== holder || lease.runtime_root !== resolvedRuntime) throw new Error("review requires the current exact active round and lease");
      if (!lease.review_lenses.includes(lens)) throw new Error("review lens is not assigned");
      if (!lease.finalization?.review_target || sha256(Buffer.from(exactJson(lease.finalization.review_target))) !== lease.finalization.review_target_sha256) throw new Error("review requires the exact frozen review target manifest");
      if (actualEffort !== lease.effort_policy.effective.review) throw new Error("review actual effort does not exactly match computed effort");
      if (Object.values(anchor.reviews).some((review) => review.round_id === roundId && (review.lens === lens || review.task_identity === taskIdentity))) throw new Error("review lens and task identity must be fresh and distinct");
      const evidenceId = `${roundId}-${lens}-${anchor.next_event_ordinal}`;
      const evidence = { schema: 1, type: "trusted-local-harness-review", assurance: "operator-attested-separate-harness-task", evidence_id: evidenceId, round_id: roundId, lease_id: leaseId, node_id: lease.node_id, lens, task_identity: taskIdentity, provider, model, effort: actualEffort, review_target: lease.finalization.review_target, review_target_sha256: lease.finalization.review_target_sha256, prompt_sha256: sha256(promptBytes), report_sha256: sha256(reportBytes), verdict: report.verdict, findings: report.findings };
      const writes = [
        { file: path.join(resolvedRuntime, "trusted-local", "review-prompts", `${evidenceId}.txt`), bytes: promptBytes },
        { file: path.join(resolvedRuntime, "trusted-local", "review-reports", `${evidenceId}.json`), bytes: reportBytes },
        { file: path.join(resolvedRuntime, "trusted-local", "reviews", `${evidenceId}.json`), bytes: Buffer.from(exactJson(evidence)) },
      ];
      round.review_recorded = true;
      if (report.verdict === "FAIL" && round.review_cycle_ordinal >= 2 && report.findings.some((finding) => ["P0", "P1"].includes(finding?.severity))) {
        round.architect_escalation_required = true;
        const promptType = round.review_cycle_ordinal > 5 ? "over-five-decomposition" : "round-two-cap";
        const architectPrompt = exactJson({ ...architectPromptFor({ type: promptType, nodeId: lease.node_id, roundId, roundOrdinal: round.review_cycle_ordinal }), reason: "P0/P1 remains after the soft two-cycle limit", requested_decision: "continue_or_stop_with_explicit_additional_round_cap" });
        writes.push({ file: path.join(resolvedRuntime, "trusted-local", "architect-prompts", `${roundId}.json`), bytes: Buffer.from(architectPrompt) });
      }
      anchor.reviews[evidenceId] = evidence; anchor.next_event_ordinal += 1;
      return { anchor, writes, result: evidence };
    });
  }

  function recordArchitectDecision({ runtimeRoot, roundId, operator, reportFile }) {
    const resolvedRuntime = path.resolve(runtimeRoot); const reportBytes = fs.readFileSync(path.resolve(reportFile)); let report;
    const validate = () => {
      assertIdentity(roundId, "round id"); assertIdentity(operator, "operator"); report = JSON.parse(reportBytes);
      if (report.provider !== "openai" || report.tool !== "codex" || report.model !== "gpt-5.6-sol" || report.reasoning_effort !== "xhigh" || !["CONTINUE", "STOP"].includes(report.decision) || !Number.isSafeInteger(report.additional_round_cap) || report.additional_round_cap < 0) throw new Error("Architect decision must exactly bind Codex gpt-5.6-sol xhigh and an explicit additional-round cap");
    };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      validate(); const round = anchor.rounds[roundId];
      if (!round?.architect_escalation_required || round.architect_decision) throw new Error("Architect decision is not required or is already recorded");
      if (round.review_cycle_ordinal > 5 && !["SPLIT", "CONTINUE_WHOLE", "OTHER"].includes(report.decomposition_decision)) throw new Error("Architect decision after five review cycles must explicitly decide decomposition");
      const decision = { schema: 1, type: "trusted-local-architect-decision", round_id: roundId, node_id: round.node_id, review_cycle_ordinal: round.review_cycle_ordinal, recorded_by: operator, report_sha256: sha256(reportBytes), ...report };
      round.architect_decision = decision; anchor.next_event_ordinal += 1;
      return { anchor, writes: [{ file: path.join(resolvedRuntime, "trusted-local", "architect-decisions", `${roundId}.json`), bytes: reportBytes }], result: decision };
    });
  }

  function recordRole({ runtimeRoot, roundId, leaseId, holder, role, taskIdentity, provider, model, effort: actualEffort, promptFile, reportFile }) {
    if (!["verification", "confirmation"].includes(role)) throw new Error("recorded role must be verification or confirmation");
    const resolvedRuntime = path.resolve(runtimeRoot);
    const promptBytes = fs.readFileSync(path.resolve(promptFile)); const reportBytes = fs.readFileSync(path.resolve(reportFile));
    let report;
    const validate = () => {
      for (const [value, label] of [[roundId, "round id"], [leaseId, "lease id"], [holder, "holder"], [taskIdentity, "task identity"], [provider, "provider"], [model, "model"]]) assertIdentity(value, label);
      effort(actualEffort, `actual ${role} effort`); report = JSON.parse(reportBytes);
      if (report.verdict !== "PASS" || !Array.isArray(report.findings) || report.findings.length) throw new Error(`${role} report must be a clean PASS`);
    };
    return lockedMutation(resolvedRuntime, validate, (anchor) => {
      validate(); const round = anchor.rounds[roundId]; const lease = anchor.leases[leaseId];
      if (!round || round.status !== "ACTIVE" || round.lease_id !== leaseId || !lease || lease.holder !== holder || lease.runtime_root !== resolvedRuntime || !lease.finalization) throw new Error(`${role} requires the current exact finalized round and lease`);
      if (actualEffort !== lease.effort_policy.effective[role]) throw new Error(`${role} actual effort does not exactly match computed effort`);
      if (lease[role]) throw new Error(`${role} is already recorded for this round`);
      const evidence = { schema: 1, type: `trusted-local-${role}`, assurance: "operator-attested-separate-harness-task", round_id: roundId, lease_id: leaseId, node_id: lease.node_id, task_identity: taskIdentity, provider, model, effort: actualEffort, review_target: lease.finalization.review_target, review_target_sha256: lease.finalization.review_target_sha256, confirmation_policy: lease.review_policy.confirmation, prompt_sha256: sha256(promptBytes), report_sha256: sha256(reportBytes), verdict: "PASS", findings: [] };
      lease[role] = evidence; anchor.next_event_ordinal += 1;
      return { anchor, writes: [
        { file: path.join(resolvedRuntime, "trusted-local", `${role}-prompts`, `${roundId}.txt`), bytes: promptBytes },
        { file: path.join(resolvedRuntime, "trusted-local", `${role}-reports`, `${roundId}.json`), bytes: reportBytes },
        { file: path.join(resolvedRuntime, "trusted-local", role, `${roundId}.json`), bytes: Buffer.from(exactJson(evidence)) },
      ], result: evidence };
    });
  }

  return { anchorPath, admit, close, renew, finalize, accept, importBytes, recordReview, recordRole, recordArchitectDecision };
}

export const TRUSTED_LOCAL_ASSURANCE = "operator-attested-local-consistency";
export const TRUSTED_LOCAL_EFFORT_POLICY = POLICY_VERSION;
