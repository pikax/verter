/**
 * @ai-generated - Adversarial tests for the unified authority trust boundaries.
 */
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import childProcess from "node:child_process";
import * as lib from "./lib.mjs";
import {
  PACKAGE_ROOT,
  digestPayload,
  loadAuthority,
  parseToml,
  validateAuthority,
  validateCharters,
  validateReceiptFile,
} from "./lib.mjs";

const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");

function withTempDir(prefix, run) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  try {
    return run(directory);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

function receiptText(node, fields = {}) {
  const head = fields.candidate_sha || "2".repeat(40);
  const tree = fields.candidate_tree || "3".repeat(40);
  const body = [
    "schema = 2",
    'type = "v2-acceptance"',
    `node_id = ${JSON.stringify(node.id)}`,
    'verdict = "ACCEPTED"',
    `base_sha = ${JSON.stringify(fields.base_sha || "1".repeat(40))}`,
    `candidate_sha = ${JSON.stringify(head)}`,
    `candidate_tree = ${JSON.stringify(tree)}`,
    `integration_sha = ${JSON.stringify(fields.integration_sha || head)}`,
    `integration_tree = ${JSON.stringify(fields.integration_tree || tree)}`,
    `authority_sha256 = "${"4".repeat(64)}"`,
    `charter_sha256 = "${sha256(fs.readFileSync(path.join(PACKAGE_ROOT, node.charter)))}"`,
    'activation_receipt = ""',
    "predecessor_receipts = []",
    "opened_conditionals = []",
    "conditional_predecessor_receipts = []",
    "external_authorizations = []",
    `lease_receipt = "LEASE:${"5".repeat(64)}"`,
    `dispatch_receipt = "DISPATCH:${"9".repeat(64)}"`,
    `finalization_receipt = "FINAL:${"a".repeat(64)}"`,
    'lease_holder = "receipt-holder"',
    'candidate_ref = "refs/heads/receipt-candidate"',
    'changed_paths = ["docs/arch/refactor/rev11/unified/fixtures/receipt-candidate.txt"]',
    `gate_receipts = ["GATE-C:${"6".repeat(64)}", "GATE-I:${"7".repeat(64)}"]`,
    `review_receipts = ["REVIEW:${"8".repeat(64)}"]`,
    'accepted_at = "2026-08-27T12:00:00.000Z"',
    'accepted_by = "receipt-holder"',
    "",
  ].join("\n");
  return `${body}payload_sha256 = "${digestPayload(body)}"\n`;
}

test("parseToml rejects prototype-bearing table paths without polluting globals", () => {
  delete Object.prototype.rev11_review_polluted;
  try {
    assert.throws(
      () => parseToml('[__proto__]\nrev11_review_polluted = "yes"\n'),
      /unsafe|prototype/i,
    );
    assert.equal(Object.prototype.rev11_review_polluted, undefined);
  } finally {
    delete Object.prototype.rev11_review_polluted;
  }
});

test("parseToml rejects integers that cannot round-trip exactly", () => {
  assert.throws(() => parseToml("schema = 9007199254740993\n"), /safe integer/i);
});

test("receipt validation rejects digest-valid nonexistent Git identities", () => {
  const authority = loadAuthority();
  const node = authority.nodes.find((candidate) => candidate.id === "ORC0");
  withTempDir("rev11-receipt-git-", (directory) => {
    const file = path.join(directory, "ORC0.toml");
    fs.writeFileSync(file, receiptText(node));
    assert.match(validateReceiptFile(file, node, PACKAGE_ROOT).errors.join("\n"), /Git|commit|identity/i);
  });
});

test("receipt validation rejects authenticated-prefix suffix injection", () => {
  const authority = loadAuthority();
  const node = authority.nodes.find((candidate) => candidate.id === "ORC0");
  withTempDir("rev11-receipt-suffix-", (directory) => {
    const file = path.join(directory, "ORC0.toml");
    fs.writeFileSync(file, `${receiptText(node)}post_digest_override = "attacker-controlled"\n`);
    assert.match(validateReceiptFile(file, node, PACKAGE_ROOT).errors.join("\n"), /payload_sha256.*final|suffix|additional/i);
  });
});

test("schema-incomplete routed receipts fail closed without typed-field crashes", () => {
  const authority = loadAuthority();
  const node = authority.nodes.find((candidate) => candidate.id === "ORC0");
  withTempDir("rev11-receipt-incomplete-", (directory) => {
    const file = path.join(directory, "ORC0.toml");
    const body = 'schema = 2\ntype = "v2-acceptance"\nnode_id = "ORC0"\n';
    fs.writeFileSync(file, `${body}payload_sha256 = "${digestPayload(body)}"\n`);
    let checked;
    assert.doesNotThrow(() => { checked = validateReceiptFile(file, node, PACKAGE_ROOT); });
    assert.match(checked.errors.join("\n"), /required|opened_conditionals|verdict/i);
  });
});

test("strict authority validation applies the activation schema", () => {
  withTempDir("rev11-schema-", (directory) => {
    fs.cpSync(PACKAGE_ROOT, directory, { recursive: true });
    const activation = path.join(directory, "authority/state/activation.toml");
    fs.appendFileSync(activation, 'attacker_field = "accepted"\n');
    const errors = validateAuthority(loadAuthority(directory), { strict: true, checkGenerated: false });
    assert.match(errors.join("\n"), /activation.*attacker_field|additional propert/i);
  });
});

test("charter validation rejects traversal before reading a file", () => {
  const authority = loadAuthority();
  const node = structuredClone(authority.nodes.find((candidate) => candidate.id === "ORC0"));
  node.charter = "../outside.md";
  assert.match(validateCharters([node], PACKAGE_ROOT).join("\n"), /unsafe charter path|outside.*charter/i);
});

test("authority loading refuses a symlinked DAG module", () => {
  withTempDir("rev11-module-link-", (directory) => {
    const authorityRoot = path.join(directory, "authority");
    fs.mkdirSync(path.join(authorityRoot, "dag"), { recursive: true });
    fs.writeFileSync(path.join(authorityRoot, "root.toml"), 'modules = ["dag/escaped.toml"]\n');
    const outside = path.join(directory, "outside.toml");
    fs.writeFileSync(outside, "[[node]]\nid = \"A\"\n");
    fs.symlinkSync(outside, path.join(authorityRoot, "dag/escaped.toml"));
    assert.throws(() => loadAuthority(directory), /symlink|confined|unsafe/i);
  });
});

test("receipt validation requires exact predecessor and evidence context even for real Git objects", () => {
  const authority = loadAuthority();
  const node = authority.nodes.find((candidate) => candidate.id === "ORC0");
  const head = childProcess.execFileSync("git", ["rev-parse", "HEAD"], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
  const tree = childProcess.execFileSync("git", ["show", "-s", "--format=%T", head], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
  withTempDir("rev11-receipt-context-", (directory) => {
    const file = path.join(directory, "ORC0.toml");
    fs.writeFileSync(file, receiptText(node, { base_sha: head, candidate_sha: head, candidate_tree: tree, integration_sha: head, integration_tree: tree }));
    assert.match(validateReceiptFile(file, node, PACKAGE_ROOT).errors.join("\n"), /predecessor|validation context|gate receipt|review receipt|external authorization/i);
  });
});

test("external authorization is rejected unless its digest is immutably allowlisted", () => {
  const authority = loadAuthority();
  withTempDir("rev11-external-manifest-", (runtimeRoot) => {
    const external = path.join(runtimeRoot, "external");
    fs.mkdirSync(external, { recursive: true });
    const body = `schema = 2\nauthorization = "maintainer_unified_v2_activation"\nnode_id = "ORC0"\ncandidate_tree = "${"1".repeat(40)}"\ngranted_by = "maintainer"\nexpires_at = "never"\n`;
    fs.writeFileSync(path.join(external, "ORC0.toml"), `${body}payload_sha256 = "${digestPayload(body)}"\n`);
    assert.match(lib.deriveState(authority, { runtimeRoot }).errors.join("\n"), /allowlist|immutable|authorization schema/i);
  });
});

test("the closed legacy set is manifest-bound, not filename-derived", () => {
  withTempDir("rev11-legacy-manifest-", (directory) => {
    fs.cpSync(PACKAGE_ROOT, directory, { recursive: true });
    const authority = loadAuthority(directory);
    const node = authority.nodes.find((candidate) => candidate.id === "ORC0");
    const head = childProcess.execFileSync("git", ["rev-parse", "HEAD"], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
    const tree = childProcess.execFileSync("git", ["show", "-s", "--format=%T", head], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
    const body = `schema = 2\ntype = "legacy-accepted"\nnode_id = "ORC0"\nverdict = "ACCEPTED"\naccepted_sha = "${head}"\naccepted_tree = "${tree}"\ncandidate_sha = "${head}"\ncandidate_tree = "${tree}"\nintegration_sha = "${head}"\nintegration_tree = "${tree}"\ncharter_sha256 = "${sha256(fs.readFileSync(path.join(directory, node.charter)))}"\nsource_ledger_sha256 = "${"0".repeat(64)}"\npredecessors = []\ngate_evidence = "forged"\nreview_evidence = "forged"\n`;
    fs.writeFileSync(path.join(directory, "state/legacy-receipts/ORC0.toml"), `${body}payload_sha256 = "${digestPayload(body)}"\n`);
    assert.match(lib.deriveState(authority, { runtimeRoot: path.join(directory, ".runtime-test") }).errors.join("\n"), /legacy.*manifest|unlisted legacy/i);
  });
});

test("atomic admission and amendment APIs are part of the executable boundary", () => {
  assert.equal(typeof lib.admitNode, "function");
  assert.equal(typeof lib.renewLease, "function");
  assert.equal(typeof lib.releaseLease, "function");
  assert.equal(typeof lib.computeAuthorityDigest, "function");
  assert.equal(typeof lib.validateAmendments, "function");
  assert.equal(typeof lib.createAmendment, "function");
  assert.equal(typeof lib.dispatchNode, "function");
  assert.equal(typeof lib.finalizeCandidate, "function");
  assert.equal(typeof lib.validateLandedReceiptFile, "function");
  assert.equal(typeof lib.createDirectiveAuthorization, "function");
  assert.equal(typeof lib.executeCommandPlan, "function");
  assert.equal(typeof lib.runGateEvidence, "function");
  assert.equal(typeof lib.runReviewEvidence, "function");
  assert.equal(typeof lib.validateReviewCapability, "function");
});

test("gate execution custody runs real direct subprocesses and fails closed on exit and timeout", () => {
  withTempDir("rev11-gate-custody-", (directory) => {
    const pass = path.join(directory, "pass.mjs");
    const fail = path.join(directory, "fail.mjs");
    const hang = path.join(directory, "hang.mjs");
    fs.writeFileSync(pass, 'process.stdout.write("real gate execution\\n");\n');
    fs.writeFileSync(fail, "process.exit(7);\n");
    fs.writeFileSync(hang, "setInterval(() => {}, 1000);\n");
    const [result] = lib.executeCommandPlan([`${process.execPath} ${pass}`], { cwd: directory, timeoutMs: 10_000 });
    assert.equal(result.status, 0);
    assert.equal(result.stdout, "real gate execution\n");
    assert.equal(result.stdout_sha256, sha256(result.stdout));
    assert.throws(() => lib.executeCommandPlan([`${process.execPath} ${fail}`], { cwd: directory, timeoutMs: 10_000 }), /status=7/);
    assert.throws(() => lib.executeCommandPlan([`${process.execPath} ${hang}`], { cwd: directory, timeoutMs: 50 }), /ETIMEDOUT|timed out|gate command failed/i);
  });
});

test("review custody refuses a locally claimed capability absent immutable trusted ratification", () => {
  const authority = loadAuthority();
  const checked = lib.validateReviewCapability(authority, {
    evidence_id: "FORGED-REVIEW", custody_binding: `review-capability:${"1".repeat(64)}`,
    reviewer_executable_sha256: "2".repeat(64), reviewer: "local-reviewer", lens: "architecture",
    model: "gpt-5.6-sol", reasoning_effort: "ultra",
  });
  assert.match(checked.errors.join("\n"), /trusted immutable reviewer capability/i);
  assert.equal(checked.script, null);
});

test("the canonical docs gate discovers every tool and test with a digest-bound plan", () => {
  const output = childProcess.execFileSync(process.execPath, [path.join(PACKAGE_ROOT, "tools/run-docs-gate.mjs"), "--list"], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" });
  const plan = JSON.parse(output);
  const actual = fs.readdirSync(path.join(PACKAGE_ROOT, "tools")).filter((name) => name.endsWith(".mjs")).map((name) => `docs/arch/refactor/rev11/unified/tools/${name}`).sort();
  assert.deepEqual(plan.syntax_inputs, actual);
  assert.deepEqual(plan.test_inputs, actual.filter((name) => name.endsWith(".test.mjs")));
  assert.match(plan.discovery_sha256, /^[0-9a-f]{64}$/);
});

test("cold packets embed the exact node-scoped source clauses for small and large applicability sets", () => {
  const authority = loadAuthority();
  for (const id of ["CCA1", "ORC0"]) {
    const node = authority.nodes.find((candidate) => candidate.id === id);
    const attachment = fs.readFileSync(path.join(PACKAGE_ROOT, `provenance/packet-source-clauses/${id}.md`), "utf8").trimEnd();
    const section = lib.packetSourceBindings(authority, node);
    assert.equal(section.includes(attachment), true, `${id} exact clause bytes must be in the persisted packet body`);
    assert.doesNotMatch(section, /Resolve only|Open only this node-specific attachment/);
  }
});

test("review findings require candidate-bound one-time P2 disposition and authenticated next-cycle closure", () => {
  withTempDir("rev11-finding-disposition-", (directory) => {
    fs.cpSync(PACKAGE_ROOT, directory, { recursive: true });
    const authority = loadAuthority(directory);
    const candidateSha = "1".repeat(40); const candidateTree = "2".repeat(40);
    const completedAt = "2026-08-27T20:00:00.000Z"; const closedAt = "2026-08-27T20:30:00.000Z"; const expiresAt = "2099-08-27T21:00:00.000Z";
    const finding = {
      severity: "P2", fingerprint: "f".repeat(64), owner: "maintainer", class_wide_sweep: ["all matching call sites"],
      next_cycle_obligation: "close every matching call site in the ratified next cycle",
      next_cycle_receipt: "", authorization_binding: "", status: "AUTHORIZED_DEFERRED",
    };
    const closure = { schema: 1, type: "next-cycle-obligation-closure", receipt_id: "NEXT-REVIEW", node_id: "ORC0", candidate_sha: candidateSha, candidate_tree: candidateTree, review_profile: "public-3", lens: "wire-public", severity: "P2", fingerprint: finding.fingerprint, owner: finding.owner, obligation: finding.next_cycle_obligation, status: "CLOSED", closed_at: closedAt, closure_evidence: ["class-wide sweep receipt SHA-256:abc"] };
    const closureBytes = Buffer.from(`${JSON.stringify(closure)}\n`); const closureDigest = sha256(closureBytes);
    finding.next_cycle_receipt = `NEXT-REVIEW:${closureDigest}`;
    const disposition = { schema: 1, type: "one-time-finding-disposition", one_time: true, node_id: "ORC0", candidate_sha: candidateSha, candidate_tree: candidateTree, review_profile: "public-3", lens: "wire-public", severity: "P2", fingerprint: finding.fingerprint, owner: finding.owner, class_wide_sweep: finding.class_wide_sweep, expires_at: expiresAt, next_cycle_obligation: finding.next_cycle_obligation, next_cycle_receipt: finding.next_cycle_receipt };
    const receiptBytes = Buffer.from(`${JSON.stringify(disposition)}\n`);
    const receiptDigest = sha256(receiptBytes);
    finding.authorization_binding = `finding-disposition:${receiptDigest}`;
    const receiptDirectory = path.join(directory, "authority/state/ratification-receipts");
    fs.mkdirSync(receiptDirectory, { recursive: true });
    fs.writeFileSync(path.join(receiptDirectory, "p2-review.txt"), receiptBytes);
    fs.writeFileSync(path.join(receiptDirectory, "p2-next-cycle.txt"), closureBytes);
    const trustedLedger = [
      "schema = 2", "", "[[slot]]", 'purpose = "finding-disposition"', 'ratified_by = "maintainer"',
      'receipt_path = "authority/state/ratification-receipts/p2-review.txt"', `receipt_sha256 = "${receiptDigest}"`, "",
      "[[slot]]", 'purpose = "next-cycle-closure"', 'ratified_by = "maintainer"',
      'receipt_path = "authority/state/ratification-receipts/p2-next-cycle.txt"', `receipt_sha256 = "${closureDigest}"`, "",
    ].join("\n");
    const ledgerFile = path.join(directory, "authority/state/trusted-ratifications.toml");
    fs.writeFileSync(ledgerFile, trustedLedger);
    const row = { evidence_id: "R", node_id: "ORC0", candidate_sha: candidateSha, candidate_tree: candidateTree, review_profile: "public-3", lens: "wire-public", completed_at: completedAt, findings: [finding] };
    const now = Date.parse("2026-08-27T20:45:00.000Z");
    assert.deepEqual(lib.validateReviewFindingDispositions(authority, row, { now }), []);
    assert.match(lib.validateReviewFindingDispositions(authority, { ...row, findings: [{ ...finding, authorization_binding: `finding-disposition:${"0".repeat(64)}` }] }, { now }).join("\n"), /trusted disposition/i);
    assert.match(lib.validateReviewFindingDispositions(authority, { ...row, candidate_tree: "3".repeat(40) }, { now }).join("\n"), /candidate\/profile\/lens\/severity/i);
    assert.match(lib.validateReviewFindingDispositions(authority, { ...row, findings: [{ ...finding, severity: "P1" }] }, { now }).join("\n"), /blocking P1/i);
    assert.deepEqual(lib.validateReviewFindingDispositions(authority, { ...row, findings: [{ ...finding, severity: "P3", status: "CLOSED", authorization_binding: "", next_cycle_receipt: "" }] }, { now }), []);
    const tooLateClosure = { ...closure, closed_at: "2100-01-01T00:00:00.000Z" };
    fs.writeFileSync(path.join(receiptDirectory, "p2-next-cycle.txt"), `${JSON.stringify(tooLateClosure)}\n`);
    assert.match(lib.validateReviewFindingDispositions(authority, row, { now: Date.parse("2100-01-01T00:01:00.000Z") }).join("\n"), /digest mismatch|outside disposition expiry/i);
    fs.writeFileSync(path.join(receiptDirectory, "p2-next-cycle.txt"), closureBytes);
    fs.writeFileSync(ledgerFile, trustedLedger.split("[[slot]]")[0] + "[[slot]]" + trustedLedger.split("[[slot]]")[1]);
    assert.match(lib.validateReviewFindingDispositions(authority, row, { now }).join("\n"), /authenticated next-cycle closure/i);
  });
});

test("the exact committed custody boundary projects ORC0 only after finalization and keeps BR0, TCM0R, and amendments fail-closed", () => {
  const authority = loadAuthority();
  withTempDir("rev11-custody-refusal-", (runtimeRoot) => {
    const state = lib.deriveState(authority, { runtimeRoot });
    assert.deepEqual(state.errors, []);
    assert.match(state.states.get("BR0").blockers.join("; "), /missing immutable static directive slot maintainer_rev11_repair_freeze_lift|missing immutable static directive slot maintainer_successor_genesis/);
    assert.match(state.states.get("TCM0R").blockers.join("; "), /maintainer_tcm0_rescope_ratification|ORC0 activation/i);
    const head = childProcess.execFileSync("git", ["rev-parse", "HEAD"], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
    const tree = childProcess.execFileSync("git", ["show", "-s", "--format=%T", head], { cwd: PACKAGE_ROOT, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
    const directive = lib.readToml(path.join(PACKAGE_ROOT, "authority/state/external-authorizations.toml")).authorization[0];
    const body = `schema = 2\ntype = "external-authorization"\nauthorization = "${directive.authorization}"\nnode_id = "ORC0"\ncandidate_sha = "${head}"\ncandidate_tree = "${tree}"\nauthority_sha256 = "${lib.computeAuthorityDigest(PACKAGE_ROOT)}"\ngranted_by = "${directive.granted_by}"\nratification_path = "${directive.ratification_path}"\nratification_receipt_sha256 = "${directive.ratification_receipt_sha256}"\nexpires_at = "${directive.expires_at}"\ngrant_mode = "${directive.grant_mode}"\ndirective_scope = "${directive.directive_scope}"\n`;
    const artifactFile = path.join(runtimeRoot, "forged-local-authorization.toml");
    fs.writeFileSync(artifactFile, `${body}payload_sha256 = "${digestPayload(body)}"\n`);
    assert.throws(() => lib.importRuntimeArtifact(authority, { runtimeRoot, kind: "authorization", file: artifactFile }), /directive-mode authorization-import is forbidden/i);
    assert.throws(() => lib.createDirectiveAuthorization(authority, { runtimeRoot, id: "ORC0", holder: "holder", leaseId: "missing" }), /exact ORC0 lease|candidate-finalize/i);
    assert.throws(() => lib.createAmendment(authority, { id: "AMD-LOCAL-BOOTSTRAP", beforeRoot: PACKAGE_ROOT, ratifiedBy: "local-self-bootstrap", ratificationReceiptSha256: "1".repeat(64), runtimeRoot }), /trusted authority-amendment ratification slot/i);
  });
});
