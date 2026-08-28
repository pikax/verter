/** @ai-generated - Canonical CLI proof for the trusted-local ORC0 lifecycle. */
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import * as lib from "./lib.mjs";

function exec(command, args, cwd) { return childProcess.execFileSync(command, args, { cwd, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, timeout: 120_000, killSignal: "SIGKILL" }).trim(); }
function git(cwd, ...args) { return exec("git", args, cwd); }
function cli(env, args) { return exec(process.execPath, [env.programctl, ...args, "--runtime-root", env.runtimeRoot], env.repo); }
function prepare(temp) {
  const source = git(lib.PACKAGE_ROOT, "rev-parse", "--show-toplevel"); const repo = path.join(temp, "repo");
  exec("git", ["clone", "--shared", source, repo], temp); git(repo, "config", "user.email", "trusted-local@example.invalid"); git(repo, "config", "user.name", "Trusted Local Test"); git(repo, "switch", "-c", "program/architecture-lock", "HEAD");
  const packageRoot = path.join(repo, "docs/arch/refactor/rev11"); fs.rmSync(packageRoot, { recursive: true, force: true }); fs.cpSync(lib.PACKAGE_ROOT, packageRoot, { recursive: true });
  git(repo, "add", "-A", "docs/arch/refactor/rev11");
  if (git(repo, "status", "--porcelain=v1", "--", "docs/arch/refactor/rev11")) git(repo, "commit", "-m", "test: install exact authority package");
  return { repo, packageRoot, runtimeRoot: path.join(temp, "external runtime"), programctl: path.join(packageRoot, "tools/programctl.mjs") };
}
function createCandidate(env) {
  const worktree = path.join(path.dirname(env.repo), "candidate"); git(env.repo, "worktree", "add", "-b", "codex/orc0-test", worktree, "HEAD");
  const file = path.join(worktree, "docs/arch/refactor/rev11/fixtures/orc0-trusted-local.txt"); fs.writeFileSync(file, "trusted-local candidate\n");
  git(worktree, "add", "docs/arch/refactor/rev11/fixtures/orc0-trusted-local.txt"); git(worktree, "commit", "-m", "test: trusted-local ORC0 candidate");
  return { worktree, ref: "refs/heads/codex/orc0-test" };
}
function harnessFiles(temp, name) {
  const prompt = path.join(temp, `${name}.prompt.txt`); const report = path.join(temp, `${name}.report.json`);
  fs.writeFileSync(prompt, `fresh harness task ${name}\n`); fs.writeFileSync(report, '{"verdict":"PASS","findings":[]}\n'); return { prompt, report };
}

test("programctl drives J1 grandfathering through trusted-local ORC0 activation", { timeout: 180_000 }, () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "rev11-trusted-cli-"));
  try {
    const env = prepare(temp); assert.match(cli(env, ["phase"]), /phase=DORMANT/);
    cli(env, ["landed-receipt-import", path.join(env.packageRoot, "templates/landed-receipt.template.toml")]); assert.match(cli(env, ["phase"]), /phase=ORC0/);
    git(env.repo, "add", "docs/arch/refactor/rev11/authority/state/activation.toml"); git(env.repo, "commit", "-m", "test: bind J1 grandfathered landing");
    const candidate = createCandidate(env); const admission = JSON.parse(cli(env, ["admit", "ORC0", "--holder", "maintainer", "--candidate-ref", candidate.ref]));
    assert.equal(admission.round_id, "ORC0-R2", "rejected preactivation R1 is reconciled append-only"); assert.equal(admission.effort_policy.effective.review, "high");
    const packet = JSON.parse(cli(env, ["dispatch", "ORC0", "--holder", "maintainer", "--lease-id", admission.lease_id])); assert.equal(packet.round_id, admission.round_id); assert.equal(packet.effort_policy.effective.review, "high");
    cli(env, ["candidate-finalize", "ORC0", "--holder", "maintainer", "--lease-id", admission.lease_id]);
    assert.deepEqual(packet.review_policy.review_lenses, ["adversarial", "conformance", "wire-public"]);
    const candidateFile = path.join(candidate.worktree, "docs/arch/refactor/rev11/fixtures/orc0-trusted-local.txt");
    fs.appendFileSync(candidateFile, "review-side mutation\n");
    const refusedFiles = harnessFiles(temp, "mutated-target");
    assert.throws(() => cli(env, ["harness-record", "--role", "review", "--round-id", admission.round_id, "--lease-id", admission.lease_id, "--holder", "maintainer", "--lens", "adversarial", "--task", "/root/orc0-mutated", "--agent", "mutated-reviewer", "--provider", "openai", "--model", "gpt-5.6-sol", "--effort", "high", "--prompt", refusedFiles.prompt, "--report", refusedFiles.report]), /clean|frozen review target/i);
    fs.writeFileSync(candidateFile, "trusted-local candidate\n");
    const disposableReview = path.join(temp, "reviewer-worktree"); git(env.repo, "worktree", "add", "--detach", disposableReview, packet.candidate.sha);
    let writableEvidence;
    for (const lens of packet.review_policy.review_lenses) {
      const files = harnessFiles(temp, lens); const modeArgs = lens === "adversarial" ? ["--worktree-mode", "write-enabled", "--disposable-worktree", disposableReview] : [];
      const recorded = JSON.parse(cli(env, ["harness-record", "--role", "review", "--round-id", admission.round_id, "--lease-id", admission.lease_id, "--holder", "maintainer", "--lens", lens, "--task", `/root/orc0-${lens}`, "--agent", `reviewer-${lens}`, "--provider", lens === "conformance" ? "openai" : "anthropic", "--model", lens === "conformance" ? "gpt-5.6-sol" : "claude-fresh", "--effort", "high", "--prompt", files.prompt, "--report", files.report, ...modeArgs]));
      if (lens === "adversarial") writableEvidence = recorded;
    }
    assert.throws(() => cli(env, ["round-accept", admission.round_id, "--holder", "maintainer"]), /cleanup/i);
    git(env.repo, "worktree", "remove", disposableReview);
    cli(env, ["review-cleanup-record", "--evidence-id", writableEvidence.evidence_id, "--holder", "maintainer", "--worktree", disposableReview]);
    for (const role of ["verification", "confirmation"]) {
      const files = harnessFiles(temp, role); cli(env, ["harness-record", "--role", role, "--round-id", admission.round_id, "--lease-id", admission.lease_id, "--holder", "maintainer", "--task", `/root/orc0-${role}`, "--agent", `${role}-agent`, "--provider", "openai", "--model", "gpt-5.6-sol", "--effort", "high", "--prompt", files.prompt, "--report", files.report]);
    }
    const accepted = JSON.parse(cli(env, ["round-accept", admission.round_id, "--holder", "maintainer"])); assert.equal(accepted.type, "trusted-local-acceptance");
    git(env.repo, "merge", "--ff-only", candidate.ref);
    assert.equal(git(env.repo, "status", "--porcelain=v1", "--untracked-files=all"), "");
    const landing = JSON.parse(cli(env, ["landing-record", "--round-id", admission.round_id, "--holder", "maintainer"])); assert.equal(landing.canonical_sha, accepted.candidate_sha);
    const activated = JSON.parse(cli(env, ["activate", "--activated-by", "maintainer"])); assert.equal(activated.phase, "ACTIVE"); assert.match(cli(env, ["phase"]), /phase=ACTIVE/);
    assert.equal(git(env.repo, "status", "--porcelain=v1", "--untracked-files=all"), "", "activation must not mutate tracked authority after review");
    git(env.repo, "worktree", "remove", candidate.worktree);
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
});
