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
function createCandidate(env, { nodeId = "ORC0", branch = "codex/orc0-test", directory = "candidate" } = {}) {
  const worktree = path.join(path.dirname(env.repo), directory); git(env.repo, "worktree", "add", "-b", branch, worktree, "HEAD");
  const relative = `docs/arch/refactor/rev11/fixtures/${nodeId.toLowerCase()}-trusted-local.txt`;
  const file = path.join(worktree, relative); fs.writeFileSync(file, nodeId === "ORC0" ? "trusted-local candidate\n" : `trusted-local ${nodeId} candidate\n`);
  git(worktree, "add", relative); git(worktree, "commit", "-m", `test: trusted-local ${nodeId} candidate`);
  return { worktree, ref: `refs/heads/${branch}` };
}
function harnessFiles(temp, name) {
  const prompt = path.join(temp, `${name}.prompt.txt`); const report = path.join(temp, `${name}.report.json`);
  fs.writeFileSync(prompt, `fresh harness task ${name}\n`); fs.writeFileSync(report, '{"verdict":"PASS","findings":[]}\n'); return { prompt, report };
}

test("programctl drives J1 grandfathering through trusted-local ORC0 activation and successor landing", { timeout: 240_000 }, () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "rev11-trusted-cli-"));
  try {
    const env = prepare(temp); const activationFile = path.join(env.packageRoot, "authority/state/activation.toml"); const dormantActivation = fs.readFileSync(activationFile);
    assert.match(cli(env, ["phase"]), /phase=DORMANT/);
    fs.writeFileSync(activationFile, dormantActivation.toString("utf8").replace(/^j1_receipt = ".*"$/m, `j1_receipt = "J1-LANDED-GRANDFATHERED:${"0".repeat(64)}"`));
    assert.throws(() => cli(env, ["landed-receipt-import", path.join(env.packageRoot, "templates/landed-receipt.template.toml")]), /authority lock current digest .* does not match/);
    fs.writeFileSync(activationFile, dormantActivation);
    cli(env, ["landed-receipt-import", path.join(env.packageRoot, "templates/landed-receipt.template.toml")]); assert.match(cli(env, ["phase"]), /phase=ORC0/);
    assert.deepEqual(fs.readFileSync(activationFile), dormantActivation, "J1 import must remain external and leave tracked activation authority unchanged");
    assert.equal(git(env.repo, "status", "--porcelain=v1", "--untracked-files=all"), "", "J1 import must not dirty the reviewed authority worktree");
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
    assert.match(cli(env, ["frontier"]), /(?:^|\n)CCA0(?:\n|$)/, "landed ORC0 acceptance must unlock CCA0");

    const successor = createCandidate(env, { nodeId: "CCA0", branch: "codex/cca0-test", directory: "cca0-candidate" });
    const successorAdmission = JSON.parse(cli(env, ["admit", "CCA0", "--holder", "compiler-maintainer", "--candidate-ref", successor.ref]));
    const successorPacket = JSON.parse(cli(env, ["dispatch", "CCA0", "--holder", "compiler-maintainer", "--lease-id", successorAdmission.lease_id]));
    cli(env, ["candidate-finalize", "CCA0", "--holder", "compiler-maintainer", "--lease-id", successorAdmission.lease_id]);
    for (const lens of successorPacket.review_policy.review_lenses) {
      const files = harnessFiles(temp, `cca0-${lens}`);
      cli(env, ["harness-record", "--role", "review", "--round-id", successorAdmission.round_id, "--lease-id", successorAdmission.lease_id, "--holder", "compiler-maintainer", "--lens", lens, "--task", `/root/cca0-${lens}`, "--agent", `cca0-reviewer-${lens}`, "--provider", "openai", "--model", "gpt-5.6-sol", "--effort", "high", "--prompt", files.prompt, "--report", files.report]);
    }
    for (const role of ["verification", "confirmation"]) {
      const files = harnessFiles(temp, `cca0-${role}`);
      cli(env, ["harness-record", "--role", role, "--round-id", successorAdmission.round_id, "--lease-id", successorAdmission.lease_id, "--holder", "compiler-maintainer", "--task", `/root/cca0-${role}`, "--agent", `cca0-${role}-agent`, "--provider", "openai", "--model", "gpt-5.6-sol", "--effort", "high", "--prompt", files.prompt, "--report", files.report]);
    }
    const successorAccepted = JSON.parse(cli(env, ["round-accept", successorAdmission.round_id, "--holder", "compiler-maintainer"])); assert.equal(successorAccepted.node_id, "CCA0");
    git(env.repo, "merge", "--ff-only", successor.ref);
    const successorLanding = JSON.parse(cli(env, ["landing-record", "--round-id", successorAdmission.round_id, "--holder", "compiler-maintainer"])); assert.equal(successorLanding.node_id, "CCA0");
    assert.equal(successorLanding.canonical_sha, successorAccepted.candidate_sha, "successor landing must bind the exact fast-forwarded candidate");
    assert.match(cli(env, ["phase"]), /phase=ACTIVE/, "ACTIVE must survive canonical successor advance");
    assert.match(cli(env, ["frontier"]), /(?:^|\n)CCA1(?:\n|$)/, "landed CCA0 acceptance must unlock its descendant");

    const authority = lib.loadAuthority(env.packageRoot);
    const anchorFile = path.join(lib.trustedLocalControlRoot(authority), "anchor.json");
    const acceptanceFile = path.join(env.runtimeRoot, "trusted-local", "acceptances", `${successorAdmission.round_id}.json`);
    const landingFile = path.join(env.runtimeRoot, "trusted-local", "landings", `${successorAdmission.round_id}.json`);
    const exactAnchor = fs.readFileSync(anchorFile); const exactAcceptance = fs.readFileSync(acceptanceFile); const exactLanding = fs.readFileSync(landingFile);
    const claimCases = [
      { name: "no anchor claim ignores orphan artifacts", anchor: (row) => { row.rounds[successorAdmission.round_id].status = "FIX_REQUIRED"; delete row.landings[successorAdmission.round_id]; }, acceptance: "exact", landing: "exact", error: null, ready: false },
      { name: "accepted claim without landing is valid but not projected", anchor: (row) => { delete row.landings[successorAdmission.round_id]; }, acceptance: "exact", landing: "exact", error: null, ready: false },
      { name: "landing claim without accepted claim fails closed", anchor: (row) => { row.rounds[successorAdmission.round_id].status = "FIX_REQUIRED"; }, acceptance: "exact", landing: "exact", error: /landing claim .* no ACCEPTED round claim/, ready: false },
      { name: "exact accepted and landing claims project", anchor: () => {}, acceptance: "exact", landing: "exact", error: null, ready: true },
      { name: "missing acceptance artifact fails closed", anchor: () => {}, acceptance: "missing", landing: "exact", error: /accepted claim .* artifact is missing/, ready: false },
      { name: "malformed acceptance artifact fails closed", anchor: () => {}, acceptance: "malformed", landing: "exact", error: /accepted claim .* artifact is malformed/, ready: false },
      { name: "mismatched acceptance artifact fails closed", anchor: () => {}, acceptance: "mismatch", landing: "exact", error: /accepted claim .* artifact mismatches/, ready: false },
      { name: "missing landing artifact fails closed", anchor: () => {}, acceptance: "exact", landing: "missing", error: /landing claim .* artifact is missing/, ready: false },
      { name: "malformed landing artifact fails closed", anchor: () => {}, acceptance: "exact", landing: "malformed", error: /landing claim .* artifact is malformed/, ready: false },
      { name: "mismatched landing artifact fails closed", anchor: () => {}, acceptance: "exact", landing: "mismatch", error: /landing claim .* artifact mismatches/, ready: false },
    ];
    const installArtifact = (file, exact, mode, field) => {
      if (mode === "missing") fs.rmSync(file, { force: true });
      else if (mode === "malformed") fs.writeFileSync(file, "{\n");
      else if (mode === "mismatch") { const row = JSON.parse(exact); row[field] = "mismatched-operator"; fs.writeFileSync(file, `${JSON.stringify(row, null, 2)}\n`); }
      else fs.writeFileSync(file, exact);
    };
    for (const claimCase of claimCases) {
      const anchor = JSON.parse(exactAnchor); claimCase.anchor(anchor);
      fs.writeFileSync(anchorFile, `${JSON.stringify(anchor, null, 2)}\n`);
      installArtifact(acceptanceFile, exactAcceptance, claimCase.acceptance, "accepted_by");
      installArtifact(landingFile, exactLanding, claimCase.landing, "landed_by");
      const claimState = lib.deriveState(authority, { runtimeRoot: env.runtimeRoot });
      const custodyErrors = claimState.errors.filter((error) => /trusted-local (?:accepted|landing) claim/.test(error));
      if (claimCase.error) assert.match(custodyErrors.join("\n"), claimCase.error, claimCase.name);
      else assert.deepEqual(custodyErrors, [], claimCase.name);
      assert.equal(claimState.states.get("CCA1").status === "READY", claimCase.ready, claimCase.name);
      if (claimCase.name === "mismatched acceptance artifact fails closed") {
        assert.throws(() => cli(env, ["admit", "CCA0", "--holder", "other-maintainer", "--candidate-ref", successor.ref]), /state invalid.*accepted claim/is, "invalid claimed custody must prevent re-admission");
      }
    }
    fs.writeFileSync(anchorFile, exactAnchor); fs.writeFileSync(acceptanceFile, exactAcceptance); fs.writeFileSync(landingFile, exactLanding);
    git(env.repo, "reset", "--hard", landing.canonical_sha);
    const rewritten = lib.deriveState(authority, { runtimeRoot: env.runtimeRoot });
    assert.match(rewritten.errors.join("\n"), /trusted-local landing .* not .* retained by the current canonical history/);
    assert.notEqual(rewritten.states.get("CCA1").status, "READY", "a rewritten canonical branch must not retain descendant readiness from the removed CCA0 landing");
    git(env.repo, "worktree", "remove", successor.worktree);
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
});
