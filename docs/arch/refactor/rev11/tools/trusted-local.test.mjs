/**
 * @ai-generated - Discriminating tests for the trusted-local lifecycle control plane.
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  ARCHITECT_MANDATE,
  architectPromptFor,
  assessNodeEffort,
  createLocalLifecycle,
  readLocalAnchor,
  reinitializeLocalLifecycle,
  reviewPolicyForNode,
} from "./trusted-local.mjs";

function fixture(run, { preactivationHistory = null } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rev11-trusted-local-"));
  try {
    const controlRoot = path.join(root, "control");
    const runtimeA = path.join(root, "runtime-a");
    const runtimeB = path.join(root, "runtime-b");
    const lifecycle = createLocalLifecycle({ controlRoot, preactivationHistory });
    return run({ root, controlRoot, runtimeA, runtimeB, lifecycle });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

const candidate = {
  sha: "1".repeat(40),
  tree: "2".repeat(40),
  ref: "refs/heads/candidate",
  worktree: "/tmp/candidate",
};

const highNode = {
  id: "ORC0",
  kind: "governance",
  risk: "critical",
  conflict_domains: ["governance"],
  public_api: true,
  semantic_authority: true,
  implementation_effort_min: "high",
  review_effort_min: "high",
  verification_effort_min: "high",
  confirmation_effort_min: "high",
};

const finding = (severity = "P1", fingerprint = "a".repeat(64)) => ({ severity, fingerprint, owner: "trusted-local lifecycle", status: "OPEN" });

function harnessFiles(root, name, report) {
  const promptFile = path.join(root, `${name}.prompt`); const reportFile = path.join(root, `${name}.json`);
  fs.writeFileSync(promptFile, `${name} prompt\n`); fs.writeFileSync(reportFile, `${JSON.stringify(report)}\n`);
  return { promptFile, reportFile };
}

function recordHighReviewSet({ lifecycle, runtimeA, root, admitted, suffix, severity = "P1" }) {
  for (const [index, lens] of admitted.review_lenses.entries()) {
    const files = harnessFiles(root, `${suffix}-${lens}`, { verdict: "FAIL", findings: [finding(severity, `${index + 1}`.repeat(64))] });
    lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens, taskIdentity: `/root/${suffix}-${lens}`, agentIdentity: `${suffix}-${lens}-reviewer`, provider: "openai", model: "gpt-5.6-sol", effort: "high", ...files });
  }
}

test("automatic effort assessment preserves floors and permits only upward overrides", () => {
  assert.deepEqual(assessNodeEffort(highNode), {
    implementation: "high", review: "high", verification: "high", confirmation: "high",
  });
  const medium = { ...highNode, id: "DOC0", kind: "docs", risk: "low", public_api: false, semantic_authority: false,
    implementation_effort_min: "low", review_effort_min: "medium", verification_effort_min: "low", confirmation_effort_min: "medium" };
  assert.deepEqual(assessNodeEffort(medium), {
    implementation: "low", review: "medium", verification: "low", confirmation: "medium",
  });
  assert.throws(() => assessNodeEffort(highNode, { review: "medium" }), /lower|floor/i);
});

test("review and confirmation policy scales deterministically with risk and supports a specialist lens", () => {
  assert.deepEqual(reviewPolicyForNode({ ...highNode, specialist_review_lens: "wire-public" }), {
    risk_band: "high",
    review_lenses: ["adversarial", "conformance", "wire-public"],
    confirmation: "independent-full",
  });
  assert.deepEqual(reviewPolicyForNode({ ...highNode, kind: "semantic", risk: "medium", public_api: false, semantic_authority: false }), {
    risk_band: "medium",
    review_lenses: ["adversarial", "conformance"],
    confirmation: "targeted",
  });
  assert.deepEqual(reviewPolicyForNode({ ...highNode, kind: "docs", risk: "low", public_api: false, semantic_authority: false }), {
    risk_band: "low",
    review_lenses: ["adversarial"],
    confirmation: "not-required",
  });
});

test("Architect prompts are neutral, non-exhaustive, exact-profile, and carry the durable-design mandate", () => {
  const roundTwo = architectPromptFor({ type: "round-two-cap", nodeId: "ORC0", roundId: "ORC0-R2", roundOrdinal: 2 });
  assert.equal(roundTwo.provider, "openai");
  assert.equal(roundTwo.tool, "codex");
  assert.equal(roundTwo.model, "gpt-5.6-sol");
  assert.equal(roundTwo.reasoning_effort, "xhigh");
  assert.equal(roundTwo.mandate, ARCHITECT_MANDATE);
  assert.equal(roundTwo.options_non_exhaustive, true);
  const overFive = architectPromptFor({ type: "over-five-decomposition", nodeId: "ORC0", roundId: "ORC0-R6", roundOrdinal: 6 });
  assert.match(overFive.question, /break.*smaller independently reviewable sub-subblocks/i);
  assert.match(roundTwo.scope_guardrail, /ratified contract.*optional debt.*non-blocking/i);
});

test("PASS reports with findings and malformed FAIL dispositions refuse before evidence mutation", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
    const passWithFinding = harnessFiles(root, "bad-pass", { verdict: "PASS", findings: [finding()] });
    const before = fs.readFileSync(lifecycle.anchorPath);
    assert.throws(() => lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/bad-pass", agentIdentity: "reviewer-a", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...passWithFinding }), /clean PASS|findings/i);
    assert.deepEqual(fs.readFileSync(lifecycle.anchorPath), before);
    const malformed = harnessFiles(root, "bad-fail", { verdict: "FAIL", findings: [{ severity: "SEVERE", status: "CLOSED" }] });
    assert.throws(() => lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/bad-fail", agentIdentity: "reviewer-b", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...malformed }), /severity|fingerprint|OPEN/i);
    assert.deepEqual(fs.readFileSync(lifecycle.anchorPath), before);
  });
});

test("a failed review cycle completes only after every required lens and FIX_REQUIRED disposition", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
    const lenses = ["adversarial", "conformance", "context-specific"];
    for (let index = 0; index < lenses.length - 1; index += 1) {
      const files = harnessFiles(root, `partial-${index}`, { verdict: "FAIL", findings: [finding("P1", String(index + 1).repeat(64))] });
      lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: lenses[index], taskIdentity: `/root/partial-${index}`, agentIdentity: `reviewer-${index}`, provider: "openai", model: "gpt-5.6-sol", effort: "high", ...files });
    }
    assert.throws(() => lifecycle.close({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", outcome: "FIX_REQUIRED" }), /complete.*review.*profile|all required lenses/i);
    const finalFiles = harnessFiles(root, "partial-final", { verdict: "FAIL", findings: [finding("P1", "3".repeat(64))] });
    lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: lenses[2], taskIdentity: "/root/partial-final", agentIdentity: "reviewer-2", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...finalFiles });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    assert.equal(lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }).review_cycle_ordinal, 2);
  });
});

test("reviewer differs from author and task identities are fresh across roles and rounds", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
    const files = harnessFiles(root, "identity", { verdict: "PASS", findings: [] });
    assert.throws(() => lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/shared-task", agentIdentity: "author", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...files }), /author.*reviewer|distinct/i);
    lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/shared-task", agentIdentity: "reviewer-a", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...files });
    assert.throws(() => lifecycle.recordRole({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", role: "verification", taskIdentity: "/root/shared-task", agentIdentity: "verifier-a", provider: "openai", model: "gpt-5.6-sol", effort: "high", ...files }), /fresh.*task|already used/i);
  });
});

test("cycle six requires an over-five Architect decomposition ruling regardless of severity", () => {
  const lowNode = { ...highNode, id: "DOC0", kind: "docs", risk: "low", public_api: false, semantic_authority: false,
    implementation_effort_min: "low", review_effort_min: "low", verification_effort_min: "low", confirmation_effort_min: "low" };
  fixture(({ runtimeA, lifecycle, root }) => {
    for (let cycle = 1; cycle <= 5; cycle += 1) {
      const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: lowNode, candidate, holder: "author" });
      lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
      const files = harnessFiles(root, `cycle-${cycle}`, { verdict: "FAIL", findings: [finding("P3", String(cycle).repeat(64))] });
      lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: `/root/cycle-${cycle}`, agentIdentity: `reviewer-${cycle}`, provider: "openai", model: "gpt-5.6-sol", effort: "low", ...files });
      lifecycle.close({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    }
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: lowNode, candidate, holder: "author" }), /Architect.*decomposition|over-five/i);
  });
});

test("one repo-global anchor records independent admissions without acting as a work lease", () => {
  fixture(({ runtimeA, runtimeB, lifecycle }) => {
    const first = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author-a" });
    assert.equal(first.round_id, "ORC0-R1");
    const second = lifecycle.admit({ runtimeRoot: runtimeB, node: { ...highNode, id: "ORC0-ALIAS" }, candidate, holder: "author-b" });
    assert.equal(second.round_id, "ORC0-ALIAS-R1");
    const anchor = readLocalAnchor({ controlRoot: path.dirname(lifecycle.anchorPath) });
    assert.equal(Object.values(anchor.leases).filter((row) => row.status === "ACTIVE").length, 2);
  });
});

test("round ordinals are global and an old closed round cannot regain acceptance", () => {
  fixture(({ runtimeA, runtimeB, lifecycle }) => {
    const r1 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r1.lease_id, holder: "author", outcome: "ABORTED" });
    const r2 = lifecycle.admit({ runtimeRoot: runtimeB, node: highNode, candidate, holder: "author" });
    assert.equal(r2.round_id, "ORC0-R2");
    const before = fs.readFileSync(lifecycle.anchorPath);
    assert.throws(() => lifecycle.accept({ runtimeRoot: runtimeA, roundId: "ORC0-R1", holder: "author" }), /current round/i);
    assert.deepEqual(fs.readFileSync(lifecycle.anchorPath), before);
  });
});

test("anchor loss blocks mutation until explicit reinitialization exposes lost continuity", () => {
  fixture(({ controlRoot, runtimeA, lifecycle }) => {
    lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    fs.unlinkSync(lifecycle.anchorPath);
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }), /anchor.*lost|reinitial/i);
    const reset = reinitializeLocalLifecycle({ controlRoot, operator: "maintainer", reason: "local disk restored without control anchor" });
    assert.equal(reset.continuity, "unknown/lost");
    assert.equal(readLocalAnchor({ controlRoot }).lineage_generation, 2);
    const after = createLocalLifecycle({ controlRoot }).admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    assert.match(after.round_id, /^ORC0-L2-R1$/);
  });
});

test("single-read imports install the originally validated bytes when the source path is replaced", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const source = path.join(root, "artifact.json");
    const original = Buffer.from('{"schema":1,"type":"operator-attestation"}\n');
    fs.writeFileSync(source, original);
    const installed = lifecycle.importBytes({
      runtimeRoot: runtimeA,
      source,
      destination: "imports/artifact.json",
      validate(bytes) {
        assert.deepEqual(bytes, original);
        fs.writeFileSync(source, '{"schema":1,"type":"replaced"}\n');
      },
    });
    assert.deepEqual(fs.readFileSync(installed), original);
  });
});

test("a downward effort override refuses before creating runtime or local custody", () => {
  fixture(({ runtimeA, lifecycle }) => {
    assert.equal(fs.existsSync(lifecycle.anchorPath), false);
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author", effortOverrides: { review: "medium" } }), /cannot lower/i);
    assert.equal(fs.existsSync(lifecycle.anchorPath), false);
    assert.equal(fs.existsSync(runtimeA), false);
  });
});

test("transaction-boundary crashes recover exactly one admission", () => {
  for (const failpoint of ["after-marker", "after-write-1", "before-anchor", "after-anchor"]) fixture(({ runtimeA, runtimeB, lifecycle }) => {
    process.env.VERTER_TRUSTED_LOCAL_FAILPOINT = failpoint;
    try { assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }), /failpoint/); }
    finally { delete process.env.VERTER_TRUSTED_LOCAL_FAILPOINT; }
    const next = lifecycle.admit({ runtimeRoot: runtimeB, node: highNode, candidate, holder: "other" });
    assert.equal(next.round_id, "ORC0-R2");
    const anchor = readLocalAnchor({ controlRoot: path.dirname(lifecycle.anchorPath) });
    assert.equal(Object.values(anchor.leases).filter((lease) => lease.node_id === "ORC0").length, 2, failpoint);
    assert.equal(fs.existsSync(path.join(path.dirname(lifecycle.anchorPath), "transaction.json")), false, failpoint);
  });
});

test("second completed P1 review/fix cycle requires the fixed neutral Architect and an explicit additional-round cap", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const r1 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }); lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: r1.lease_id, holder: "author", candidate });
    recordHighReviewSet({ lifecycle, runtimeA, root, admitted: r1, suffix: "cap-r1" });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r1.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    const r2 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }); lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: r2.lease_id, holder: "author", candidate });
    recordHighReviewSet({ lifecycle, runtimeA, root, admitted: r2, suffix: "cap-r2" });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r2.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    const escalation = JSON.parse(fs.readFileSync(path.join(runtimeA, "trusted-local", "architect-prompts", `${r2.round_id}.json`), "utf8"));
    assert.equal(escalation.mandate, ARCHITECT_MANDATE);
    assert.equal(escalation.options_non_exhaustive, true);
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }), /Architect decision/);
    const decision = path.join(root, "architect.json"); fs.writeFileSync(decision, '{"provider":"openai","tool":"codex","model":"gpt-5.6-sol","reasoning_effort":"xhigh","decision":"CONTINUE","additional_round_cap":1}\n');
    lifecycle.recordArchitectDecision({ runtimeRoot: runtimeA, roundId: r2.round_id, operator: "maintainer", reportFile: decision });
    const r3 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    assert.equal(r3.round_id, "ORC0-R3"); lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: r3.lease_id, holder: "author", candidate });
    recordHighReviewSet({ lifecycle, runtimeA, root, admitted: r3, suffix: "cap-r3" });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r3.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    assert.equal(fs.existsSync(path.join(runtimeA, "trusted-local", "architect-prompts", `${r3.round_id}.json`)), true, "a critical round that exhausts the prior cap must become eligible for a fresh ruling");
    assert.throws(() => lifecycle.recordArchitectDecision({ runtimeRoot: runtimeA, roundId: r2.round_id, operator: "maintainer", reportFile: decision }), /already recorded/i);
    const legacyAnchor = JSON.parse(fs.readFileSync(lifecycle.anchorPath)); delete legacyAnchor.rounds[r3.round_id].architect_escalation_required;
    fs.writeFileSync(lifecycle.anchorPath, `${JSON.stringify(legacyAnchor, null, 2)}\n`);
    lifecycle.recordArchitectDecision({ runtimeRoot: runtimeA, roundId: r3.round_id, operator: "maintainer", reportFile: decision });
    assert.equal(lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }).round_id, "ORC0-R4");
  });
});

test("an exact neutral Architect STOP decision remains terminal", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    for (let cycle = 1; cycle <= 2; cycle += 1) {
      const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
      lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
      recordHighReviewSet({ lifecycle, runtimeA, root, admitted, suffix: `stop-r${cycle}` });
      lifecycle.close({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    }
    const stop = path.join(root, "architect-stop.json"); fs.writeFileSync(stop, '{"provider":"openai","tool":"codex","model":"gpt-5.6-sol","reasoning_effort":"xhigh","decision":"STOP","additional_round_cap":0}\n');
    lifecycle.recordArchitectDecision({ runtimeRoot: runtimeA, roundId: "ORC0-R2", operator: "maintainer", reportFile: stop });
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }), /stopped by the neutral Architect/i);
  });
});

test("reconciled ORC0 R1 does not count as a review/fix cycle, but the second live failed cycle escalates", () => {
  const history = { type: "trusted-local-preactivation-history", acceptance_eligible: false, disposition: "REJECTED_AUDIT_ONLY", node_id: "ORC0", round_id: "ORC0-R1", lease_id: "historic-r1", minimum_successor_round_ordinal: 2 };
  fixture(({ runtimeA, lifecycle, root }) => {
    const fail = (admitted, suffix) => {
      const finalized = lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
      assert.match(finalized.review_target_sha256, /^[0-9a-f]{64}$/);
      recordHighReviewSet({ lifecycle, runtimeA, root, admitted, suffix });
    };
    const first = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    assert.equal(first.round_id, "ORC0-R2"); assert.equal(first.review_cycle_ordinal, 1); fail(first, "first-live");
    assert.equal(fs.existsSync(path.join(runtimeA, "trusted-local", "architect-prompts", `${first.round_id}.json`)), false);
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: first.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    const second = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    assert.equal(second.round_id, "ORC0-R3"); assert.equal(second.review_cycle_ordinal, 2); fail(second, "second-live");
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: second.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    assert.equal(fs.existsSync(path.join(runtimeA, "trusted-local", "architect-prompts", `${second.round_id}.json`)), true);
  }, { preactivationHistory: history });
});

test("finalization freezes an immutable review manifest repeated by review evidence", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const admitted = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    const finalized = lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: admitted.lease_id, holder: "author", candidate });
    assert.equal(finalized.review_target.candidate_sha, candidate.sha);
    assert.equal(finalized.review_target.candidate_tree, candidate.tree);
    assert.match(finalized.review_target_sha256, /^[0-9a-f]{64}$/);
    const prompt = path.join(root, "frozen.prompt"); const report = path.join(root, "frozen.json");
    fs.writeFileSync(prompt, "review frozen target\n"); fs.writeFileSync(report, '{"verdict":"PASS","findings":[]}\n');
    const evidence = lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: admitted.round_id, leaseId: admitted.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/frozen-adversarial", agentIdentity: "frozen-reviewer", provider: "openai", model: "gpt-5.6-sol", effort: "high", promptFile: prompt, reportFile: report });
    assert.deepEqual(evidence.review_target, finalized.review_target);
    assert.equal(evidence.review_target_sha256, finalized.review_target_sha256);
  });
});
