/**
 * @ai-generated - Discriminating tests for the trusted-local lifecycle control plane.
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  assessNodeEffort,
  createLocalLifecycle,
  readLocalAnchor,
  reinitializeLocalLifecycle,
} from "./trusted-local.mjs";

function fixture(run) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rev11-trusted-local-"));
  try {
    const controlRoot = path.join(root, "control");
    const runtimeA = path.join(root, "runtime-a");
    const runtimeB = path.join(root, "runtime-b");
    const lifecycle = createLocalLifecycle({ controlRoot });
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

test("one repo-global anchor excludes the same node and domain across runtime roots without partial publication", () => {
  fixture(({ runtimeA, runtimeB, lifecycle }) => {
    const first = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author-a" });
    assert.equal(first.round_id, "ORC0-R1");
    const beforeAnchor = fs.readFileSync(lifecycle.anchorPath);
    const beforeRuntime = fs.existsSync(runtimeB) ? fs.readdirSync(runtimeB) : [];
    assert.throws(
      () => lifecycle.admit({ runtimeRoot: runtimeB, node: { ...highNode, id: "ORC0-ALIAS" }, candidate, holder: "author-b" }),
      /conflict domain|active node/i,
    );
    assert.deepEqual(fs.existsSync(runtimeB) ? fs.readdirSync(runtimeB) : [], beforeRuntime);
    assert.deepEqual(fs.readFileSync(lifecycle.anchorPath), beforeAnchor);
  });
});

test("round ordinals are global and an old closed round cannot regain acceptance", () => {
  fixture(({ runtimeA, runtimeB, lifecycle }) => {
    const r1 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r1.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
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
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeB, node: highNode, candidate, holder: "other" }), /active node|conflict domain/);
    const anchor = readLocalAnchor({ controlRoot: path.dirname(lifecycle.anchorPath) });
    assert.equal(Object.values(anchor.leases).filter((lease) => lease.node_id === "ORC0").length, 1, failpoint);
    assert.equal(fs.existsSync(path.join(path.dirname(lifecycle.anchorPath), "transaction.json")), false, failpoint);
  });
});

test("round-two P1 requires the fixed neutral Architect and an explicit additional-round cap", () => {
  fixture(({ runtimeA, lifecycle, root }) => {
    const r1 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }); lifecycle.close({ runtimeRoot: runtimeA, leaseId: r1.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    const r2 = lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }); lifecycle.finalize({ runtimeRoot: runtimeA, leaseId: r2.lease_id, holder: "author", candidate });
    const prompt = path.join(root, "review.prompt"); const report = path.join(root, "review.json"); fs.writeFileSync(prompt, "review\n"); fs.writeFileSync(report, '{"verdict":"FAIL","findings":[{"severity":"P1"}]}\n');
    lifecycle.recordReview({ runtimeRoot: runtimeA, roundId: r2.round_id, leaseId: r2.lease_id, holder: "author", lens: "adversarial", taskIdentity: "/root/r2-adversarial", provider: "openai", model: "gpt-5.6-sol", effort: "high", promptFile: prompt, reportFile: report });
    lifecycle.close({ runtimeRoot: runtimeA, leaseId: r2.lease_id, holder: "author", outcome: "FIX_REQUIRED" });
    assert.throws(() => lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }), /Architect decision/);
    const decision = path.join(root, "architect.json"); fs.writeFileSync(decision, '{"provider":"openai","tool":"codex","model":"gpt-5.6-sol","reasoning_effort":"xhigh","decision":"CONTINUE","additional_round_cap":1}\n');
    lifecycle.recordArchitectDecision({ runtimeRoot: runtimeA, roundId: r2.round_id, operator: "maintainer", reportFile: decision });
    assert.equal(lifecycle.admit({ runtimeRoot: runtimeA, node: highNode, candidate, holder: "author" }).round_id, "ORC0-R3");
  });
});
