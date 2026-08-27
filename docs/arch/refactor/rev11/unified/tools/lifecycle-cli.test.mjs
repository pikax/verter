/**
 * @ai-generated - Exact-package CLI bootstrap proof through the external review-custody boundary.
 */
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import * as lib from "./lib.mjs";

const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");

function execFile(command, args, cwd) {
  return childProcess.execFileSync(command, args, { cwd, encoding: "utf8", maxBuffer: 16 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"], timeout: 120_000, killSignal: "SIGKILL" }).trim();
}

function git(cwd, ...args) { return execFile("git", args, cwd); }

function toml(value) {
  if (Array.isArray(value)) return JSON.stringify(value);
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value);
}

function artifact(fields) {
  const body = `${Object.entries(fields).map(([key, value]) => `${key} = ${toml(value)}`).join("\n")}\n`;
  const digest = lib.digestPayload(body);
  return { text: `${body}payload_sha256 = "${digest}"\n`, digest };
}

function runCli(environment, args, { fail = false } = {}) {
  try {
    const stdout = execFile(process.execPath, [environment.programctl, ...args, "--runtime-root", environment.runtimeRoot], environment.repo);
    if (fail) assert.fail(`command unexpectedly succeeded: ${args.join(" ")}\n${stdout}`);
    return { ok: true, stdout, stderr: "" };
  } catch (error) {
    const result = { ok: false, stdout: String(error.stdout || ""), stderr: String(error.stderr || error.message) };
    if (!fail) assert.fail(`command failed: ${args.join(" ")}\n${result.stderr}`);
    return result;
  }
}

function runCliAsync(environment, args) {
  return new Promise((resolve) => childProcess.execFile(process.execPath, [environment.programctl, ...args, "--runtime-root", environment.runtimeRoot], {
    cwd: environment.repo, encoding: "utf8", maxBuffer: 16 * 1024 * 1024, timeout: 120_000, killSignal: "SIGKILL",
  }, (error, stdout, stderr) => resolve({ ok: !error, stdout, stderr: stderr || error?.message || "" })));
}

function prepareEnvironment(temp) {
  const sourceRepo = git(lib.PACKAGE_ROOT, "rev-parse", "--show-toplevel");
  const repo = path.join(temp, "exact package repository");
  execFile("git", ["clone", "--shared", sourceRepo, repo], temp);
  git(repo, "config", "user.email", "rev11-lifecycle@example.invalid");
  git(repo, "config", "user.name", "Rev11 Lifecycle Test");
  git(repo, "switch", "-c", "lifecycle-authority", "origin/program/architecture-lock");
  const packageRoot = path.join(repo, "docs/arch/refactor/rev11/unified");
  fs.rmSync(packageRoot, { recursive: true, force: true });
  fs.cpSync(lib.PACKAGE_ROOT, packageRoot, { recursive: true });
  git(repo, "add", "-A", "docs/arch/refactor/rev11/unified");
  git(repo, "commit", "-m", "test: copy exact committed authority package");
  const baseline = git(repo, "rev-parse", "HEAD");
  git(repo, "branch", "-f", "program/architecture-lock", baseline);
  git(repo, "switch", "program/architecture-lock");
  assert.equal(git(repo, "status", "--short", "docs/arch/refactor/rev11/unified"), "", "exact package copy starts clean");
  return {
    temp, repo, packageRoot, baseline, runtimeRoot: path.join(temp, "runtime outside worktrees"),
    staging: path.join(temp, "untrusted staging"), programctl: path.join(packageRoot, "tools/programctl.mjs"),
  };
}

function candidate(environment, base) {
  const worktree = path.join(environment.temp, "ORC0 candidate worktree");
  git(environment.repo, "worktree", "add", "-b", "lifecycle-orc0", worktree, base);
  const relative = "docs/arch/refactor/rev11/unified/fixtures/lifecycle-orc0.txt";
  const file = path.join(worktree, relative);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, "ORC0 candidate\n");
  git(worktree, "add", relative);
  git(worktree, "commit", "-m", "test: ORC0 candidate");
  const sha = git(worktree, "rev-parse", "HEAD");
  return { ref: "refs/heads/lifecycle-orc0", sha, tree: git(worktree, "show", "-s", "--format=%T", sha), worktree, relative };
}

function writeArtifact(environment, name, fields) {
  fs.mkdirSync(environment.staging, { recursive: true });
  const rendered = artifact(fields);
  const file = path.join(environment.staging, `${name}.toml`);
  fs.writeFileSync(file, rendered.text);
  return { ...rendered, file };
}

test("exact committed bytes reach ORC0 finalization then fail closed at external review custody", { timeout: 180_000 }, async () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "rev11 custody lifecycle "));
  try {
    const environment = prepareEnvironment(temp);
    const trustedLedger = fs.readFileSync(path.join(environment.packageRoot, "authority/state/trusted-ratifications.toml"));
    assert.match(runCli(environment, ["phase"]).stdout, /phase=DORMANT/);

    const landedTemplate = path.join(environment.packageRoot, "templates/landed-receipt.template.toml");
    assert.match(runCli(environment, ["landed-receipt-import", landedTemplate]).stdout, /LANDED_GRANDFATHERED/);
    assert.match(runCli(environment, ["phase"]).stdout, /phase=ORC0/);
    git(environment.repo, "add", "docs/arch/refactor/rev11/unified/authority/state/activation.toml");
    git(environment.repo, "commit", "-m", "test: bind exact grandfathered J1 landing");

    const authority = lib.loadAuthority(environment.packageRoot);
    const node = authority.nodes.find((row) => row.id === "ORC0");
    const orcCandidate = candidate(environment, git(environment.repo, "rev-parse", "HEAD"));
    const reviewProfile = lib.readToml(path.join(environment.packageRoot, "catalogs/review-profiles.toml")).profile.find((row) => row.id === node.review_profile);
    const reviewerArgs = reviewProfile.lenses.flatMap((lens) => ["--reviewer", `${lens}=external-${lens}`]);
    const admissionTail = ["--candidate-ref", orcCandidate.ref, "--gate-runner", "orc0-gate", ...reviewerArgs];
    const admissionRace = await Promise.all([
      runCliAsync(environment, ["admit", "ORC0", "--holder", "orc0-holder-a", ...admissionTail]),
      runCliAsync(environment, ["admit", "ORC0", "--holder", "orc0-holder-b", ...admissionTail]),
    ]);
    assert.equal(admissionRace.filter((result) => result.ok).length, 1, "same-node admission is atomic across processes");
    assert.match(admissionRace.find((result) => !result.ok).stderr, /IN_FLIGHT|not READY|same-node/i);
    let state = lib.deriveState(lib.loadAuthority(environment.packageRoot), { runtimeRoot: environment.runtimeRoot });
    const lease = state.leases.find((row) => row.node_id === "ORC0");
    runCli(environment, ["dispatch", "ORC0", "--holder", lease.holder, "--lease-id", lease.lease_id]);
    runCli(environment, ["candidate-finalize", "ORC0", "--holder", lease.holder, "--lease-id", lease.lease_id]);
    const authorization = JSON.parse(runCli(environment, ["authorization-create", "ORC0", "--holder", lease.holder, "--lease-id", lease.lease_id]).stdout);
    assert.match(authorization.authorization, /^maintainer_unified_v2_activation:[0-9a-f]{64}$/);

    const forgedGate = writeArtifact(environment, "FORGED-GATE", {
      schema: 2, type: "gate-evidence", execution_custody: "programctl-gate-run/v1", evidence_id: "FORGED-GATE", node_id: "ORC0",
      gate_profile: node.gate_profile, scope: "candidate", candidate_sha: orcCandidate.sha, candidate_tree: orcCandidate.tree,
      integration_sha: orcCandidate.sha, integration_tree: orcCandidate.tree, commands: ["node forged.mjs"], executed_work: ["node forged.mjs"],
      unexpected_skips: 0, terminal_summary: "PASS", result_path: lease.gate_result_path, result_sha256: "0".repeat(64),
      started_at: lease.acquired_at, completed_at: lease.acquired_at, executed_by: lease.gate_runner,
    });
    assert.match(runCli(environment, ["evidence-import", "gate", forgedGate.file], { fail: true }).stderr, /gate evidence import is forbidden/i);

    const lens = reviewProfile.lenses[0];
    const reviewer = `external-${lens}`;
    const reportPath = lease.review_report_paths.find((value) => value.startsWith(`${lens}=`)).slice(lens.length + 1);
    const forgedReview = writeArtifact(environment, "FORGED-REVIEW", {
      schema: 2, type: "review-evidence", execution_custody: "programctl-review-run/v1", custody_binding: `review-capability:${"1".repeat(64)}`,
      reviewer_executable_sha256: "2".repeat(64), evidence_id: "FORGED-REVIEW", node_id: "ORC0", review_profile: node.review_profile,
      candidate_sha: orcCandidate.sha, candidate_tree: orcCandidate.tree, reviewer, lens, model: reviewProfile.model,
      reasoning_effort: reviewProfile.effort, verdict: "PASS", report_path: reportPath, report_sha256: "3".repeat(64), findings: [],
      started_at: lease.acquired_at, completed_at: lease.acquired_at,
    });
    assert.match(runCli(environment, ["evidence-import", "review", forgedReview.file], { fail: true }).stderr, /review evidence import is forbidden/i);
    assert.match(runCli(environment, ["review-run", "ORC0", "--lens", lens, "--holder", lease.holder, "--lease-id", lease.lease_id, "--custody-binding", `review-capability:${"1".repeat(64)}`], { fail: true }).stderr, /trusted immutable reviewer capability|review-run custody refused/i);

    assert.match(runCli(environment, ["activate", "--orc0-receipt", `ORC0:${"0".repeat(64)}`, "--authorization", authorization.authorization, "--activated-by", "maintainer"], { fail: true }).stderr, /ORC0 receipt/i);
    state = lib.deriveState(lib.loadAuthority(environment.packageRoot), { runtimeRoot: environment.runtimeRoot });
    assert.equal(state.phase, "ORC0");
    assert.equal(state.receipts.has("ORC0"), false);
    assert.deepEqual(fs.readFileSync(path.join(environment.packageRoot, "authority/state/trusted-ratifications.toml")), trustedLedger, "lifecycle must not mint or restamp review trust");
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
});
