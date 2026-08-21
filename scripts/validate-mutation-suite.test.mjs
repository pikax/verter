// Mutation suite for scripts/validate-program-state.mjs,
// scripts/validate-stack-window.mjs, and scripts/lib/stack-window-lib.mjs
// (the shared model both validators consume).
//
//   node --test scripts/validate-mutation-suite.test.mjs
//
// This is NOT another hand-picked set of "mutate field X, expect violation
// Y" cases layered on top of the existing validate-program-state.test.mjs /
// validate-stack-window.test.mjs suites. Those prove specific scenarios;
// this suite additionally proves COMPLETENESS: every violation-emitting
// call site the validators actually contain (scripts/lib/check-inventory.mjs
// derives that list FROM THE SOURCE, every run — see that file's header)
// must be tripped by at least one mutation here. The final test in this
// file asserts the derived inventory has zero uncovered checks; add a new
// v(...)/push(...) call site to a validator with no mutation proven against
// it, and that test fails, naming the check by file/line/text. Do not
// "simplify" that final assertion into a fixed expected count — a literal
// count is exactly the hand-maintained parallel list this suite exists to
// avoid; it must be able to grow (or shrink) with the source it audits.
//
// Standards followed throughout (see each test):
//   - every mutation is proven to actually apply (the mutated fixture text
//     is asserted to differ from its unmutated base before being run);
//   - every assertion targets the SPECIFIC derived check via
//     CheckRegistry#find (an anchor ambiguous or absent against the current
//     source throws immediately), not just "exit code is non-zero";
//   - git-identity fixtures run against a REAL temporary repository, same
//     discipline as validate-program-state.test.mjs.
//
// One acknowledged, explicitly-named gap: scripts/validate-program-state.mjs
// checks the paired `${shaField}^{tree}` derivation of an already-confirmed
// commit resolves (verifyLiveGitIdentities, "could not be checked — git
// could not resolve ... ^{tree}"). Every real git commit has a resolvable
// tree by construction — reaching that branch requires corrupting the
// object store so a commit resolves as an object but its referenced tree
// does not (deleting the tree's loose object file after the commit is
// built), which is invasive enough to risk being flaky across git storage
// strategies (loose vs. auto-packed) rather than a meaningful behavioral
// mutation of validator INPUT. It is not covered here; see the final
// coverage test for how this is recorded as a NAMED, bounded exception
// rather than a silent gap.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { CheckRegistry } from "./lib/check-inventory.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const PS_FILE = join(HERE, "validate-program-state.mjs");
const SW_FILE = join(HERE, "validate-stack-window.mjs");
const SWL_FILE = join(HERE, "lib", "stack-window-lib.mjs");

// The one named, bounded exception — see the file header. Matched by anchor
// (the SAME lookup CheckRegistry#find uses), so if the source text it names
// ever changes or disappears, `find` throws loudly instead of this
// exemption silently covering a DIFFERENT check.
const ACKNOWLEDGED_GAPS = [
  { file: PS_FILE, anchor: "could not be checked — git could not resolve" },
];

const registry = new CheckRegistry([PS_FILE, SW_FILE, SWL_FILE]);
for (const gap of ACKNOWLEDGED_GAPS) {
  registry.markCovered(registry.find(gap.file, gap.anchor));
}

// Assert `result` (a spawnSync-shaped {status, out, err}) is a failure whose
// stderr matches the ONE derived check named by (file, anchor), and mark
// that check covered. Throws at test-definition-adjacent time (via
// registry.find) if the anchor no longer resolves uniquely — a rename or a
// removed check breaks this loudly, not silently.
function expectCheck(file, anchor, result) {
  const check = registry.find(file, anchor);
  assert.notEqual(
    result.status,
    0,
    `expected a violation for anchor ${JSON.stringify(anchor)}, got exit 0:\n${result.out}`,
  );
  assert.match(
    result.err,
    check.regex,
    `stderr did not match the derived check for anchor ${JSON.stringify(anchor)} (${check.file}:${check.line}):\n${result.err}`,
  );
  registry.markCovered(check);
}

// Prove a mutation actually changed the fixture text before running it —
// "a mutation must be proven to APPLY" (see the maintainer directive quoted
// in the module header). Every mutated-fixture builder below routes through
// this.
function applied(base, mutated, label) {
  assert.notEqual(
    mutated,
    base,
    `mutation ${label} did not change the fixture text — vacuous mutation`,
  );
  return mutated;
}

let dir;
let gitRoot; // SHA_BASE -> SHA (tip); SHA_DANGLING off SHA_BASE, unreachable from tip
let emptyGitRepo; // git init, zero commits — HEAD does not resolve
let notGitDir; // plain directory, never git-init'ed
let fakeGitDir; // wraps `git`, can break one named subcommand on demand
let realGitPath;
let amendmentsDir;

let SHA_BASE, TREE_BASE;
let SHA, TREE;
let SHA_DANGLING;
// Concurrent-implementation disjointness fixtures (all branched off SHA_BASE,
// same convention as SHA_DANGLING): CONCURRENT_A/CONCURRENT_B each add their
// OWN distinct file — a real, git-merge-tree-clean pair. CONCURRENT_CONFLICT_A
// / CONCURRENT_CONFLICT_B both rewrite the SAME line of the SAME pre-existing
// file to different content — a real git merge-tree content conflict.
let CONCURRENT_A, CONCURRENT_B, CONCURRENT_C;
let CONCURRENT_A_TREE, CONCURRENT_B_TREE, CONCURRENT_C_TREE;
let CONCURRENT_CONFLICT_A, CONCURRENT_CONFLICT_B;
// Branch names each CONCURRENT_* sha resolves from — kept live in before()
// (never deleted) so they double as implementation_ref resolution targets.
const CONCURRENT_A_REF = "concurrent-a";
const CONCURRENT_B_REF = "concurrent-b";
const CONCURRENT_C_REF = "concurrent-c";
const CONCURRENT_CONFLICT_A_REF = "concurrent-conflict-a";
const CONCURRENT_CONFLICT_B_REF = "concurrent-conflict-b";
// A TAG (not a branch) at CONCURRENT_A — round 4, FIX 1 discriminator: a
// real, resolvable, tip-matching ref that is NOT under refs/heads/.
const CONCURRENT_A_TAG = "concurrent-a-tag";
const SHA_NONEXISTENT = "abcdef1234567890abcdef1234567890abcdef12"; // well-formed, never committed

function git(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

before(() => {
  dir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-"));
  notGitDir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-nogit-"));

  gitRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-git-"));
  git(["init", "-q"], gitRoot);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], gitRoot);
  git(["config", "user.email", "test@example.invalid"], gitRoot);
  git(["config", "user.name", "Test"], gitRoot);
  git(["config", "commit.gpgsign", "false"], gitRoot);

  writeFileSync(join(gitRoot, "base.txt"), "base\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "base"], gitRoot);
  SHA_BASE = git(["rev-parse", "HEAD"], gitRoot);
  TREE_BASE = git(["rev-parse", "HEAD^{tree}"], gitRoot);

  writeFileSync(join(gitRoot, "tip.txt"), "tip\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "tip"], gitRoot);
  SHA = git(["rev-parse", "HEAD"], gitRoot);
  TREE = git(["rev-parse", "HEAD^{tree}"], gitRoot);

  git(["checkout", "-q", "-b", "scratch", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "scratch.txt"), "scratch\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "scratch"], gitRoot);
  SHA_DANGLING = git(["rev-parse", "HEAD"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);
  git(["branch", "-D", "scratch"], gitRoot);

  // These five branches are kept LIVE (never deleted) — AMD-013 FIX 2
  // requires implementation_candidate_sha to be bound to a resolvable
  // implementation_ref whose live tip equals the pin exactly, so every
  // fixture that rehearses an IN_PROGRESS block against one of these SHAs
  // needs a real ref pointing at it, unlike the deleted-branch convention
  // SHA_DANGLING uses (that fixture's whole point is to be unreachable).
  git(["checkout", "-q", "-b", "concurrent-a", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "concurrent-a.txt"), "a\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "concurrent-a"], gitRoot);
  CONCURRENT_A = git(["rev-parse", "HEAD"], gitRoot);
  CONCURRENT_A_TREE = git(["rev-parse", "HEAD^{tree}"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);

  git(["checkout", "-q", "-b", "concurrent-b", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "concurrent-b.txt"), "b\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "concurrent-b"], gitRoot);
  CONCURRENT_B = git(["rev-parse", "HEAD"], gitRoot);
  CONCURRENT_B_TREE = git(["rev-parse", "HEAD^{tree}"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);

  git(["checkout", "-q", "-b", "concurrent-c", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "concurrent-c.txt"), "c\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "concurrent-c"], gitRoot);
  CONCURRENT_C = git(["rev-parse", "HEAD"], gitRoot);
  CONCURRENT_C_TREE = git(["rev-parse", "HEAD^{tree}"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);

  git(["checkout", "-q", "-b", "concurrent-conflict-a", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "base.txt"), "conflict-a\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "concurrent-conflict-a"], gitRoot);
  CONCURRENT_CONFLICT_A = git(["rev-parse", "HEAD"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);

  git(["checkout", "-q", "-b", "concurrent-conflict-b", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "base.txt"), "conflict-b\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "concurrent-conflict-b"], gitRoot);
  CONCURRENT_CONFLICT_B = git(["rev-parse", "HEAD"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);

  // A real, resolvable TAG (not a branch) at CONCURRENT_A — round 4, FIX 1's
  // "confirm it is a real ref (refs/heads/...)" requirement: this ref
  // resolves cleanly via `git rev-parse --verify` and its live tip genuinely
  // equals CONCURRENT_A, but it is a tag, not a branch, so implementation_ref
  // must still reject it.
  git(["tag", CONCURRENT_A_TAG, CONCURRENT_A], gitRoot);

  emptyGitRepo = mkdtempSync(join(tmpdir(), "validate-mutation-suite-empty-git-"));
  git(["init", "-q"], emptyGitRepo);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], emptyGitRepo);

  realGitPath = execFileSync("which", ["git"], { encoding: "utf8" }).trim();
  fakeGitDir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-fakegit-"));
  const fakeGitScript = join(fakeGitDir, "git");
  // Deliberately CommonJS (no package.json here to declare "type": "module")
  // so the shebang-executed script needs no module-resolution setup.
  writeFileSync(
    fakeGitScript,
    `#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const args = process.argv.slice(2);
if (process.env.FAKE_GIT_BREAK && args[0] === process.env.FAKE_GIT_BREAK) {
  process.stderr.write("fake-git: simulated failure of " + args[0] + "\\n");
  process.exit(17);
}
const res = spawnSync(process.env.FAKE_GIT_REAL, args, { stdio: "inherit" });
process.exit(res.status === null ? 1 : res.status);
`,
    "utf8",
  );
  chmodSync(fakeGitScript, 0o755);

  amendmentsDir = join(dir, "amendments");
  mkdirSync(amendmentsDir, { recursive: true });
  writeFileSync(
    join(amendmentsDir, "AMD-900-not-ratified-fixture.md"),
    "# AMD-900 — test fixture\n\n**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.\n",
  );
});

after(() => {
  rmSync(dir, { recursive: true, force: true });
  rmSync(gitRoot, { recursive: true, force: true });
  rmSync(emptyGitRepo, { recursive: true, force: true });
  rmSync(notGitDir, { recursive: true, force: true });
  rmSync(fakeGitDir, { recursive: true, force: true });
});

function write(name, content) {
  const p = join(dir, name);
  writeFileSync(p, content, "utf8");
  return p;
}

function fakeGitEnv(breakCmd) {
  return {
    ...process.env,
    PATH: `${fakeGitDir}:${process.env.PATH}`,
    FAKE_GIT_REAL: realGitPath,
    FAKE_GIT_BREAK: breakCmd,
  };
}

// --authority is now mandatory-by-default in live mode (see the "block
// authorization registry" tests below, which exercise it directly and pass
// their own --authority/--no-authority). Every other call site here predates
// that check and never sets up an authority-registry.toml fixture, so it
// opts out via the explicit --no-authority escape unless the caller already
// named one of the two flags itself. Tests that specifically exercise the
// mandatory-by-default resolution (neither flag given) pass this marker
// instead — stripped before the real invocation — so they are not silently
// defeated by the same auto-injection.
const TEST_DEFAULT_AUTHORITY_MARKER = "--test-default-authority-behavior";
function runPS(dagPath, statePath, mode, extraArgs = [], { cwd = gitRoot, env } = {}) {
  const opts = { encoding: "utf8", cwd };
  if (env) opts.env = env;
  const testingDefaultAuthority = extraArgs.includes(TEST_DEFAULT_AUTHORITY_MARKER);
  const cleaned = extraArgs.filter((a) => a !== TEST_DEFAULT_AUTHORITY_MARKER);
  const args =
    testingDefaultAuthority || cleaned.includes("--authority") || cleaned.includes("--no-authority")
      ? cleaned
      : [...cleaned, "--no-authority"];
  const res = spawnSync(
    process.execPath,
    [PS_FILE, "--dag", dagPath, "--state", statePath, "--mode", mode, ...args],
    opts,
  );
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "" };
}

function runSW(args, { cwd, env } = {}) {
  const opts = { encoding: "utf8" };
  if (cwd) opts.cwd = cwd;
  if (env) opts.env = env;
  const res = spawnSync(process.execPath, [SW_FILE, ...args], opts);
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "" };
}

// -- DAG fixture builders

function dagBlock({ id, name = id, class: cls = "foundational", predecessors, conditional }) {
  const predLine =
    predecessors === undefined
      ? ""
      : `predecessors = [${predecessors.map((p) => `"${p}"`).join(", ")}]\n`;
  const condLine = conditional
    ? `conditional_predecessor_if_opened = [${conditional.map((p) => `"${p}"`).join(", ")}]\n`
    : "";
  return `[[block]]\nid = "${id}"\nname = "${name}"\nclass = "${cls}"\n${predLine}${condLine}`;
}
function dagText(blocks) {
  return `schema = 1\nrevision = 11\nentry_gate = "X"\nfinal_gate = "X"\n\n` + blocks.join("\n");
}

const DAG1 = dagText([dagBlock({ id: "A0", predecessors: [] })]);
const DAG1_DIGEST = createHash("sha256").update(DAG1).digest("hex");

const DAG3 = dagText([
  dagBlock({ id: "A0", predecessors: [] }),
  dagBlock({ id: "A1", predecessors: ["A0"] }),
  dagBlock({ id: "A2", predecessors: ["A0", "A1"] }),
]);
const DAG3_DIGEST = createHash("sha256").update(DAG3).digest("hex");

const DAG3_CP = dagText([
  dagBlock({ id: "A0", predecessors: [] }),
  dagBlock({ id: "A1", predecessors: ["A0"], class: "foundational-private-checkpoint" }),
  dagBlock({ id: "A2", predecessors: ["A0", "A1"] }),
]);
const DAG3_CP_DIGEST = createHash("sha256").update(DAG3_CP).digest("hex");

const DAG3_SUB = dagText([
  dagBlock({ id: "A0", predecessors: [] }),
  dagBlock({ id: "A1", predecessors: ["A0"], class: "subsystem" }),
  dagBlock({ id: "A2", predecessors: ["A0", "A1"] }),
]);
const DAG3_SUB_DIGEST = createHash("sha256").update(DAG3_SUB).digest("hex");

// -- State fixture builders (mirrors validate-program-state.test.mjs)

function block(id, status, overrides = {}) {
  const fields = {
    charter_digest: "",
    context_packet_digest: "",
    base_sha: "",
    candidate_sha: "",
    implementation_candidate_sha: "",
    implementation_ref: "",
    candidate_tree: "",
    accepted_sha: "",
    accepted_tree: "",
    landing_equivalence_digest: "",
    evidence_digest: "",
    entry_lock_digest: "",
    stack_id: "",
    stack_snapshot_digest: "",
    stack_layer: 0,
    landing_order: 0,
    conformance_review: "PENDING",
    conformance_reviewed_sha: "",
    architecture_review: "PENDING",
    architecture_reviewed_sha: "",
    adversarial_review: "PENDING",
    adversarial_reviewed_sha: "",
    maintainer_decision: "PENDING",
    notes: "",
    ...overrides,
  };
  const lines = Object.entries(fields).map(([k, val]) =>
    typeof val === "number" ? `${k} = ${val}` : `${k} = "${val}"`,
  );
  return `[[block]]\nid = "${id}"\nstatus = "${status}"\n${lines.join("\n")}\n`;
}

const DIGEST = createHash("sha256").update("mutation-suite-digest").digest("hex");
const DIGEST2 = createHash("sha256").update("mutation-suite-digest-2").digest("hex");

function acceptedBlock(id, overrides = {}) {
  return block(id, "ACCEPTED", {
    entry_lock_digest: DIGEST,
    charter_digest: DIGEST,
    context_packet_digest: DIGEST,
    base_sha: SHA,
    candidate_sha: SHA,
    candidate_tree: TREE,
    accepted_sha: SHA,
    accepted_tree: TREE,
    landing_equivalence_digest: DIGEST,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    conformance_reviewed_sha: SHA,
    architecture_review: "PASS",
    architecture_reviewed_sha: SHA,
    adversarial_review: "PASS",
    adversarial_reviewed_sha: SHA,
    maintainer_decision: "ACCEPTED",
    ...overrides,
  });
}

function checkpointBlock(id, overrides = {}) {
  return block(id, "PRIVATE_CHECKPOINT", {
    charter_digest: DIGEST,
    context_packet_digest: DIGEST,
    base_sha: SHA,
    candidate_sha: SHA,
    candidate_tree: TREE,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    conformance_reviewed_sha: SHA,
    architecture_review: "PASS",
    architecture_reviewed_sha: SHA,
    adversarial_review: "PASS",
    adversarial_reviewed_sha: SHA,
    ...overrides,
  });
}

function header({
  status = "ACTIVE",
  current,
  repoSha = SHA,
  dagDigest = DAG1_DIGEST,
  extraTop = "",
  orchestration = "",
  integrationBranch = "main",
  integrationHeadSha = repoSha,
}) {
  return `schema = 1
revision = 11
status = "${status}"
authority_package_digest = ""
release_report_digest = ""
program_dag_digest = "${dagDigest}"
entry_checkout_sha = "${repoSha}"
entry_checkout_tree = "${repoSha}"
implementation_baseline_sha = ""
implementation_baseline_tree = ""
implementation_lock_digest = ""
performance_gates_digest = ""
architecture_premise_ledger_digest = ""
current_block = "${current}"
${extraTop}
[repository]
remote = "https://example.invalid/repo"
branch = "main"
head_sha = "${repoSha}"
head_tree = "${repoSha}"
dirty = false
untracked_count = 0
integration_branch = "${integrationBranch}"
integration_head_sha = "${integrationHeadSha}"
${orchestration}
`;
}

// =====================================================================
// PROGRAM-STATE — live git identity (verifyLiveGitIdentities)
// =====================================================================

test("[PS] no git repository", () => {
  const dag = write("dag-git1.toml", DAG1);
  const state = write("state-git1.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { cwd: notGitDir });
  expectCheck(PS_FILE, "requires a git repository to verify", r);
});

test("[PS] batch-check subprocess failure", () => {
  const dag = write("dag-git2.toml", DAG1);
  const state = write(
    "state-git2.toml",
    header({ current: "A0" }) + block("A0", "LOCKED", { base_sha: SHA }),
  );
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("cat-file") });
  expectCheck(PS_FILE, "batch-check failed while verifying identity fields", r);
});

test("[PS] SHA does not resolve to a real commit", () => {
  const dag = write("dag-git3.toml", DAG1);
  const state = write(
    "state-git3.toml",
    header({ current: "A0" }) + block("A0", "LOCKED", { base_sha: SHA_NONEXISTENT }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "does not resolve to an existing git commit object", r);
});

test("[PS] tree field cannot be verified — paired sha field malformed", () => {
  const dag = write("dag-git4.toml", DAG1);
  const state = write(
    "state-git4.toml",
    header({ current: "A0" }) + block("A0", "LOCKED", { candidate_tree: TREE, candidate_sha: "" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "cannot be verified — its paired", r);
});

test("[PS] tree field is not the tree of its paired sha", () => {
  const dag = write("dag-git5.toml", DAG1);
  const state = write(
    "state-git5.toml",
    header({ current: "A0" }) +
      block("A0", "LOCKED", { candidate_sha: SHA, candidate_tree: TREE_BASE }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not the tree of", r);
});

test("[PS] configured trunk ref cannot be resolved (zero commits) — verifyLiveGitIdentities cannot verify reachability against an unresolved pin", () => {
  // repository.branch = "main" (the header() default) does not exist as a
  // real ref in a zero-commit repo, so resolvePinnedTrunk itself fails first
  // (its own violation, not asserted here); pinnedTrunk is then null when
  // passed into verifyLiveGitIdentities (round 4, FIX 3), which must record
  // its OWN distinct violation rather than silently skip reachability.
  const dag = write("dag-git6.toml", DAG1);
  const state = write("state-git6.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { cwd: emptyGitRepo });
  expectCheck(PS_FILE, "reachability cannot be checked against an untrustworthy trunk pin", r);
});

test("[PS] rev-list subprocess failure", () => {
  const dag = write("dag-git7.toml", DAG1);
  const state = write("state-git7.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("rev-list") });
  expectCheck(
    PS_FILE,
    "could not enumerate commits reachable from the configured trunk ref's live tip",
    r,
  );
});

test("[PS] ACCEPTED with a dangling (unreachable) accepted_sha", () => {
  const dag = write("dag-git8.toml", DAG1);
  const state = write(
    "state-git8.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", { accepted_sha: SHA_DANGLING, landing_equivalence_digest: DIGEST }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not genuinely landed", r);
});

test("[PS] resolvePinnedTrunk merge-base --is-ancestor subprocess failure", () => {
  // AMD-013 round 6: resolvePinnedTrunk now itself shells out to `git
  // merge-base --is-ancestor` (the pin-vs-live-tip ancestry check) — this
  // call runs UNCONDITIONALLY, before checks 3 & 4's own base_sha/
  // accepted_sha ancestry check ever gets a chance to run, so a blanket
  // "merge-base" break now surfaces THIS failure first (see the next test
  // for checks 3 & 4's own merge-base failure, isolated with a targeted
  // shim that lets this call through).
  const dag = write("dag-git9.toml", DAG1);
  const state = write("state-git9.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("merge-base") });
  expectCheck(PS_FILE, "could not check whether the pinned trunk", r);
});

test("[PS] base_sha/accepted_sha ancestry merge-base subprocess failure (isolated from resolvePinnedTrunk's own merge-base call)", () => {
  // A blanket "merge-base" break (fakeGitEnv) now always hits
  // resolvePinnedTrunk's own ancestor check first (the test above) — this
  // targeted shim instead fails merge-base ONLY when invoked with SHA_BASE
  // (checks 3 & 4's base_sha ancestry call), letting resolvePinnedTrunk's
  // own call (against the default header's SHA/SHA pin/live-tip pair, which
  // never mentions SHA_BASE) pass through to real git untouched.
  const shimDir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-mergebase-targeted-"));
  const shimScript = join(shimDir, "git");
  writeFileSync(
    shimScript,
    `#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const args = process.argv.slice(2);
if (args[0] === "merge-base" && args.includes("${SHA_BASE}")) {
  process.stderr.write("shim: simulated merge-base failure for base_sha ancestry check\\n");
  process.exit(17);
}
const res = spawnSync(process.env.FAKE_GIT_REAL, args, { stdio: "inherit" });
process.exit(res.status === null ? 1 : res.status);
`,
    "utf8",
  );
  chmodSync(shimScript, 0o755);

  const dag = write("dag-git9b.toml", DAG1);
  const state = write(
    "state-git9b.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", {
        base_sha: SHA_BASE,
        accepted_sha: SHA,
        landing_equivalence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", [], {
    env: { ...process.env, PATH: `${shimDir}:${process.env.PATH}`, FAKE_GIT_REAL: realGitPath },
  });
  expectCheck(PS_FILE, "ancestry against accepted_sha", r);
  rmSync(shimDir, { recursive: true, force: true });
});

test("[PS] base_sha is not an ancestor of accepted_sha", () => {
  const dag = write("dag-git10.toml", DAG1);
  const state = write(
    "state-git10.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", {
        base_sha: SHA,
        accepted_sha: SHA_BASE,
        landing_equivalence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not an ancestor of accepted_sha", r);
});

// =====================================================================
// PROGRAM-STATE — header / DAG structure
// =====================================================================

test("[PS] state missing a required top-level key", () => {
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(base, base.replace('status = "ACTIVE"\n', ""), "drop status key");
  const dag = write("dag-hdr1.toml", DAG1);
  const state = write("state-hdr1.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is missing required top-level key", r);
});

test("[PS] state schema/revision mismatched against DAG", () => {
  const dag = write(
    "dag-hdr2.toml",
    dagText([dagBlock({ id: "A0", predecessors: [] })]).replace("revision = 11", "revision = 999"),
  );
  const dagDigest = createHash("sha256").update(readFileSync(dag)).digest("hex");
  const state = write(
    "state-hdr2.toml",
    header({ current: "A0", dagDigest }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "does not match DAG", r);
});

test("[PS] live top-level status is not ACTIVE", () => {
  const dag = write("dag-hdr3.toml", DAG1);
  const state = write(
    "state-hdr3.toml",
    header({ current: "A0", status: "PAUSED" }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "top-level status is", r);
});

test("[PS] empty program_dag_digest in live mode", () => {
  const dag = write("dag-hdr4.toml", DAG1);
  const state = write(
    "state-hdr4.toml",
    header({ current: "A0", dagDigest: "" }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "silently disables the ledger-to-DAG binding", r);
});

test("[PS] program_dag_digest mismatched against the real DAG file hash", () => {
  const dag = write("dag-hdr5.toml", DAG1);
  const state = write(
    "state-hdr5.toml",
    header({ current: "A0", dagDigest: DIGEST2 }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "does not match the SHA-256 of the DAG file", r);
});

test("[PS] DAG duplicate block id", () => {
  const dag = write(
    "dag-hdr6.toml",
    dagText([dagBlock({ id: "A0", predecessors: [] }), dagBlock({ id: "A0", predecessors: [] })]),
  );
  const state = write(
    "state-hdr6.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "DAG declares duplicate block id", r);
});

test("[PS] DAG block has no predecessors array", () => {
  const dag = write("dag-hdr7.toml", dagText([dagBlock({ id: "A0", predecessors: undefined })]));
  assert.ok(
    !readFileSync(dag, "utf8").includes("predecessors"),
    "predecessors line must be absent",
  );
  const state = write(
    "state-hdr7.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "has no predecessors array", r);
});

test("[PS] DAG names an unknown predecessor", () => {
  const dag = write("dag-hdr8.toml", dagText([dagBlock({ id: "A0", predecessors: ["ZZ"] })]));
  const state = write(
    "state-hdr8.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "unknown predecessor ${JSON.stringify(p)}", r);
});

test("[PS] DAG names an unknown conditional predecessor", () => {
  const dag = write(
    "dag-hdr9.toml",
    dagText([dagBlock({ id: "A0", predecessors: [], conditional: ["ZZ"] })]),
  );
  const state = write(
    "state-hdr9.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "unknown conditional predecessor ${JSON.stringify(p)}", r);
});

test("[PS] DAG predecessor cycle + unreachable block (self-loop)", () => {
  const dag = write(
    "dag-hdr10.toml",
    dagText([
      dagBlock({ id: "A0", predecessors: [] }),
      dagBlock({ id: "B0", predecessors: ["A0"] }),
      dagBlock({ id: "C0", predecessors: ["C0"] }),
    ]),
  );
  const state = write(
    "state-hdr10.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) +
      block("A0", "LOCKED") +
      "\n" +
      block("B0", "LOCKED") +
      "\n" +
      block("C0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "predecessor cycle through", r);
  expectCheck(PS_FILE, "is not reachable from root", r);
});

test("[PS] DAG has more than one root", () => {
  const dag = write(
    "dag-hdr11.toml",
    dagText([dagBlock({ id: "A0", predecessors: [] }), dagBlock({ id: "B0", predecessors: [] })]),
  );
  const state = write(
    "state-hdr11.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) +
      block("A0", "LOCKED") +
      "\n" +
      block("B0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "must have exactly one root block", r);
});

test("[PS] state declares duplicate block id", () => {
  const dag = write("dag-hdr12.toml", DAG1);
  const state = write(
    "state-hdr12.toml",
    header({ current: "A0" }) + block("A0", "LOCKED") + "\n" + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "state declares duplicate block id", r);
});

test("[PS] state block set does not equal DAG block set", () => {
  const dag = write("dag-hdr13.toml", DAG1);
  const state = write("state-hdr13.toml", header({ current: "A0" }) + block("Z9", "LOCKED"));
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "state block set does not equal DAG block set", r);
});

// =====================================================================
// PROGRAM-STATE — per-block status / review enums
// =====================================================================

test("[PS] state block has no status", () => {
  const base = block("A0", "LOCKED");
  const mutated = applied(base, `[[block]]\nid = "A0"\n`, "strip status + all fields");
  const dag = write("dag-status1.toml", DAG1);
  const state = write("state-status1.toml", header({ current: "A0" }) + mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "has no status", r);
});

test("[PS] state block status outside the declared enum", () => {
  const dag = write("dag-status2.toml", DAG1);
  const state = write(
    "state-status2.toml",
    header({ current: "A0" }) + block("A0", "BOGUS_STATUS"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "outside the declared enum", r);
});

test("[PS] state block review field outside the declared review enum", () => {
  const dag = write("dag-status3.toml", DAG1);
  const state = write(
    "state-status3.toml",
    header({ current: "A0" }) + block("A0", "LOCKED", { conformance_review: "BOGUS_VERDICT" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "outside the declared review enum", r);
});

// =====================================================================
// PROGRAM-STATE — review verdict identity binding
// =====================================================================

test("[PS] PASS mandate with no reviewed sha bound", () => {
  const dag = write("dag-verdict1.toml", DAG1);
  const state = write(
    "state-verdict1.toml",
    header({ current: "A0" }) +
      block("A0", "LOCKED", { conformance_review: "PASS", conformance_reviewed_sha: "" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "must bind the exact candidate it was issued against", r);
});

test("[PS] PASS mandate whose candidate_sha is not well-formed", () => {
  const dag = write("dag-verdict2.toml", DAG1);
  const state = write(
    "state-verdict2.toml",
    header({ current: "A0" }) +
      block("A0", "LOCKED", {
        conformance_review: "PASS",
        conformance_reviewed_sha: SHA,
        candidate_sha: "",
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "cannot verify the verdict is bound to the current candidate", r);
});

test("[PS] PASS mandate reviewed against a stale candidate", () => {
  const dag = write("dag-verdict3.toml", DAG1);
  const state = write(
    "state-verdict3.toml",
    header({ current: "A0" }) +
      block("A0", "LOCKED", {
        conformance_review: "PASS",
        conformance_reviewed_sha: SHA,
        candidate_sha: SHA_BASE,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the verdict was issued against a different candidate and is stale", r);
});

test("[PS] non-PASS mandate carrying a reviewed sha", () => {
  const dag = write("dag-verdict4.toml", DAG1);
  const state = write(
    "state-verdict4.toml",
    header({ current: "A0" }) +
      block("A0", "LOCKED", { conformance_review: "PENDING", conformance_reviewed_sha: SHA }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "a non-PASS mandate must not carry a reviewed candidate SHA", r);
});

// =====================================================================
// PROGRAM-STATE — sequencing invariant
// =====================================================================

test("[PS] PRIVATE_CHECKPOINT predecessor with no --stack-window (fail closed)", () => {
  const dag = write("dag-seq1.toml", DAG3_CP);
  const state = write(
    "state-seq1.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1") +
      "\n" +
      block("A2", "REVIEW", {
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "which this validator does not model — fail closed", r);
});

test("[PS] opened conditional predecessor not accepted", () => {
  const dag = write(
    "dag-seq2.toml",
    dagText([
      dagBlock({ id: "A0", predecessors: [] }),
      dagBlock({ id: "A1", predecessors: ["A0"], conditional: ["A2"] }),
      dagBlock({ id: "A2", predecessors: ["A0"] }),
    ]),
  );
  const state = write(
    "state-seq2.toml",
    header({
      current: "A1",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "READY"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "this path is not modelled beyond LOCKED/ACCEPTED", r);
});

test("[PS] stacked-work exception: bare stack_id with no established stack", () => {
  const dag = write("dag-seq3.toml", DAG3);
  const state = write(
    "state-seq3.toml",
    header({ current: "A0", dagDigest: DAG3_DIGEST }) +
      block("A0", "READY") +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "so no validated immutable stack snapshot is bound", r);
  expectCheck(PS_FILE, "does not carry the same non-empty stack_id", r);
  expectCheck(PS_FILE, "and the contingent stacked-work exception is REJECTED —", r);
});

test("[PS] stack_layer is not an integer", () => {
  const dag = write("dag-seq4.toml", DAG3);
  const state = write(
    "state-seq4.toml",
    header({ current: "A0", dagDigest: DAG3_DIGEST }) +
      block("A0", "IN_PROGRESS", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 0,
      }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: "not-a-number", // the SUCCESSOR's own stack_layer is what L713 checks
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not an integer`", r); // literal backtick anchors the exact call site
});

test("[PS] stacked-work exception: mismatched snapshot digest", () => {
  const dag = write("dag-seq5.toml", DAG3);
  const state = write(
    "state-seq5.toml",
    header({ current: "A0", dagDigest: DAG3_DIGEST }) +
      block("A0", "IN_PROGRESS", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST2,
        stack_layer: 0,
      }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not the same well-formed snapshot digest as block", r);
});

test("[PS] stacked-work exception: terminated predecessor cannot be a lower layer", () => {
  const dag = write("dag-seq6.toml", DAG3);
  const state = write(
    "state-seq6.toml",
    header({ current: "A1", dagDigest: DAG3_DIGEST }) +
      block("A0", "ABORTED", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 0 }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "cannot be a lower layer of the same validated stack snapshot", r);
});

test("[PS] stacked-work exception: predecessor stack_layer not below successor's", () => {
  const dag = write("dag-seq7.toml", DAG3);
  const state = write(
    "state-seq7.toml",
    header({ current: "A0", dagDigest: DAG3_DIGEST }) +
      block("A0", "IN_PROGRESS", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
      }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not below block", r);
});

test("[PS] stackless READY with unaccepted predecessor", () => {
  const dag = write("dag-seq8.toml", DAG3);
  const state = write(
    "state-seq8.toml",
    header({ current: "A0", dagDigest: DAG3_DIGEST }) +
      block("A0", "READY") +
      "\n" +
      block("A1", "READY") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "but direct predecessor(s) not ACCEPTED", r);
});

// =====================================================================
// PROGRAM-STATE — status-gated evidence/review/entry-lock obligations
// =====================================================================

test("[PS] ACCEPTANCE_RECOMMENDED with everything unresolved", () => {
  const dag = write("dag-gate1.toml", DAG1);
  const state = write(
    "state-gate1.toml",
    header({ current: "A0" }) + block("A0", "ACCEPTANCE_RECOMMENDED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "but ${field} is not a non-empty 40-char lowercase git object id", r);
  expectCheck(PS_FILE, "but ${field} is not a non-empty 64-char lowercase SHA-256", r);
  expectCheck(PS_FILE, "is the DAG's entry (root) block and its entry-lock record", r);
  expectCheck(PS_FILE, "before acceptance recommendation, acceptance, or a private checkpoint", r);
});

test("[PS] NOT_REQUIRED mandate on a foundational-class block", () => {
  const dag = write("dag-gate2.toml", DAG1);
  const state = write(
    "state-gate2.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", {
        conformance_review: "NOT_REQUIRED",
        conformance_reviewed_sha: "",
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is permitted only for architecture_review on a subsystem-class block", r);
});

test("[PS] PRIVATE_CHECKPOINT on a wrong-class block", () => {
  const dag = write("dag-gate3.toml", DAG1);
  const state = write("state-gate3.toml", header({ current: "A0" }) + checkpointBlock("A0"));
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is permitted only for a block whose DAG class is", r);
});

test("[PS] ACCEPTED without recorded maintainer acceptance", () => {
  const dag = write("dag-gate4.toml", DAG1);
  const state = write(
    "state-gate4.toml",
    header({ current: "A0" }) + acceptedBlock("A0", { maintainer_decision: "PENDING" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "acceptance is maintainer-only", r);
});

test("[PS] ACCEPTED with diverged identity and no landing-equivalence digest", () => {
  const dag = write("dag-gate5.toml", DAG1);
  const state = write(
    "state-gate5.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", {
        accepted_sha: SHA_BASE,
        accepted_tree: TREE_BASE,
        landing_equivalence_digest: "",
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "a differing accepted identity is legal only with a repository-validated landing-equivalence artifact",
    r,
  );
});

// =====================================================================
// PROGRAM-STATE — amendment authority gate
// =====================================================================

test("[PS] enabling_amendment names no real amendment file", () => {
  const dag = write("dag-amd1.toml", DAG1);
  const state = write(
    "state-amd1.toml",
    header({ current: "A0" }) + block("A0", "READY", { notes: "", enabling_amendment: "AMD-999" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "declares enabling_amendment ${JSON.stringify(amdId)} but", r);
});

test("[PS] enabling_amendment is not ratified", () => {
  const dag = write("dag-amd2.toml", DAG1);
  const state = write(
    "state-amd2.toml",
    header({ current: "A0" }) + block("A0", "READY", { enabling_amendment: "AMD-900" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "an unratified enabling amendment has no execution authority", r);
});

// =====================================================================
// PROGRAM-STATE — block authorization registry (--authority)
// =====================================================================

const BASE_TXT_SHA256 = createHash("sha256").update("base\n").digest("hex");
const TIP_TXT_SHA256 = createHash("sha256").update("tip\n").digest("hex");

// A document's declared `kind` must match where it actually lives (see
// validate-program-state.mjs's KIND_DIR / isPathUnder), resolved under the
// SAME directory the DAG file lives in (dirname(--dag) === `dir` for every
// fixture in this suite). Every authDoc() fixture is a REAL file under the
// matching charters/amendments/rulings subdirectory of `dir`, with content
// pre-ratified for AMENDMENT/RULING kinds — a test that targets kind/
// placement/ratification overrides exactly the field it needs; every other
// test gets a document that clears all three checks by default.
const KIND_SUBDIR = { CHARTER: "charters", AMENDMENT: "amendments", RULING: "rulings" };
let authDocSeq = 0;
function authDoc({ id, kind = "CHARTER", content, path, sha256 }) {
  const subdir = KIND_SUBDIR[kind] ?? "unknown-kind";
  const body = content ?? `# ${id} test fixture\n\n**Status:** RATIFIED (test fixture).\n`;
  let finalPath = path;
  let finalSha256 = sha256;
  if (finalPath === undefined) {
    mkdirSync(join(dir, subdir), { recursive: true });
    finalPath = join(dir, subdir, `${id}-${authDocSeq++}.md`);
    writeFileSync(finalPath, body, "utf8");
    finalSha256 ??= createHash("sha256").update(body).digest("hex");
  }
  finalSha256 ??= BASE_TXT_SHA256;
  return `[[document]]\nid = "${id}"\nkind = "${kind}"\npath = "${finalPath}"\nsha256 = "${finalSha256}"\n`;
}
function authRecord({
  block: blockId,
  documents,
  ratifiedBy = "maintainer",
  ratifiedAt = "2026-08-20",
  scope = "test authorization scope",
}) {
  const docsLine = `documents = [${documents.map((d) => `"${d}"`).join(", ")}]\n`;
  const byLine = ratifiedBy === null ? "" : `ratified_by = "${ratifiedBy}"\n`;
  const atLine = ratifiedAt === null ? "" : `ratified_at = "${ratifiedAt}"\n`;
  const scopeLine = scope === null ? "" : `scope = "${scope}"\n`;
  return `[[authorization]]\nblock = "${blockId}"\n${docsLine}${byLine}${atLine}${scopeLine}`;
}
function authorityText(parts) {
  return `schema = 1\nrevision = 11\n\n${parts.join("\n")}`;
}
const VALID_AUTHORITY = authorityText([
  authDoc({ id: "DOC-1" }),
  authRecord({ block: "A0", documents: ["DOC-1"] }),
]);

test("[PS] --authority is mandatory by default: a missing default registry next to --state is a violation", () => {
  const dag = write("dag-auth-mandatory.toml", DAG1);
  const state = write(
    "state-auth-mandatory.toml",
    header({ current: "A0" }) + block("A0", "READY"),
  );
  // Neither --authority nor --no-authority given — enforcement must be
  // unconditional, not opt-in (BLOCKING: nothing in the real repo passed
  // --authority, so the whole check was dead code).
  const r = runPS(dag, state, "live", [TEST_DEFAULT_AUTHORITY_MARKER]);
  expectCheck(PS_FILE, "could not be read or parsed", r);
  assert.match(
    r.err,
    /authority-registry\.toml/,
    `expected the default path next to --state:\n${r.err}`,
  );
});

test("[PS] --authority default path next to --state is picked up automatically when present", () => {
  const dag = write("dag-auth-default-ok.toml", DAG1);
  const state = write(
    "state-auth-default-ok.toml",
    header({ current: "A0" }) + block("A0", "READY"),
  );
  // Written to the SAME directory as --state under the fixed default name —
  // no --authority flag given, proving the default resolution actually
  // engages rather than silently no-op'ing.
  writeFileSync(join(dirname(state), "authority-registry.toml"), VALID_AUTHORITY, "utf8");
  const r = runPS(dag, state, "live", [TEST_DEFAULT_AUTHORITY_MARKER]);
  assert.equal(
    r.status,
    0,
    `expected pass via the default authority path, got:\n${r.err}\n${r.out}`,
  );
});

test("[PS] --no-authority is the sole, explicit opt-out", () => {
  const dag = write("dag-auth-skip.toml", DAG1);
  const state = write("state-auth-skip.toml", header({ current: "A0" }) + block("A0", "READY"));
  const r = runPS(dag, state, "live", ["--no-authority"]);
  assert.equal(r.status, 0, `expected pass with --no-authority given, got:\n${r.err}\n${r.out}`);
});

test("[PS] --authority and --no-authority together is a usage error", () => {
  const dag = write("dag-auth-conflict.toml", DAG1);
  const state = write("state-auth-conflict.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write("authority-conflict.toml", VALID_AUTHORITY);
  const r = runPS(dag, state, "live", ["--authority", authority, "--no-authority"]);
  assert.equal(r.status, 2, `expected a usage error (exit 2), got:\n${r.err}\n${r.out}`);
  assert.match(r.err, /mutually exclusive/, `expected a mutual-exclusivity message:\n${r.err}`);
});

test("[PS] the reviewer's self-authorization bypass is rejected: an arbitrary correctly-digested file cited as CHARTER authority for a self-ratified block", () => {
  const dag = write("dag-auth-bypass.toml", DAG1);
  const state = write("state-auth-bypass.toml", header({ current: "A0" }) + block("A0", "READY"));
  // The exact shape of the proven bypass: a real, correctly-digested file
  // that is NOT a charter/amendment/ruling, tagged CHARTER, backing a
  // self-ratified authorization record.
  const readme = join(dir, "not-an-authority-document.md");
  writeFileSync(readme, "# Not a charter, amendment, or ruling\n", "utf8");
  const readmeDigest = createHash("sha256").update(readFileSync(readme)).digest("hex");
  const authority = write(
    "authority-bypass.toml",
    authorityText([
      `[[document]]\nid = "FAKE"\nkind = "CHARTER"\npath = "${readme}"\nsha256 = "${readmeDigest}"\n`,
      authRecord({ block: "A0", documents: ["FAKE"], ratifiedBy: "A0 implementer (self)" }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "does not resolve under", r);
});

test("[PS] authority document has an invalid kind", () => {
  const dag = write("dag-auth-badkind.toml", DAG1);
  const state = write("state-auth-badkind.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-badkind.toml",
    authorityText([
      `[[document]]\nid = "DOC-1"\nkind = "MEMO"\npath = "${join(dir, "charters", "x.md")}"\nsha256 = "${BASE_TXT_SHA256}"\n`,
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "not one of CHARTER/AMENDMENT/RULING", r);
});

test("[PS] authority AMENDMENT document has no **Status:** line", () => {
  const dag = write("dag-auth-amdnostatus.toml", DAG1);
  const state = write(
    "state-auth-amdnostatus.toml",
    header({ current: "A0" }) + block("A0", "READY"),
  );
  const authority = write(
    "authority-amdnostatus.toml",
    authorityText([
      authDoc({
        id: "DOC-1",
        kind: "AMENDMENT",
        content: "# AMD test fixture\n\nNo status field here.\n",
      }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "(AMENDMENT) ${doc.path} has no **Status:** line", r);
});

test("[PS] authority AMENDMENT document is not ratified", () => {
  const dag = write("dag-auth-amdunrat.toml", DAG1);
  const state = write("state-auth-amdunrat.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-amdunrat.toml",
    authorityText([
      authDoc({
        id: "DOC-1",
        kind: "AMENDMENT",
        content:
          "# AMD test fixture\n\n**Status:** PROPOSED — NOT RATIFIED. No execution authority.\n",
      }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "(AMENDMENT) ${doc.path} is not ratified", r);
});

test("[PS] authority RULING document declares an unratified Status", () => {
  const dag = write("dag-auth-rulingdraft.toml", DAG1);
  const state = write(
    "state-auth-rulingdraft.toml",
    header({ current: "A0" }) + block("A0", "READY"),
  );
  const authority = write(
    "authority-rulingdraft.toml",
    authorityText([
      authDoc({
        id: "DOC-1",
        kind: "RULING",
        content:
          "# Ruling test fixture\n\n**Status:** DRAFT — authored for maintainer review; no ratification yet.\n",
      }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "(RULING) ${doc.path} declares Status:", r);
});

test("[PS] a RULING document with no Status line is NOT a violation (rulings aren't held to the charter/amendment convention)", () => {
  const dag = write("dag-auth-rulingok.toml", DAG1);
  const state = write("state-auth-rulingok.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-rulingok.toml",
    authorityText([
      authDoc({
        id: "DOC-1",
        kind: "RULING",
        content:
          "# Maintainer ruling — narrative only, no **Status:** line.\n\nGiven by the maintainer.\n",
      }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] a fully valid --authority registry passes", () => {
  const dag = write("dag-auth-ok.toml", DAG1);
  const state = write("state-auth-ok.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write("authority-ok.toml", VALID_AUTHORITY);
  const r = runPS(dag, state, "live", ["--authority", authority]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] authority registry cannot be read", () => {
  const dag = write("dag-auth1.toml", DAG1);
  const state = write("state-auth1.toml", header({ current: "A0" }) + block("A0", "READY"));
  const r = runPS(dag, state, "live", ["--authority", join(dir, "nonexistent-authority.toml")]);
  expectCheck(PS_FILE, "could not be read or parsed", r);
});

test("[PS] authority registry is unparseable TOML", () => {
  const dag = write("dag-auth1b.toml", DAG1);
  const state = write("state-auth1b.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write("authority-bad.toml", "schema = 1\n[[document\nbroken\n");
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "could not be read or parsed", r);
});

test("[PS] authority document has a malformed sha256", () => {
  const dag = write("dag-auth2.toml", DAG1);
  const state = write("state-auth2.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-badshape.toml",
    authorityText([
      authDoc({ id: "DOC-1", sha256: "not-a-digest" }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "has a [[document]] with a missing id/path or a malformed sha256", r);
});

test("[PS] authority registry declares a duplicate document id", () => {
  const dag = write("dag-auth3.toml", DAG1);
  const state = write("state-auth3.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-dupdoc.toml",
    authorityText([
      // Two independently-valid (kind/placement/shape all clear) documents
      // sharing one id — each authDoc() call writes its own distinct file.
      authDoc({ id: "DOC-1" }),
      authDoc({ id: "DOC-1" }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "declares more than one [[document]] with id", r);
});

test("[PS] authority document path does not exist on disk", () => {
  const dag = write("dag-auth4.toml", DAG1);
  const state = write("state-auth4.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-missingfile.toml",
    authorityText([
      // Correctly placed under charters/ (so kind/placement clear) but the
      // file itself was never written.
      authDoc({
        id: "DOC-1",
        path: join(dir, "charters", "does-not-exist.md"),
        sha256: BASE_TXT_SHA256,
      }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "which does not exist on disk — authority is not bound to exact bytes", r);
});

test("[PS] authority document digest is stale", () => {
  const dag = write("dag-auth5.toml", DAG1);
  const state = write("state-auth5.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-stale.toml",
    authorityText([
      // Real, correctly-placed file (written by authDoc's default path) but
      // an sha256 override that does not match its actual bytes.
      authDoc({ id: "DOC-1", sha256: TIP_TXT_SHA256 }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "does not match the current SHA-256 of", r);
});

test("[PS] authorization record has no string block id", () => {
  const dag = write("dag-auth6.toml", DAG1);
  const state = write("state-auth6.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-noblockid.toml",
    authorityText([
      authDoc({ id: "DOC-1" }),
      `[[authorization]]\ndocuments = ["DOC-1"]\nratified_by = "maintainer"\nratified_at = "2026-08-20"\nscope = "x"\n`,
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "has an [[authorization]] record with no string block id", r);
});

test("[PS] authority registry declares a duplicate authorization for one block", () => {
  const dag = write("dag-auth7.toml", DAG1);
  const state = write("state-auth7.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-dupauth.toml",
    authorityText([
      authDoc({ id: "DOC-1" }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
      authRecord({ block: "A0", documents: ["DOC-1"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "declares more than one [[authorization]] record for block", r);
});

test("[PS] authorization is missing required metadata fields", () => {
  const dag = write("dag-auth8.toml", DAG1);
  const state = write("state-auth8.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-missingmeta.toml",
    authorityText([
      authDoc({ id: "DOC-1" }),
      authRecord({ block: "A0", documents: ["DOC-1"], ratifiedBy: null }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "is missing required field(s):", r);
});

test("[PS] authorization names zero authority documents", () => {
  const dag = write("dag-auth9.toml", DAG1);
  const state = write("state-auth9.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-zerodoc.toml",
    authorityText([authRecord({ block: "A0", documents: [] })]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "names zero authority documents", r);
});

test("[PS] authorization references an unknown document id", () => {
  const dag = write("dag-auth10.toml", DAG1);
  const state = write("state-auth10.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write(
    "authority-unknowndoc.toml",
    authorityText([
      authDoc({ id: "DOC-1" }),
      authRecord({ block: "A0", documents: ["DOC-DOES-NOT-EXIST"] }),
    ]),
  );
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "references unknown document id", r);
});

test("[PS] a block past LOCKED has no authorization record", () => {
  const dag = write("dag-auth11.toml", DAG1);
  const state = write("state-auth11.toml", header({ current: "A0" }) + block("A0", "READY"));
  const authority = write("authority-empty.toml", authorityText([]));
  const r = runPS(dag, state, "live", ["--authority", authority]);
  expectCheck(PS_FILE, "no [[authorization]] record for it", r);
});

// =====================================================================
// PROGRAM-STATE — concurrent-implementation ceiling, serialised FINAL
// certification, current_block binding, and the fixed-landing-order
// cumulative rehearsal (AMD-013)
// (MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md,
// ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md,
// contracts/stacked-prs.md)
// =====================================================================

// A single-root DAG (R0, ACCEPTED) with N children (each predecessors =
// [R0]) — satisfies the "exactly one DAG root" / sequencing checks while
// giving N mutually-independent blocks to hold concurrently active or
// certifying, so every fixture below isolates the ONE new check it names
// rather than incidentally tripping the unrelated multi-root violation too.
function childrenDag(ids) {
  return dagText([
    dagBlock({ id: "R0", predecessors: [] }),
    ...ids.map((id) => dagBlock({ id, predecessors: ["R0"] })),
  ]);
}
function rootAccepted() {
  return acceptedBlock("R0");
}

const SIX_CHILD_IDS = ["A0", "B0", "C0", "D0", "E0", "F0"];
const SIX_CHILD_DAG = childrenDag(SIX_CHILD_IDS);
const SIX_CHILD_DAG_DIGEST = createHash("sha256").update(SIX_CHILD_DAG).digest("hex");

test("[PS] more than 5 blocks concurrently active (the ratified ceiling)", () => {
  const dag = write("dag-cur1.toml", SIX_CHILD_DAG);
  const state = write(
    "state-cur1.toml",
    header({ current: "A0", dagDigest: SIX_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      SIX_CHILD_IDS.map((id) => block(id, "IN_PROGRESS")).join("\n"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the ratified concurrent-implementation/train ceiling", r);
});

test("[PS] five IN_PROGRESS plus one ACCEPTANCE_RECOMMENDED is six active trains — over the ceiling (Finding B)", () => {
  // The exact shape the AMD-013 v3 review flagged: the prior draft capped
  // ONLY implementing.length, so 5 IN_PROGRESS + 1 ACCEPTANCE_RECOMMENDED
  // (6 concurrently active blocks) was silently legal. Every block here is
  // rehearsal-ready (base_sha/candidate identity + landing_order) and
  // pairwise disjoint so the ceiling violation is isolated from any
  // rehearsal-input violation.
  const ids = ["A0", "B0", "C0", "D0", "E0", "F0"];
  const dag = write("dag-cur1b.toml", childrenDag(ids));
  const dagDigest = createHash("sha256").update(childrenDag(ids)).digest("hex");
  const inProgress = (id, order) =>
    block(id, "IN_PROGRESS", {
      base_sha: SHA,
      implementation_candidate_sha: SHA,
      landing_order: order,
    });
  const state = write(
    "state-cur1b.toml",
    header({ current: "A0", dagDigest }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", {
        base_sha: SHA_BASE,
        candidate_sha: CONCURRENT_A,
        candidate_tree: CONCURRENT_A_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        conformance_review: "PASS",
        conformance_reviewed_sha: CONCURRENT_A,
        architecture_review: "PASS",
        architecture_reviewed_sha: CONCURRENT_A,
        adversarial_review: "PASS",
        adversarial_reviewed_sha: CONCURRENT_A,
        landing_order: 1,
      }) +
      "\n" +
      inProgress("B0", 2) +
      "\n" +
      inProgress("C0", 3) +
      "\n" +
      inProgress("D0", 4) +
      "\n" +
      inProgress("E0", 5) +
      "\n" +
      inProgress("F0", 6),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the ratified concurrent-implementation/train ceiling", r);
  assert.match(r.err, /counts every concurrently active block regardless of status/);
});

test("[PS] current_block names no state block", () => {
  const dag = write("dag-cur2.toml", DAG1);
  const state = write("state-cur2.toml", header({ current: "ZZZ" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "names no state block", r);
});

const TWO_CHILD_DAG = childrenDag(["A0", "B0"]);
const TWO_CHILD_DAG_DIGEST = createHash("sha256").update(TWO_CHILD_DAG).digest("hex");

// A0 -> A1 chain (both below the ACCEPTED root R0) — the one fixture family
// below that needs a REAL DAG predecessor edge between two concurrently
// active blocks (every other fixture in this section uses mutually
// independent siblings deliberately, per the comment above childrenDag).
const CHAIN_DAG = dagText([
  dagBlock({ id: "R0", predecessors: [] }),
  dagBlock({ id: "A0", predecessors: ["R0"] }),
  dagBlock({ id: "A1", predecessors: ["A0"] }),
]);
const CHAIN_DAG_DIGEST = createHash("sha256").update(CHAIN_DAG).digest("hex");

// A REVIEW/ACCEPTANCE_RECOMMENDED row over an unaccepted DAG predecessor
// needs the full contingent-stacked-work exception fields (governance.md:6)
// or the unrelated sequencing violation fires alongside whatever this
// fixture is actually isolating.
function stackedOver(id, status, overrides = {}) {
  return block(id, status, {
    stack_id: "S1",
    stack_snapshot_digest: DIGEST,
    stack_layer: id === "A0" ? 0 : 1,
    ...overrides,
  });
}

test("[PS] more than one block ACCEPTANCE_RECOMMENDED (final certification must serialise to one)", () => {
  const dag = write("dag-cert1.toml", TWO_CHILD_DAG);
  const acceptanceRecommended = (overrides) => ({
    base_sha: SHA,
    candidate_sha: SHA,
    candidate_tree: TREE,
    charter_digest: DIGEST,
    context_packet_digest: DIGEST,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    conformance_reviewed_sha: SHA,
    architecture_review: "PASS",
    architecture_reviewed_sha: SHA,
    adversarial_review: "PASS",
    adversarial_reviewed_sha: SHA,
    ...overrides,
  });
  const state = write(
    "state-cert1.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", acceptanceRecommended({ landing_order: 1 })) +
      "\n" +
      block("B0", "ACCEPTANCE_RECOMMENDED", acceptanceRecommended({ landing_order: 2 })),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "final certification must serialise to exactly one block at a time", r);
});

test("[PS] ACCEPTANCE_RECOMMENDED block disagrees with current_block", () => {
  const dag = write("dag-cur3.toml", TWO_CHILD_DAG);
  const state = write(
    "state-cur3.toml",
    header({ current: "B0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", {
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        conformance_review: "PASS",
        conformance_reviewed_sha: SHA,
        architecture_review: "PASS",
        architecture_reviewed_sha: SHA,
        adversarial_review: "PASS",
        adversarial_reviewed_sha: SHA,
      }) +
      "\n" +
      block("B0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "current_block must name the sole block under final certification", r);
});

const THREE_CHILD_DAG = childrenDag(["A0", "B0", "C0"]);
const THREE_CHILD_DAG_DIGEST = createHash("sha256").update(THREE_CHILD_DAG).digest("hex");

test("[PS] current_block not among the concurrently active blocks (nothing ACCEPTANCE_RECOMMENDED)", () => {
  const dag = write("dag-cur4.toml", THREE_CHILD_DAG);
  const state = write(
    "state-cur4.toml",
    header({ current: "C0", dagDigest: THREE_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      // A0 REVIEW (not ACCEPTANCE_RECOMMENDED) proves REVIEW is itself a
      // legal current_block target under the new model — the prior draft's
      // model would have treated A0 as "certifying" here instead.
      block("A0", "REVIEW", {
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS") +
      "\n" +
      block("C0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not one of the concurrently active (IN_PROGRESS/REVIEW) blocks", r);
});

// -- Fixed-landing-order cumulative rehearsal (verifyConcurrentLandingSafety)

test("[PS] concurrently active IN_PROGRESS block missing a well-formed implementation_candidate_sha", () => {
  const dag = write("dag-disj1.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj1.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", { landing_order: 2 }), // no implementation_candidate_sha
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "every IN_PROGRESS block must bind a real rehearsal identity, whether or not another block is concurrently active",
    r,
  );
});

test("[PS] concurrently active REVIEW block missing a well-formed candidate_sha (rehearsal-specific check, distinct from the unconditional IN_PROGRESS check above)", () => {
  const dag = write("dag-disj1b.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj1b.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      // A REVIEW row with NO candidate_sha at all — EVIDENCE_BOUND (elsewhere
      // in main()) already reports its own violation for this, but the
      // rehearsal's own candidate-identity check (verifyConcurrentLandingSafety,
      // the non-IN_PROGRESS branch) is a SEPARATE call site and must ALSO fire,
      // independently of the IN_PROGRESS-specific check exercised above.
      block("B0", "REVIEW", { landing_order: 2 }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "cannot be established without a real candidate identity for every concurrently active block",
    r,
  );
});

test("[PS] IN_PROGRESS block's implementation_ref is not a well-formed ref name (Finding E, second bullet — closed)", () => {
  const dag = write("dag-implref1.toml", TWO_CHILD_DAG);
  const state = write(
    "state-implref1.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: "", // missing — the default, unbound declaration
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "implementation_candidate_sha must be bound to a resolvable live ref", r);
});

test("[PS] IN_PROGRESS block's implementation_ref does not resolve to a real commit", () => {
  const dag = write("dag-implref2.toml", TWO_CHILD_DAG);
  const state = write(
    "state-implref2.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: "no-such-branch-anywhere",
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "could not be resolved (git rev-parse --verify)", r);
});

test("[PS] IN_PROGRESS block's implementation_ref resolves, but its live tip does not match the declared implementation_candidate_sha (stale pin)", () => {
  // Finding E, second bullet, closed: implementation_ref is a REAL, live ref
  // (concurrent-b) — it resolves fine — but the ledger declares
  // implementation_candidate_sha as a DIFFERENT real commit (concurrent-a).
  // A validator that only checked "does implementation_ref resolve to
  // SOMETHING" (rather than requiring it to equal the pin exactly) would
  // wrongly pass this.
  const dag = write("dag-implref3.toml", TWO_CHILD_DAG);
  const state = write(
    "state-implref3.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A, // stale — the ref has moved on
        implementation_ref: CONCURRENT_B_REF, // live tip is CONCURRENT_B, not CONCURRENT_A
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_C,
        implementation_ref: CONCURRENT_C_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the pin does not match the live ref's current tip", r);
});

// -- Round 4, FIX 1: implementation_ref must be an actual, normalized branch
// ref — never a raw object id or the HEAD pseudoref, both of which
// trivially "resolve" (to themselves, or to wherever this worktree happens
// to be checked out) no matter how stale implementation_candidate_sha
// really is, and never any other rev-parse-able object (e.g. a tag) that
// isn't a branch.

test("[PS] implementation_ref is rejected when it is a raw 40-char object id, even though it trivially 'resolves' to itself (round 4, FIX 1)", () => {
  const dag = write("dag-implref-oid.toml", DAG1);
  const state = write(
    "state-implref-oid.toml",
    header({ current: "A0" }) +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: SHA,
        implementation_ref: SHA, // a raw OID, self-matching under the pre-fix defect
        base_sha: SHA_BASE,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "is a raw object id or the literal HEAD pseudoref, not an actual branch ref",
    r,
  );
});

test("[PS] implementation_ref is rejected when it is the literal HEAD pseudoref, even though it trivially resolves wherever this worktree is checked out (round 4, FIX 1)", () => {
  const dag = write("dag-implref-head.toml", DAG1);
  const state = write(
    "state-implref-head.toml",
    header({ current: "A0" }) +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: SHA, // gitRoot's checked-out tip — HEAD resolves here too
        implementation_ref: "HEAD",
        base_sha: SHA_BASE,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "is a raw object id or the literal HEAD pseudoref, not an actual branch ref",
    r,
  );
});

test("[PS] implementation_ref that resolves to a real, tip-matching commit but is NOT a branch (a TAG) is still rejected (round 4, FIX 1)", () => {
  const dag = write("dag-implref-tag.toml", DAG1);
  const state = write(
    "state-implref-tag.toml",
    header({ current: "A0" }) +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_TAG, // real ref, resolves, tip matches — but a tag, not a branch
        base_sha: SHA_BASE,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "implementation_ref must name an actual refs/heads/... branch, never any other rev-parse-able object or pseudoref",
    r,
  );
});

// -- Round 4, FIX 2: the implementation_ref/implementation_candidate_sha
// binding must be checked for a SOLE IN_PROGRESS block — never gated on
// verifyConcurrentLandingSafety's own active.length > 1 rehearsal, which the
// ordinary, overwhelmingly common single-IN_PROGRESS ledger never satisfies.

test("[PS] implementation_ref/implementation_candidate_sha binding is checked even with exactly ONE active IN_PROGRESS block (round 4, FIX 2 scoping discriminator)", () => {
  const dag = write("dag-implref-solo.toml", DAG1);
  const state = write(
    "state-implref-solo.toml",
    header({ current: "A0" }) +
      block("A0", "IN_PROGRESS", { implementation_ref: "no-such-branch-anywhere-solo" }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "every IN_PROGRESS block must bind a real rehearsal identity, whether or not another block is concurrently active",
    r,
  );
});

// -- Round 4 mutation-kill discriminators. Each empirically PROVES its test
// actually discriminates (not merely asserted): apply the exact rejected
// mutation to a SCRATCH COPY of validate-program-state.mjs, run the same
// fixture against the mutated copy and observe it WRONGLY passes (the bug
// this fix closed is back), then run the real, unmutated PS_FILE against the
// identical fixture and observe it correctly fails.

// The scratch copy is written INTO scripts/ (alongside PS_FILE) rather than
// a mkdtemp scratch dir — validate-program-state.mjs imports
// ./lib/rev11-toml.mjs and ./lib/stack-window-lib.mjs by relative path, so a
// copy dropped anywhere else fails to resolve those imports at runtime (a
// setup failure, not a discriminating result). A unique per-call filename,
// removed in the caller's `finally`, keeps this test-only and non-committed.
let scratchCounter = 0;
function scratchMutate(mutate, label) {
  const original = readFileSync(PS_FILE, "utf8");
  const mutated = applied(original, mutate(original), label);
  const scratchPath = join(
    HERE,
    `.mutation-scratch-${process.pid}-${scratchCounter++}.validate-program-state.mjs`,
  );
  writeFileSync(scratchPath, mutated, "utf8");
  return scratchPath;
}

function runScratch(scratchPath, dagPath, statePath, mode) {
  const res = spawnSync(
    process.execPath,
    [scratchPath, "--dag", dagPath, "--state", statePath, "--mode", mode, "--no-authority"],
    { encoding: "utf8", cwd: gitRoot },
  );
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "" };
}

test("[PS] mutation-kill: reintroducing raw-OID/HEAD acceptance for implementation_ref is caught by the round-4 FIX-1 test", () => {
  const scratchPath = scratchMutate(
    (src) => src.replace('if (ref === "HEAD" || SHA_RE.test(ref)) {', "if (false) {"),
    "neutralize the HEAD/raw-OID rejection",
  );
  try {
    const dag = write("dag-mutkill-oid.toml", DAG1);
    const state = write(
      "state-mutkill-oid.toml",
      header({ current: "A0" }) +
        block("A0", "IN_PROGRESS", {
          implementation_candidate_sha: SHA,
          implementation_ref: "HEAD",
          base_sha: SHA_BASE,
        }),
    );
    const mutatedResult = runScratch(scratchPath, dag, state, "live");
    assert.equal(
      mutatedResult.status,
      0,
      `expected the MUTATED (bug-reintroducing) validator to wrongly PASS, got:\n${mutatedResult.err}\n${mutatedResult.out}`,
    );
    const realResult = runPS(dag, state, "live");
    expectCheck(
      PS_FILE,
      "is a raw object id or the literal HEAD pseudoref, not an actual branch ref",
      realResult,
    );
  } finally {
    rmSync(scratchPath, { force: true });
  }
});

test("[PS] mutation-kill: reintroducing the active.length < 2 scoping gate silently un-checks a sole IN_PROGRESS block's implementation_ref, caught by the round-4 FIX-2 test", () => {
  const scratchPath = scratchMutate(
    (src) =>
      src.replace(
        "const implementationRefResults = verifyImplementationRefFields(stateById, v);",
        "const implementationRefResults = new Map();",
      ),
    "neutralize the unconditional implementation_ref field check",
  );
  try {
    const dag = write("dag-mutkill-scope.toml", DAG1);
    const state = write(
      "state-mutkill-scope.toml",
      header({ current: "A0" }) +
        block("A0", "IN_PROGRESS", { implementation_ref: "no-such-branch-anywhere-mutkill" }),
    );
    const mutatedResult = runScratch(scratchPath, dag, state, "live");
    assert.equal(
      mutatedResult.status,
      0,
      `expected the MUTATED (scoping-regressed) validator to wrongly PASS a lone unchecked IN_PROGRESS block, got:\n${mutatedResult.err}\n${mutatedResult.out}`,
    );
    const realResult = runPS(dag, state, "live");
    expectCheck(
      PS_FILE,
      "every IN_PROGRESS block must bind a real rehearsal identity, whether or not another block is concurrently active",
      realResult,
    );
  } finally {
    rmSync(scratchPath, { force: true });
  }
});

test("[PS] landing_order is not a positive integer", () => {
  const dag = write("dag-lo1.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo1.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      // landing_order 0 is the block()/template default — the "not
      // participating" value, invalid the moment >1 block is active.
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 0,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is not a positive integer", r);
});

test("[PS] entry-lock identity: repository.branch is not a well-formed branch name", () => {
  // AMD-013 round 5: repository.branch/head_sha are now the IMMUTABLE A0
  // entry-lock identity (verifyEntryLockIdentity), never the trunk-pin
  // oracle (that is repository.integration_branch/integration_head_sha,
  // covered separately below). Runs UNCONDITIONALLY, on a single-block
  // ledger — no rehearsal in play at all — to demonstrate it is not gated
  // on concurrency.
  const dag = write("dag-trunk0.toml", DAG1);
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(
    base,
    base.replace(/branch = "main"/, 'branch = ""'),
    "empty repository.branch",
  );
  const state = write("state-trunk0.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the immutable A0 entry-lock branch", r);
});

test("[PS] entry-lock identity: repository.head_sha is not a resolved 40-char lowercase git object id", () => {
  const dag = write("dag-trunk1.toml", TWO_CHILD_DAG);
  const base =
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
    rootAccepted() +
    "\n" +
    block("A0", "IN_PROGRESS", {
      implementation_candidate_sha: CONCURRENT_A,
      implementation_ref: CONCURRENT_A_REF,
      base_sha: SHA_BASE,
      landing_order: 1,
    }) +
    "\n" +
    block("B0", "IN_PROGRESS", {
      implementation_candidate_sha: CONCURRENT_B,
      implementation_ref: CONCURRENT_B_REF,
      base_sha: SHA_BASE,
      landing_order: 2,
    });
  const mutated = applied(
    base,
    base.replace(/head_sha = "[0-9a-f]{40}"/, 'head_sha = "not-a-real-sha"'),
    "malform repository.head_sha",
  );
  const state = write("state-trunk1.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the immutable A0 entry-lock checkout SHA", r);
});

test("[PS] entry-lock identity: repository.head_sha resolved but does not equal entry_checkout_sha (drift)", () => {
  // A well-formed but WRONG head_sha — the entry-lock identity has drifted
  // from its own entry_checkout_sha record. Distinct violation from the
  // malformed-shape case above (that one never reaches the equality check).
  const dag = write("dag-trunk1b.toml", DAG1);
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(
    base,
    base.replace(/head_sha = "[0-9a-f]{40}"/, `head_sha = "${SHA_BASE}"`),
    "drift repository.head_sha away from entry_checkout_sha",
  );
  const state = write("state-trunk1b.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "the immutable A0 entry-lock SHA has drifted from its own entry-checkout record",
    r,
  );
});

test("[PS] entry-lock identity: repository.head_tree is not a resolved 40-char lowercase tree object id", () => {
  const dag = write("dag-trunk1c.toml", DAG1);
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(
    base,
    base.replace(/head_tree = "[0-9a-f]{40}"/, 'head_tree = "not-a-real-sha"'),
    "malform repository.head_tree",
  );
  const state = write("state-trunk1c.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the immutable A0 entry-lock checkout TREE", r);
});

test("[PS] entry-lock identity: repository.head_tree resolved but does not equal entry_checkout_tree (drift)", () => {
  const dag = write("dag-trunk1d.toml", DAG1);
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(
    base,
    base.replace(/head_tree = "[0-9a-f]{40}"/, `head_tree = "${SHA_BASE}"`),
    "drift repository.head_tree away from entry_checkout_tree",
  );
  const state = write("state-trunk1d.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "the immutable A0 entry-lock TREE has drifted from its own entry-checkout record",
    r,
  );
});

test("[PS] pinned-trunk resolution: repository.integration_branch is not a well-formed branch name", () => {
  // Trunk-pin resolution (resolvePinnedTrunk) runs UNCONDITIONALLY on every
  // live-mode validation, sourced from integration_branch/integration_head_sha
  // (AMD-013 round 5) — distinct from, and never satisfied by, the
  // entry-lock repository.branch/head_sha pair covered above. Proven here
  // with a SINGLE-block ledger — no rehearsal in play at all — to
  // demonstrate it is not gated on concurrency.
  const dag = write("dag-trunk0i.toml", DAG1);
  const base = header({ current: "A0" }) + block("A0", "LOCKED");
  const mutated = applied(
    base,
    base.replace(/integration_branch = "main"/, 'integration_branch = ""'),
    "empty repository.integration_branch",
  );
  const state = write("state-trunk0i.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the ledger must name the EXPLICIT configured integration-trunk ref", r);
});

test("[PS] pinned-trunk resolution: repository.integration_head_sha is not a resolved 40-char lowercase git object id", () => {
  // Trunk-pin resolution runs unconditionally now (see the test above); this
  // fixture still uses two concurrently active blocks so the SAME state also
  // exercises the fixed-landing-order rehearsal's own consumption of the
  // (failed-to-resolve) pin, one call site down.
  const dag = write("dag-trunk1i.toml", TWO_CHILD_DAG);
  const base =
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
    rootAccepted() +
    "\n" +
    block("A0", "IN_PROGRESS", {
      implementation_candidate_sha: CONCURRENT_A,
      implementation_ref: CONCURRENT_A_REF,
      base_sha: SHA_BASE,
      landing_order: 1,
    }) +
    "\n" +
    block("B0", "IN_PROGRESS", {
      implementation_candidate_sha: CONCURRENT_B,
      implementation_ref: CONCURRENT_B_REF,
      base_sha: SHA_BASE,
      landing_order: 2,
    });
  const mutated = applied(
    base,
    base.replace(
      /integration_head_sha = "[0-9a-f]{40}"/,
      'integration_head_sha = "not-a-real-sha"',
    ),
    "malform repository.integration_head_sha",
  );
  const state = write("state-trunk1i.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the ledger must PIN the integration-trunk identity", r);
});

test("[PS] concurrently active block missing base_sha (candidate present, base absent)", () => {
  const dag = write("dag-basemiss.toml", TWO_CHILD_DAG);
  const state = write(
    "state-basemiss.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      // A0 has a well-formed candidate but NO base_sha — isolates the
      // base_sha-missing check (Finding D) from the candidate-missing check
      // above it, which this fixture does not trip.
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the fixed-landing-order rehearsal replays each block's own base_sha", r);
});

test("[PS] landing_order violates same-stack layer ordering (Finding E)", () => {
  const dag = write("dag-stacklo.toml", TWO_CHILD_DAG);
  const state = write(
    "state-stacklo.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      // A0/B0 are true DAG siblings (both children of R0, no predecessor
      // edge between them) sharing stack S1 — isolates the stack-layer
      // check from the DAG-predecessor-order check above it, which this
      // fixture does not trip.
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        stack_id: "S1",
        stack_layer: 0,
        landing_order: 2, // wrong — the lower stack_layer must land first
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        stack_id: "S1",
        stack_layer: 1,
        landing_order: 1,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "a lower stack layer must land before a higher layer in the same stack", r);
});

test("[PS] duplicate landing_order among concurrently active blocks", () => {
  const dag = write("dag-lo2.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo2.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "the fixed landing order must be an unambiguous total order", r);
});

test("[PS] ACCEPTANCE_RECOMMENDED block is not first in the fixed landing order", () => {
  const dag = write("dag-lo3.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo3.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", {
        base_sha: SHA,
        candidate_sha: CONCURRENT_A,
        candidate_tree: CONCURRENT_A_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        conformance_review: "PASS",
        conformance_reviewed_sha: CONCURRENT_A,
        architecture_review: "PASS",
        architecture_reviewed_sha: CONCURRENT_A,
        adversarial_review: "PASS",
        adversarial_reviewed_sha: CONCURRENT_A,
        landing_order: 2, // wrong — must be the minimum
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "the block under final certification must be first in the fixed landing order",
    r,
  );
});

test("[PS] predecessor landing_order not before its concurrently active dependent", () => {
  const dag = write("dag-lo4.toml", CHAIN_DAG);
  const state = write(
    "state-lo4.toml",
    header({ current: "A0", dagDigest: CHAIN_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      stackedOver("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 2, // wrong — A0 is A1's predecessor, must be lower
      }) +
      "\n" +
      stackedOver("A1", "REVIEW", {
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        landing_order: 1,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "a predecessor must land before its dependent in the fixed landing order",
    r,
  );
});

test("[PS] pinned-trunk resolution: repository tip cannot be resolved (broken git rev-parse)", () => {
  // The trunk pin (resolvePinnedTrunk, Finding C) resolves BEFORE the
  // rehearsal walk ever runs — a broken `git rev-parse HEAD` fails the pin
  // itself, not the (now internal-rev-parse-free) rehearsal walk.
  const dag = write("dag-lo5.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo5.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("rev-parse") });
  expectCheck(
    PS_FILE,
    "to revalidate the ledger's pinned trunk repository.integration_head_sha",
    r,
  );
});

test("[PS] pinned-trunk resolution: a pin lagging behind the live trunk tip, but a genuine ancestor of it, is valid rehearsal input (AMD-013 round 6 — staleness alone is not a violation)", () => {
  // AMD-013 round 6 correction: the ledger this pin lives in is committed TO
  // the branch it pins, so requiring EQUALITY made the pin stale the instant
  // the committing commit landed — unfixable by any amount of resyncing.
  // integration_head_sha is now checked for ANCESTRY, not equality: SHA_BASE
  // (the pin) is a genuine, real ancestor of SHA (the live "main" tip), so
  // this must PASS — this is the exact shape (integration_head_sha lagging
  // one commit behind the live integration branch) that broke the prior
  // equality check the instant AMD-013 itself landed.
  const dag = write("dag-lo5b.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo5b.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST, integrationHeadSha: SHA_BASE }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] pinned-trunk resolution: a pin that is NOT an ancestor of the live trunk tip fails closed, and the rehearsal itself does not silently proceed", () => {
  // The mirror case: AMD-013 round 6 replaced the equality check with an
  // ancestry check, so it must still fail closed for the case that actually
  // matters — a pin naming a commit the live trunk's history does not
  // contain at all. SHA_DANGLING is committed on a since-deleted branch off
  // SHA_BASE, diverging from (never merged into) main's tip SHA — it is
  // provably NOT an ancestor of SHA. Two concurrently active blocks so this
  // ALSO exercises verifyConcurrentLandingSafety's own distinct
  // null-trunk-resolution violation (it must not silently skip once
  // resolvePinnedTrunk fails, nor reuse the trunk-pin message).
  const dag = write("dag-trunk-nonanc.toml", TWO_CHILD_DAG);
  const state = write(
    "state-trunk-nonanc.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST, integrationHeadSha: SHA_DANGLING }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "is not an ancestor of the live tip of the configured integration-trunk ref",
    r,
  );
  expectCheck(PS_FILE, "the fixed-landing-order rehearsal cannot run for", r);
});

test("[PS] accepted_sha reachability resolves against the live integration-trunk tip, not the (possibly-stale) pin", () => {
  // AMD-013 round 6: the pin may now genuinely lag behind trunk (the test
  // above). Reachability of a landed accepted_sha must still be checked
  // against the trunk's LIVE tip, never the lagging pin — a block landed
  // AFTER the pin was last recorded is reachable from the live tip but NOT
  // from the stale-but-valid ancestor pin (an ancestor's rev-list never
  // contains its own descendants). Pin = SHA_BASE (a genuine ancestor of the
  // live tip SHA); accepted_sha = SHA (the live tip itself, a descendant of
  // the pin) — reachable from the live tip trivially, NOT reachable from
  // rev-list SHA_BASE. Using the pin here would wrongly reject this
  // genuinely landed block.
  const dag = write("dag-reach-live.toml", DAG1);
  const state = write(
    "state-reach-live.toml",
    header({ current: "A0", integrationHeadSha: SHA_BASE }) +
      acceptedBlock("A0", { base_sha: SHA_BASE }),
  );
  const r = runPS(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] declared base_sha is not an ancestor of its rehearsal candidate (stale base)", () => {
  const dag = write("dag-lo6.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo6.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      // SHA and CONCURRENT_A are SIBLINGS off SHA_BASE (see the before()
      // fixture setup) — SHA is a real commit, but not CONCURRENT_A's
      // ancestor. A restack cascade "the Nth block restacks N-1 times" that
      // silently kept a stale declared base is exactly this shape.
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "the declared delta cannot be trusted for the fixed-landing-order rehearsal",
    r,
  );
});

test("[PS] base_sha ancestry check itself failing (broken git merge-base) is reported distinctly", () => {
  // A blanket "merge-base" break now always hits resolvePinnedTrunk's own
  // ancestor check first (AMD-013 round 6 — see the dedicated resolvePinnedTrunk
  // merge-base failure test above), so this uses a targeted shim that fails
  // merge-base ONLY when invoked with SHA_BASE (the rehearsal's own base_sha
  // ancestry call), letting resolvePinnedTrunk's own call (against the
  // default header's SHA/SHA pin/live-tip pair) pass through to real git.
  const shimDir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-mergebase-targeted2-"));
  const shimScript = join(shimDir, "git");
  writeFileSync(
    shimScript,
    `#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const args = process.argv.slice(2);
if (args[0] === "merge-base" && args.includes("${SHA_BASE}")) {
  process.stderr.write("shim: simulated merge-base failure for base_sha ancestry check\\n");
  process.exit(17);
}
const res = spawnSync(process.env.FAKE_GIT_REAL, args, { stdio: "inherit" });
process.exit(res.status === null ? 1 : res.status);
`,
    "utf8",
  );
  chmodSync(shimScript, 0o755);

  const dag = write("dag-lo7.toml", TWO_CHILD_DAG);
  const state = write(
    "state-lo7.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      // SHA_BASE genuinely IS an ancestor of CONCURRENT_A — this isolates
      // the subprocess-failure branch from the stale-base violation above.
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], {
    env: { ...process.env, PATH: `${shimDir}:${process.env.PATH}`, FAKE_GIT_REAL: realGitPath },
  });
  expectCheck(PS_FILE, "ancestry against its rehearsal candidate", r);
  rmSync(shimDir, { recursive: true, force: true });
});

test("[PS] two concurrently active blocks with a real merge conflict are rejected", () => {
  const dag = write("dag-disj2.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj2.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_CONFLICT_A,
        implementation_ref: CONCURRENT_CONFLICT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_CONFLICT_B,
        implementation_ref: CONCURRENT_CONFLICT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "reports real content conflicts", r);
});

test("[PS] two concurrently active blocks that are genuinely disjoint, in fixed landing order, PASS", () => {
  const dag = write("dag-disj3.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj3.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] a certifying (ACCEPTANCE_RECOMMENDED) block plus an IN_PROGRESS block, disjoint and correctly ordered, PASS", () => {
  // Directly answers "cover the certifying block too": the sole
  // ACCEPTANCE_RECOMMENDED block's OWN candidate_sha is rehearsed here,
  // first in the fixed landing order, against real trunk — not silently
  // excluded the way the prior draft's `implementing`-only rehearsal did.
  const dag = write("dag-disj-cert.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj-cert.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", {
        base_sha: SHA_BASE,
        candidate_sha: CONCURRENT_A,
        candidate_tree: CONCURRENT_A_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        conformance_review: "PASS",
        conformance_reviewed_sha: CONCURRENT_A,
        architecture_review: "PASS",
        architecture_reviewed_sha: CONCURRENT_A,
        adversarial_review: "PASS",
        adversarial_reviewed_sha: CONCURRENT_A,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] two concurrently active REVIEW blocks alongside one ACCEPTANCE_RECOMMENDED block PASS (REVIEW is not capped at one)", () => {
  // The exact shape contracts/stacked-prs.md:100 requires and the prior
  // (rejected) draft's combined REVIEW+ACCEPTANCE_RECOMMENDED cap of 1 made
  // impossible: several green REVIEW layers plus the one currently eligible
  // ACCEPTANCE_RECOMMENDED landing block, all concurrently active — REVIEW
  // cardinality here is 2, disproving the rejected draft's cap outright.
  const ids = ["A0", "B0", "C0"];
  const dag = write("dag-review-uncapped.toml", childrenDag(ids));
  const dagDigest = createHash("sha256").update(childrenDag(ids)).digest("hex");
  const state = write(
    "state-review-uncapped.toml",
    header({ current: "A0", dagDigest }) +
      rootAccepted() +
      "\n" +
      block("A0", "ACCEPTANCE_RECOMMENDED", {
        base_sha: SHA_BASE,
        candidate_sha: CONCURRENT_A,
        candidate_tree: CONCURRENT_A_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        conformance_review: "PASS",
        conformance_reviewed_sha: CONCURRENT_A,
        architecture_review: "PASS",
        architecture_reviewed_sha: CONCURRENT_A,
        adversarial_review: "PASS",
        adversarial_reviewed_sha: CONCURRENT_A,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "REVIEW", {
        base_sha: SHA_BASE,
        candidate_sha: CONCURRENT_B,
        candidate_tree: CONCURRENT_B_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        landing_order: 2,
      }) +
      "\n" +
      block("C0", "REVIEW", {
        base_sha: SHA_BASE,
        candidate_sha: CONCURRENT_C,
        candidate_tree: CONCURRENT_C_TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
        landing_order: 3,
      }),
  );
  const r = runPS(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("[PS] the rehearsal replays each block's OWN declared base_sha, not an auto-derived merge-base (Finding A/D discriminator)", () => {
  // A regression-shaped discriminator, not just a positive-path assertion:
  // proves --merge-base is actually HONORED by the git merge-tree call, not
  // silently ignored in favor of git's own ancestry search. Dedicated repo
  // (not the shared gitRoot) so its own history shape is exact:
  //   DISC_ROOT -> DISC_TRUNK (main's tip, the trunk pin) -> DISC_CAND
  //     (A0's rehearsal candidate; DISC_TRUNK is its REAL immediate parent)
  // A0 DECLARES base_sha = DISC_ROOT — a real ancestor of DISC_CAND (passes
  // the ancestor check) but NOT its immediate parent. Replaying A0's delta
  // from the DECLARED base (base.txt "root"->"candidate-version") onto
  // trunk's OWN change from that SAME declared base (base.txt
  // "root"->"trunk-version") is a genuine same-line conflict. A validator
  // that (bug) let git auto-derive the merge-base instead of honoring
  // --merge-base would instead compute merge-base(DISC_TRUNK, DISC_CAND) =
  // DISC_TRUNK itself (DISC_CAND's real parent) and see a clean, no-op
  // merge — wrongly PASSING a base_sha that does not actually describe
  // A0's delta.
  const discRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-disc-"));
  git(["init", "-q"], discRoot);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], discRoot);
  git(["config", "user.email", "test@example.invalid"], discRoot);
  git(["config", "user.name", "Test"], discRoot);
  git(["config", "commit.gpgsign", "false"], discRoot);

  writeFileSync(join(discRoot, "base.txt"), "root\n");
  git(["add", "-A"], discRoot);
  git(["commit", "-q", "-m", "root"], discRoot);
  const DISC_ROOT = git(["rev-parse", "HEAD"], discRoot);

  writeFileSync(join(discRoot, "base.txt"), "trunk-version\n");
  git(["add", "-A"], discRoot);
  git(["commit", "-q", "-m", "trunk"], discRoot);
  const DISC_TRUNK = git(["rev-parse", "HEAD"], discRoot); // main stays here — the trunk pin
  const DISC_TRUNK_TREE = git(["rev-parse", "HEAD^{tree}"], discRoot);

  // Candidate commit made on its own branch off DISC_TRUNK; unlike
  // SHA_DANGLING's deliberately-deleted convention, this branch is KEPT
  // LIVE (AMD-013 FIX 2 — implementation_ref must resolve to it). main is
  // left at DISC_TRUNK.
  const DISC_CAND_REF = "disc-candidate";
  const DISC_TRUNK_REF = "main";
  git(["checkout", "-q", "-b", DISC_CAND_REF, DISC_TRUNK], discRoot);
  writeFileSync(join(discRoot, "base.txt"), "candidate-version\n");
  git(["add", "-A"], discRoot);
  git(["commit", "-q", "-m", "candidate"], discRoot);
  const DISC_CAND = git(["rev-parse", "HEAD"], discRoot);
  git(["checkout", "-q", "main"], discRoot);

  const acceptedR0 = block("R0", "ACCEPTED", {
    entry_lock_digest: DIGEST,
    charter_digest: DIGEST,
    context_packet_digest: DIGEST,
    base_sha: DISC_ROOT,
    candidate_sha: DISC_TRUNK,
    candidate_tree: DISC_TRUNK_TREE,
    accepted_sha: DISC_TRUNK,
    accepted_tree: DISC_TRUNK_TREE,
    landing_equivalence_digest: DIGEST,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    conformance_reviewed_sha: DISC_TRUNK,
    architecture_review: "PASS",
    architecture_reviewed_sha: DISC_TRUNK,
    adversarial_review: "PASS",
    adversarial_reviewed_sha: DISC_TRUNK,
    maintainer_decision: "ACCEPTED",
  });

  const dag = write("dag-discriminator.toml", TWO_CHILD_DAG);
  const state = write(
    "state-discriminator.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST, repoSha: DISC_TRUNK }) +
      acceptedR0 +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: DISC_CAND,
        implementation_ref: DISC_CAND_REF,
        base_sha: DISC_ROOT, // stale declared base — NOT A0's real immediate parent
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: DISC_TRUNK,
        implementation_ref: DISC_TRUNK_REF,
        base_sha: DISC_TRUNK,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], { cwd: discRoot });
  expectCheck(PS_FILE, "reports real content conflicts", r);
  rmSync(discRoot, { recursive: true, force: true });
});

test("[PS] cumulative landing rehearsal itself failing (broken git merge-tree) is reported distinctly", () => {
  const dag = write("dag-disj4.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj4.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("merge-tree") });
  expectCheck(PS_FILE, "cumulative landing rehearsal could not be checked", r);
});

test("[PS] cumulative landing rehearsal cannot synthesise a rehearsal commit (broken git commit-tree)", () => {
  const dag = write("dag-disj5.toml", TWO_CHILD_DAG);
  const state = write(
    "state-disj5.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("commit-tree") });
  expectCheck(PS_FILE, "could not synthesise a rehearsal commit", r);
});

test("[PS] trunk pin discriminator: the configured trunk ref (repository.branch), not checkout HEAD, is the oracle (round-2 fix)", () => {
  // Direct reproduction of the exact shape that surfaced the round-2 defect:
  // a worktree whose checkout HEAD sits on a DIFFERENT ref than the
  // ledger-declared trunk branch, while the pin correctly names trunk's own
  // live tip. A validator that (bug, round-1 fix) compared against checkout
  // HEAD instead of the configured branch would wrongly report trunk drift
  // here and FAIL — this fixture is discriminating: it PASSES under the
  // correct (branch-ref) oracle and would FAIL under the rejected
  // (checkout-HEAD) one, with nothing else in the ledger able to produce
  // that difference (single-block ledger, no rehearsal in play at all).
  const trunkRepo = mkdtempSync(join(tmpdir(), "validate-mutation-suite-trunkdisc-"));
  git(["init", "-q"], trunkRepo);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], trunkRepo);
  git(["config", "user.email", "test@example.invalid"], trunkRepo);
  git(["config", "user.name", "Test"], trunkRepo);
  git(["config", "commit.gpgsign", "false"], trunkRepo);

  writeFileSync(join(trunkRepo, "base.txt"), "base\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "-m", "base"], trunkRepo);
  const TRUNK_SHA = git(["rev-parse", "HEAD"], trunkRepo); // main stays here — the correct pin

  // A checkout position AHEAD of trunk on its own branch — exactly a
  // feature-branch worktree, or a review checkout, sitting somewhere other
  // than trunk while trunk itself has not moved.
  git(["checkout", "-q", "-b", "feature", TRUNK_SHA], trunkRepo);
  writeFileSync(join(trunkRepo, "feature.txt"), "feature\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "-m", "feature work"], trunkRepo);
  // HEAD now resolves to the feature-branch commit, NOT TRUNK_SHA — checkout
  // is deliberately left here, never switched back to main.

  const dag = write("dag-trunkdisc.toml", DAG1);
  const state = write(
    "state-trunkdisc.toml",
    header({ current: "A0", repoSha: TRUNK_SHA }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live", [], { cwd: trunkRepo });
  assert.equal(
    r.status,
    0,
    `expected PASS (pin matches trunk's own tip even though checkout HEAD sits elsewhere), got:\n${r.err}\n${r.out}`,
  );
  rmSync(trunkRepo, { recursive: true, force: true });
});

test("[PS] trunk pin discriminator: a pin behind the live trunk tip on the SAME lineage is not trunk drift (AMD-013 round 6 — ancestry, not equality)", () => {
  // AMD-013 round 6 superseded the prior "checkout HEAD matching the pin
  // does NOT excuse an actually-stale trunk branch" discriminator — under
  // equality, ANY lag behind trunk's live tip failed closed, which is
  // exactly the defect round 6 fixes (a ledger committed to the branch it
  // pins can never equal the live tip by the time the pin is read back).
  // STALE_SHA stays a genuine ancestor of main's new tip after "trunk
  // advanced" lands on top of it — checkout HEAD position is irrelevant
  // either way, so this must PASS.
  const trunkRepo = mkdtempSync(join(tmpdir(), "validate-mutation-suite-trunkdisc2-"));
  git(["init", "-q"], trunkRepo);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], trunkRepo);
  git(["config", "user.email", "test@example.invalid"], trunkRepo);
  git(["config", "user.name", "Test"], trunkRepo);
  git(["config", "commit.gpgsign", "false"], trunkRepo);

  writeFileSync(join(trunkRepo, "base.txt"), "base\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "-m", "base"], trunkRepo);
  const STALE_SHA = git(["rev-parse", "HEAD"], trunkRepo); // the ledger's lagging pin

  writeFileSync(join(trunkRepo, "advance.txt"), "advance\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "-m", "trunk advanced"], trunkRepo);
  // main has moved on ONE commit past STALE_SHA, on the SAME lineage —
  // STALE_SHA remains a real ancestor of main's new tip.

  const dag = write("dag-trunkdisc2.toml", DAG1);
  const state = write(
    "state-trunkdisc2.toml",
    header({ current: "A0", repoSha: STALE_SHA }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live", [], { cwd: trunkRepo });
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  rmSync(trunkRepo, { recursive: true, force: true });
});

test("[PS] trunk pin discriminator: a pin whose commit was REWRITTEN out of trunk's history fails closed", () => {
  // The case that actually matters after round 6: not mere lag, but a pin
  // naming a commit trunk's history no longer contains at all — e.g. an
  // amend/rebase rewrote the commit the pin named. STALE_SHA is amended
  // in place, producing a DIFFERENT commit object at the same branch
  // position; the original STALE_SHA object still exists (git gc has not
  // run) but is provably no longer an ancestor of the rewritten tip.
  const trunkRepo = mkdtempSync(join(tmpdir(), "validate-mutation-suite-trunkrewrite-"));
  git(["init", "-q"], trunkRepo);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], trunkRepo);
  git(["config", "user.email", "test@example.invalid"], trunkRepo);
  git(["config", "user.name", "Test"], trunkRepo);
  git(["config", "commit.gpgsign", "false"], trunkRepo);

  writeFileSync(join(trunkRepo, "base.txt"), "base\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "-m", "base"], trunkRepo);
  const STALE_SHA = git(["rev-parse", "HEAD"], trunkRepo); // the ledger's now-rewritten pin

  writeFileSync(join(trunkRepo, "base.txt"), "rewritten\n");
  git(["add", "-A"], trunkRepo);
  git(["commit", "-q", "--amend", "-m", "rewritten base"], trunkRepo);
  // main's tip is now a DIFFERENT commit object at the same position;
  // STALE_SHA is neither an ancestor nor the tip itself.

  const dag = write("dag-trunkrewrite.toml", DAG1);
  const state = write(
    "state-trunkrewrite.toml",
    header({ current: "A0", repoSha: STALE_SHA }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live", [], { cwd: trunkRepo });
  expectCheck(
    PS_FILE,
    "is not an ancestor of the live tip of the configured integration-trunk ref",
    r,
  );
  rmSync(trunkRepo, { recursive: true, force: true });
});

test("[PS] rehearsal single-parent-commit discriminator: git commit-tree is invoked with exactly one -p per step", () => {
  // A dedicated shim (distinct from fakeGitEnv, which only breaks a whole
  // subcommand) that intercepts every real `commit-tree` invocation and
  // FAILS LOUDLY the moment it observes anything other than EXACTLY one
  // `-p` flag — the single-parent-commit invariant Finding A requires (a
  // second parent silently restores the two-parent MERGE COMMIT semantics
  // round 2 rejected). Running the real, unmutated validator through this
  // shim and asserting a clean PASS is the discriminator: it empirically
  // proves every commit-tree call in the current rehearsal is single-parent
  // (would FAIL the instant that regresses to two parents), not merely that
  // the source reads that way today.
  const shimDir = mkdtempSync(join(tmpdir(), "validate-mutation-suite-pcount-"));
  const shimScript = join(shimDir, "git");
  writeFileSync(
    shimScript,
    `#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const args = process.argv.slice(2);
if (args[0] === "commit-tree") {
  const pCount = args.filter((a) => a === "-p").length;
  if (pCount !== 1) {
    process.stderr.write(
      "shim: commit-tree invoked with " + pCount + " -p flag(s), expected exactly 1\\n",
    );
    process.exit(42);
  }
}
const res = spawnSync(process.env.FAKE_GIT_REAL, args, { stdio: "inherit" });
process.exit(res.status === null ? 1 : res.status);
`,
    "utf8",
  );
  chmodSync(shimScript, 0o755);

  const dag = write("dag-pcount.toml", TWO_CHILD_DAG);
  const state = write(
    "state-pcount.toml",
    header({ current: "A0", dagDigest: TWO_CHILD_DAG_DIGEST }) +
      rootAccepted() +
      "\n" +
      block("A0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_A,
        implementation_ref: CONCURRENT_A_REF,
        base_sha: SHA_BASE,
        landing_order: 1,
      }) +
      "\n" +
      block("B0", "IN_PROGRESS", {
        implementation_candidate_sha: CONCURRENT_B,
        implementation_ref: CONCURRENT_B_REF,
        base_sha: SHA_BASE,
        landing_order: 2,
      }),
  );
  const r = runPS(dag, state, "live", [], {
    env: { ...process.env, PATH: `${shimDir}:${process.env.PATH}`, FAKE_GIT_REAL: realGitPath },
  });
  assert.equal(
    r.status,
    0,
    `expected the rehearsal to pass with exactly one -p per commit-tree call, got:\n${r.err}\n${r.out}`,
  );
  assert.doesNotMatch(
    r.err,
    /shim: commit-tree invoked with/,
    "the single-parent shim fired — commit-tree was invoked with something other than exactly one -p",
  );
  rmSync(shimDir, { recursive: true, force: true });
});

// =====================================================================
// PROGRAM-STATE — live-mode placeholder / shape scans
// =====================================================================

test("[PS] live state still carries a top-level REQUIRED_ placeholder", () => {
  const dag = write("dag-shape1.toml", DAG1);
  const state = write(
    "state-shape1.toml",
    header({ current: "A0", extraTop: 'mutation_probe = "REQUIRED_PROBE"\n' }) +
      block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "live state still carries template placeholder ${where} =", r);
});

test("[PS] live state carries a REQUIRED_ placeholder inside an array", () => {
  const dag = write("dag-shape2.toml", DAG1);
  const state = write(
    "state-shape2.toml",
    header({ current: "A0", extraTop: 'mutation_probe_list = ["REQUIRED_PROBE"]\n' }) +
      block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "${where}[${idx}] =", r);
});

test("[PS] live state field ending _sha is malformed", () => {
  const dag = write("dag-shape3.toml", DAG1);
  const state = write(
    "state-shape3.toml",
    header({ current: "A0", extraTop: 'mutation_probe_sha = "not-hex"\n' }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "not a 40-char lowercase hex object id or empty", r);
});

test("[PS] live state field ending _digest is malformed", () => {
  const dag = write("dag-shape4.toml", DAG1);
  const state = write(
    "state-shape4.toml",
    header({ current: "A0", extraTop: 'mutation_probe_digest = "not-hex"\n' }) +
      block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "not a 64-char lowercase hex digest or empty", r);
});

// =====================================================================
// PROGRAM-STATE — orchestration.evidence_root(s) / evidence_digest binding
// =====================================================================

test("[PS] orchestration.evidence_roots is not an array", () => {
  const dag = write("dag-ev1.toml", DAG1);
  const state = write(
    "state-ev1.toml",
    header({
      current: "A0",
      orchestration: '\n[orchestration]\nevidence_roots = "not-an-array"\n',
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "evidence_roots is not an array", r);
});

test("[PS] orchestration declares both evidence_root and evidence_roots", () => {
  const dag = write("dag-ev2.toml", DAG1);
  const state = write(
    "state-ev2.toml",
    header({
      current: "A0",
      orchestration: `\n[orchestration]\nevidence_root = "some/path"\nevidence_roots = ["some/path"]\n`,
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "declares both evidence_root", r);
});

test("[PS] orchestration.evidence_root is not a string", () => {
  const dag = write("dag-ev3.toml", DAG1);
  const state = write(
    "state-ev3.toml",
    header({ current: "A0", orchestration: "\n[orchestration]\nevidence_root = 5\n" }) +
      block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "evidence_root is not a string:", r);
});

test("[PS] orchestration.evidence_roots[i] is not a non-empty string", () => {
  const dag = write("dag-ev4.toml", DAG1);
  const state = write(
    "state-ev4.toml",
    header({ current: "A0", orchestration: '\n[orchestration]\nevidence_roots = [""]\n' }) +
      block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "orchestration.${field} is not a non-empty string", r);
});

test("[PS] orchestration.evidence_root is not a resolvable directory", () => {
  const dag = write("dag-ev5.toml", DAG1);
  const state = write(
    "state-ev5.toml",
    header({
      current: "A0",
      orchestration: `\n[orchestration]\nevidence_root = "${join(dir, "does-not-exist")}"\n`,
    }) + block("A0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "is not a resolvable directory — evidence_digest bindings cannot be verified",
    r,
  );
});

test("[PS] evidence_digest resolves ambiguously (multiple nested artifacts)", () => {
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-evroot1-"));
  mkdirSync(join(evRoot, "A0", "reopen1"), { recursive: true });
  mkdirSync(join(evRoot, "A0", "reopen2"), { recursive: true });
  writeFileSync(join(evRoot, "A0", "reopen1", "landing-record.md"), "one\n");
  writeFileSync(join(evRoot, "A0", "reopen2", "landing-record.md"), "two\n");
  const dag = write("dag-ev6.toml", DAG1);
  const state = write(
    "state-ev6.toml",
    header({ current: "A0", orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n` }) +
      block("A0", "LOCKED", { evidence_digest: DIGEST }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "multiple nested evidence artifacts resolve for it, ambiguous", r);
  rmSync(evRoot, { recursive: true, force: true });
});

test("[PS] evidence_digest has no matching artifact under the declared root", () => {
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-evroot2-"));
  const dag = write("dag-ev7.toml", DAG1);
  const state = write(
    "state-ev7.toml",
    header({ current: "A0", orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n` }) +
      block("A0", "LOCKED", { evidence_digest: DIGEST }),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "no evidence artifact under", r);
  rmSync(evRoot, { recursive: true, force: true });
});

test("[PS] evidence_digest content does not match the resolved artifact", () => {
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-evroot3-"));
  writeFileSync(join(evRoot, "A0-summary.md"), "the real evidence content\n");
  const dag = write("dag-ev8.toml", DAG1);
  const state = write(
    "state-ev8.toml",
    header({ current: "A0", orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n` }) +
      block("A0", "LOCKED", { evidence_digest: DIGEST2 }), // wrong digest, real file exists
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "does not match the SHA-256 of ${artifact}", r);
  rmSync(evRoot, { recursive: true, force: true });
});

// =====================================================================
// PROGRAM-STATE — entry-lock RECORD content binding
// (verifyEntryLockRecordBinding, AMD-013 ratification correction 1)
//
// verifyEntryLockIdentity (covered above) only cross-checks repository.
// branch/head_sha/head_tree/entry_checkout_sha/entry_checkout_tree against
// EACH OTHER — every one an equally mutable field on the SAME in-memory
// ledger. verifyEntryLockRecordBinding additionally binds them to the DAG
// root's digest-bound entry-lock.toml RECORD, a separate file the same edit
// cannot also rewrite. The coordinated-mutation test below is the actual
// discriminator this correction was written to close: it rewrites all five
// fields IN LOCKSTEP to a different, but still internally self-consistent,
// checkout identity — exactly the shape that passed verifyEntryLockIdentity
// alone (64/0) before this correction.
// =====================================================================

test("[PS] entry_lock_digest does not match the SHA-256 of the resolved entry-lock record", () => {
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-entrylock1-"));
  mkdirSync(join(evRoot, "A0"), { recursive: true });
  writeFileSync(
    join(evRoot, "A0", "entry-lock.toml"),
    `[repository]\nbranch = "main"\nentry_checkout_sha = "${SHA}"\nentry_checkout_tree = "${SHA}"\n`,
  );
  const dag = write("dag-entrylock1.toml", DAG1);
  const state = write(
    "state-entrylock1.toml",
    header({
      current: "A0",
      repoSha: SHA,
      orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n`,
    }) + block("A0", "LOCKED", { entry_lock_digest: DIGEST2 }), // wrong digest, real record exists
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "does not match the SHA-256 of ${artifactPath}", r);
  rmSync(evRoot, { recursive: true, force: true });
});

test("[PS] resolved entry-lock record is not valid TOML", () => {
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-entrylock2-"));
  mkdirSync(join(evRoot, "A0"), { recursive: true });
  const badToml = "this is not [ valid toml\n";
  writeFileSync(join(evRoot, "A0", "entry-lock.toml"), badToml);
  const badTomlDigest = createHash("sha256").update(badToml).digest("hex");
  const dag = write("dag-entrylock2.toml", DAG1);
  const state = write(
    "state-entrylock2.toml",
    header({
      current: "A0",
      repoSha: SHA,
      orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n`,
    }) + block("A0", "LOCKED", { entry_lock_digest: badTomlDigest }), // digest matches — bytes don't parse
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "entry-lock record ${artifactPath} is not valid TOML", r);
  rmSync(evRoot, { recursive: true, force: true });
});

test("[PS] coordinated rewrite of all five entry-lock identity fields still fails against the digest-bound record", () => {
  // The exact attack this correction closes: rewrite repository.branch,
  // repository.head_sha, repository.head_tree, entry_checkout_sha, and
  // entry_checkout_tree IN LOCKSTEP to a different but still mutually
  // self-consistent checkout — verifyEntryLockIdentity's own cross-checks
  // (each field against the OTHERS) all still pass, since none of them
  // change relative to each other. Only the binding against the separate,
  // digest-pinned entry-lock.toml record — which the mutation does not and
  // cannot also rewrite — catches it.
  const evRoot = mkdtempSync(join(tmpdir(), "validate-mutation-suite-entrylock3-"));
  mkdirSync(join(evRoot, "A0"), { recursive: true });
  const recordBytes = `[repository]\nbranch = "main"\nentry_checkout_sha = "${SHA}"\nentry_checkout_tree = "${SHA}"\n`;
  writeFileSync(join(evRoot, "A0", "entry-lock.toml"), recordBytes);
  const entryLockDigest = createHash("sha256").update(recordBytes).digest("hex");

  const dag = write("dag-entrylock3.toml", DAG1);
  const base =
    header({
      current: "A0",
      repoSha: SHA,
      orchestration: `\n[orchestration]\nevidence_root = "${evRoot}"\n`,
    }) + block("A0", "LOCKED", { entry_lock_digest: entryLockDigest });
  const mutated = applied(
    base,
    base
      .replace(/branch = "main"/, 'branch = "other-branch"')
      .replace(/entry_checkout_sha = "[0-9a-f]{40}"/, `entry_checkout_sha = "${SHA_BASE}"`)
      .replace(/entry_checkout_tree = "[0-9a-f]{40}"/, `entry_checkout_tree = "${SHA_BASE}"`)
      .replace(/head_sha = "[0-9a-f]{40}"/, `head_sha = "${SHA_BASE}"`)
      .replace(/head_tree = "[0-9a-f]{40}"/, `head_tree = "${SHA_BASE}"`),
    "coordinated rewrite of branch/head_sha/head_tree/entry_checkout_sha/entry_checkout_tree to a different, self-consistent checkout",
  );
  const state = write("state-entrylock3.toml", mutated);
  const r = runPS(dag, state, "live");
  expectCheck(
    PS_FILE,
    "the immutable A0 entry lock must match its own digest-bound record, not merely its own other, equally mutable, ledger fields",
    r,
  );
  rmSync(evRoot, { recursive: true, force: true });
});

// =====================================================================
// VALIDATE-STACK-WINDOW.MJS — the two checks it owns directly
// =====================================================================

const SW_DIGEST = createHash("sha256").update("mutation-suite-sw-digest").digest("hex");
const SW_SHA = "0000000000000000000000000000000000000001";

function layerText(overrides = {}) {
  const fields = {
    index: 1,
    layer_id: "L1",
    block_id: "B1",
    charter_digest: SW_DIGEST,
    kind: "mergeable",
    branch: "b1",
    base_branch: "main",
    worktree: "wt1",
    worker: "w1",
    pr_number: 0,
    pr_url: "",
    base_sha: SW_SHA,
    base_tree: SW_SHA,
    head_sha: "",
    head_tree: "",
    patch_digest: "",
    generated_digest: "",
    evidence_digest: "",
    ci_state: "PENDING",
    review_state: "PENDING",
    mergeable: true,
    notes: "",
    ...overrides,
  };
  const lines = Object.entries(fields).map(([k, val]) => {
    if (typeof val === "number" || typeof val === "boolean") return `${k} = ${val}`;
    return `${k} = "${val}"`;
  });
  return `[[layer]]\n${lines.join("\n")}\n`;
}

function windowText({
  mode = "ATOMIC_REVIEW",
  acceptanceBlockId = "",
  status = "ACTIVE",
  overrides = {},
  layers = [layerText()],
} = {}) {
  const fields = {
    schema: 1,
    revision: 11,
    status,
    mode,
    stack_id: "S1",
    acceptance_block_id: acceptanceBlockId,
    authority_package_digest: SW_DIGEST,
    implementation_lock_digest: SW_DIGEST,
    program_state_basis_digest: SW_DIGEST,
    previous_stack_snapshot_digest: "NOT_APPLICABLE",
    root_branch: "main",
    root_base_sha: SW_SHA,
    root_base_tree: SW_SHA,
    stack_tool: "LOCAL_BRANCH_CHAIN",
    stack_tool_version: "git 2.x",
    landing_mode: "bottom-up",
    max_open_layers: 4,
    owner: "orchestrator",
    evidence_root: "docs/arch/refactor/rev11/evidence",
    ...overrides,
  };
  const lines = Object.entries(fields).map(([k, val]) =>
    typeof val === "number" ? `${k} = ${val}` : `${k} = "${val}"`,
  );
  return (
    `${lines.join("\n")}\nshared_writer_surfaces = []\nintegration_commands = []\nnotes = ""\n\n` +
    layers.join("")
  );
}

test("[SW] --current-program-state requires --mode live", () => {
  const p = write("sw-cps1.toml", windowText({ mode: "LANDABLE" }));
  const state = write("sw-cps1-state.toml", `schema = 1\nrevision = 11\n`);
  const r = runSW(["--window", p, "--mode", "template", "--current-program-state", state]);
  expectCheck(SW_FILE, "--current-program-state was given but --mode is", r);
});

test("[SW] --current-program-state cross-validation skipped after structural failure", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(base, base.replace(/^schema = 1\n/, ""), "drop schema key");
  const p = write("sw-cps2.toml", mutated);
  const state = write("sw-cps2-state.toml", `schema = 1\nrevision = 11\n`);
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SW_FILE, "cross-validation skipped —", r);
});

// =====================================================================
// STACK-WINDOW-LIB — structural rules, exercised via validate-stack-window.mjs
// =====================================================================

test("[SWL] missing required top-level key", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(base, base.replace(/^schema = 1\n/, ""), "drop schema key");
  const p = write("swl-1.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "is missing required top-level key", r);
});

test("[SWL] top-level string field is empty", () => {
  const p = write("swl-2.toml", windowText({ mode: "LANDABLE", overrides: { owner: "" } }));
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level ${key} is not a non-empty string:", r);
});

test("[SWL] acceptance_block_id is not a string", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(
    base,
    base.replace('acceptance_block_id = ""', "acceptance_block_id = 5"),
    "non-string acceptance_block_id",
  );
  const p = write("swl-3.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level acceptance_block_id is not a string", r);
});

test("[SWL] shared_writer_surfaces is not an array", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(
    base,
    base.replace("shared_writer_surfaces = []", 'shared_writer_surfaces = "x"'),
    "non-array shared_writer_surfaces",
  );
  const p = write("swl-4.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level shared_writer_surfaces is not an array", r);
});

test("[SWL] integration_commands is not an array", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(
    base,
    base.replace("integration_commands = []", 'integration_commands = "x"'),
    "non-array integration_commands",
  );
  const p = write("swl-5.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level integration_commands is not an array", r);
});

test("[SWL] top-level notes is not a string", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(base, base.replace('notes = ""', "notes = 5"), "non-string notes");
  const p = write("swl-6.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level notes is not a string", r);
});

test("[SWL] unknown top-level mode value", () => {
  const p = write(
    "swl-7.toml",
    windowText({ mode: "LANDABLE", overrides: { mode: "SOMETHING_ELSE" } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "outside the declared enum {LANDABLE, ATOMIC_REVIEW}", r);
});

test("[SWL] live window still carries status = TEMPLATE", () => {
  const p = write("swl-8.toml", windowText({ mode: "LANDABLE", status: "TEMPLATE" }));
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "live window still carries status =", r);
});

test("[SWL] top-level digest field malformed", () => {
  const p = write(
    "swl-9.toml",
    windowText({ mode: "LANDABLE", overrides: { authority_package_digest: "not-a-digest" } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "top-level ${field} is not a ", r);
});

test("[SWL] previous_stack_snapshot_digest malformed", () => {
  const p = write(
    "swl-10.toml",
    windowText({ mode: "LANDABLE", overrides: { previous_stack_snapshot_digest: "bogus" } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "previous_stack_snapshot_digest is not", r);
});

test("[SWL] root_base_sha malformed", () => {
  const p = write(
    "swl-11.toml",
    windowText({ mode: "LANDABLE", overrides: { root_base_sha: "not-a-sha" } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(
    SWL_FILE,
    "top-level root_base_sha is not a resolved 40-char lowercase git object id:",
    r,
  );
});

test("[SWL] root_base_tree malformed", () => {
  const p = write(
    "swl-12.toml",
    windowText({ mode: "LANDABLE", overrides: { root_base_tree: "not-a-sha" } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(
    SWL_FILE,
    "top-level root_base_tree is not a resolved 40-char lowercase tree object id:",
    r,
  );
});

test("[SWL] max_open_layers outside [2, 6]", () => {
  const p = write(
    "swl-13.toml",
    windowText({ mode: "LANDABLE", overrides: { max_open_layers: 7 } }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "is not an integer in [2, 6]", r);
});

test("[SWL] zero layers declared", () => {
  const base = windowText({ mode: "LANDABLE" });
  const mutated = applied(base, base.replace(/\n\[\[layer\]\][\s\S]*$/, "\n"), "strip all layers");
  const p = write("swl-14.toml", mutated);
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "declares no [[layer]] entries", r);
});

test("[SWL] layer count exceeds max_open_layers", () => {
  const p = write(
    "swl-15.toml",
    windowText({
      mode: "LANDABLE",
      overrides: { max_open_layers: 2 },
      layers: [
        layerText({ index: 1, layer_id: "L1", block_id: "B1" }),
        layerText({ index: 2, layer_id: "L2", block_id: "B2" }),
        layerText({ index: 3, layer_id: "L3", block_id: "B3" }),
      ],
    }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "exceeding its own max_open_layers =", r);
});

test("[SWL] layer required string field is empty", () => {
  const p = write(
    "swl-16.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ worker: "" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "${where} ${field} is not a non-empty string:", r);
});

test("[SWL] layer optional string field is not a string", () => {
  const p = write(
    "swl-17.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ notes: 5 })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "${where} ${field} is not a string:", r);
});

test("[SWL] duplicate layer_id", () => {
  const p = write(
    "swl-18.toml",
    windowText({
      mode: "LANDABLE",
      layers: [
        layerText({ index: 1, layer_id: "L1", block_id: "B1" }),
        layerText({ index: 2, layer_id: "L1", block_id: "B2" }),
      ],
    }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "declares duplicate layer_id", r);
});

test("[SWL] layer index is not a positive integer", () => {
  const p = write(
    "swl-19.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ index: 0 })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "index is not a positive integer:", r);
});

test("[SWL] duplicate layer index", () => {
  const p = write(
    "swl-20.toml",
    windowText({
      mode: "LANDABLE",
      layers: [
        layerText({ index: 1, layer_id: "L1", block_id: "B1" }),
        layerText({ index: 1, layer_id: "L2", block_id: "B2" }),
      ],
    }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "declares duplicate layer index", r);
});

test("[SWL] layer kind outside the declared enum", () => {
  const p = write(
    "swl-21.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ kind: "BOGUS" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "outside the declared enum {mergeable, NON_MERGEABLE_PRIVATE_LAYER}", r);
});

test("[SWL] pr_number is not a non-negative integer", () => {
  const p = write(
    "swl-22.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ pr_number: -1 })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "pr_number is not a non-negative integer:", r);
});

test("[SWL] mergeable is not a boolean", () => {
  const p = write(
    "swl-23.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ mergeable: "notabool" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "mergeable is not a boolean:", r);
});

test("[SWL] layer charter_digest malformed", () => {
  const p = write(
    "swl-24.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ charter_digest: "not-a-digest" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "${where} charter_digest is not a resolved SHA-256", r);
});

test("[SWL] layer base_sha malformed", () => {
  const p = write(
    "swl-25.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ base_sha: "not-a-sha" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "40-char lowercase git object id or empty", r);
});

test("[SWL] layer base_tree malformed", () => {
  const p = write(
    "swl-26.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ base_tree: "not-a-sha" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "tree object id or empty", r);
});

test("[SWL] layer patch_digest malformed", () => {
  const p = write(
    "swl-27.toml",
    windowText({ mode: "LANDABLE", layers: [layerText({ patch_digest: "not-a-digest" })] }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "64-char lowercase SHA-256 or empty", r);
});

test("[SWL] LANDABLE mode with non-empty acceptance_block_id", () => {
  const p = write("swl-28.toml", windowText({ mode: "LANDABLE", acceptanceBlockId: "B1" }));
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "mode is LANDABLE but acceptance_block_id is non-empty", r);
});

test("[SWL] LANDABLE mode with a duplicate block_id across layers", () => {
  const p = write(
    "swl-29.toml",
    windowText({
      mode: "LANDABLE",
      layers: [
        layerText({ index: 1, layer_id: "L1", block_id: "B1" }),
        layerText({ index: 2, layer_id: "L2", block_id: "B1" }),
      ],
    }),
  );
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "mode is LANDABLE but block_id", r);
});

function atomicWindow({
  acceptanceBlockId = "D2",
  d1Kind = "NON_MERGEABLE_PRIVATE_LAYER",
  d2Kind = "mergeable",
  d1Index = 1,
  d2Index = 2,
  overrides = {},
} = {}) {
  return windowText({
    mode: "ATOMIC_REVIEW",
    acceptanceBlockId,
    overrides: { max_open_layers: 2, ...overrides },
    layers: [
      layerText({
        index: d1Index,
        layer_id: "D1",
        block_id: "D1",
        kind: d1Kind,
        mergeable: d1Kind === "mergeable",
        base_branch: "main",
        branch: "d1",
      }),
      layerText({
        index: d2Index,
        layer_id: "D2",
        block_id: "D2",
        kind: d2Kind,
        mergeable: d2Kind === "mergeable",
        base_branch: "d1",
        branch: "d2",
      }),
    ],
  });
}

test("[SWL] ATOMIC_REVIEW with empty acceptance_block_id", () => {
  const p = write("swl-30.toml", atomicWindow({ acceptanceBlockId: "" }));
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, "mode is ATOMIC_REVIEW but acceptance_block_id is empty", r);
});

test("[SWL] ATOMIC_REVIEW with two mergeable layers", () => {
  const p = write("swl-31.toml", atomicWindow({ d1Kind: "mergeable" }));
  const r = runSW(["--window", p, "--mode", "live"]);
  expectCheck(SWL_FILE, 'does not have exactly one layer with kind = "mergeable"', r);
  expectCheck(SWL_FILE, "is not the acceptance block but kind is", r);
});

test("[SWL] --dag: private layer names a non-checkpoint block", () => {
  const dag = write(
    "swl-dag-32.toml",
    `schema = 1\nrevision = 11\n\n[[block]]\nid = "D1"\nclass = "foundational"\npredecessors = []\n\n[[block]]\nid = "D2"\nclass = "foundational"\npredecessors = ["D1"]\n`,
  );
  const p = write("swl-32.toml", atomicWindow());
  const r = runSW(["--window", p, "--mode", "live", "--dag", dag]);
  expectCheck(
    SWL_FILE,
    "a private ATOMIC_REVIEW layer must repeat the acceptance block's own id",
    r,
  );
});

function ledgerText({
  d1Snapshot,
  d2Snapshot,
  d1StackId = "S1",
  d2StackId = "S1",
  d1Layer = 1,
  d2Layer = 2,
  d1Status = "PRIVATE_CHECKPOINT",
} = {}) {
  const row = (id, status, stackId, snapshot, layerIdx) =>
    `[[block]]\nid = "${id}"\nstatus = "${status}"\nstack_id = "${stackId}"\nstack_snapshot_digest = "${snapshot}"\nstack_layer = ${layerIdx}\n`;
  return `schema = 1\nrevision = 11\n\n${row("D1", d1Status, d1StackId, d1Snapshot, d1Layer)}\n${row("D2", "REVIEW", d2StackId, d2Snapshot, d2Layer)}\n`;
}

test("[SWL] --current-program-state: block missing from ledger", () => {
  const p = write("swl-33.toml", atomicWindow());
  const state = write("swl-33-state.toml", `schema = 1\nrevision = 11\n`);
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SWL_FILE, "does not exist in the program-state ledger", r);
});

test("[SWL] --current-program-state: stack_id mismatch", () => {
  const p = write("swl-34.toml", atomicWindow());
  const snap = createHash("sha256").update(readFileSync(p)).digest("hex");
  const state = write(
    "swl-34-state.toml",
    ledgerText({ d1Snapshot: snap, d2Snapshot: snap, d1StackId: "OTHER" }),
  );
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SWL_FILE, "does not match window stack_id", r);
});

test("[SWL] --current-program-state: stack_snapshot_digest mismatch", () => {
  const p = write("swl-35.toml", atomicWindow());
  const state = write("swl-35-state.toml", ledgerText({ d1Snapshot: DIGEST, d2Snapshot: DIGEST2 }));
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SWL_FILE, "does not match the SHA-256 of the validated stack-window file", r);
});

test("[SWL] --current-program-state: stack_layer mismatch", () => {
  const p = write("swl-36.toml", atomicWindow());
  const snap = createHash("sha256").update(readFileSync(p)).digest("hex");
  const state = write(
    "swl-36-state.toml",
    ledgerText({ d1Snapshot: snap, d2Snapshot: snap, d1Layer: 5 }),
  );
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SWL_FILE, "does not match window layer index", r);
});

test("[SWL] --current-program-state: checkpoint layer not PRIVATE_CHECKPOINT", () => {
  const p = write("swl-37.toml", atomicWindow());
  const snap = createHash("sha256").update(readFileSync(p)).digest("hex");
  const state = write(
    "swl-37-state.toml",
    ledgerText({ d1Snapshot: snap, d2Snapshot: snap, d1Status: "ACCEPTED" }),
  );
  const r = runSW(["--window", p, "--mode", "live", "--current-program-state", state]);
  expectCheck(SWL_FILE, "a checkpoint layer never lands independently", r);
});

// =====================================================================
// STACK-WINDOW-LIB — evaluateCheckpointException, via
// validate-program-state.mjs's --stack-window flag (the D1/D2 wiring)
// =====================================================================

function cpWindowText({
  acceptanceId = "A2",
  a1Kind = "NON_MERGEABLE_PRIVATE_LAYER",
  a1Index = 1,
  a2Index = 2,
} = {}) {
  return atomicWindow({
    acceptanceBlockId: acceptanceId,
    d1Kind: a1Kind,
    d1Index: a1Index,
    d2Index: a2Index,
  })
    .replaceAll('block_id = "D1"', 'block_id = "A1"')
    .replaceAll('layer_id = "D1"', 'layer_id = "A1"')
    .replaceAll('block_id = "D2"', 'block_id = "A2"')
    .replaceAll('layer_id = "D2"', 'layer_id = "A2"');
}

test("[SWL via PS] window mode is not ATOMIC_REVIEW", () => {
  const dag = write("dag-d1d2-1.toml", DAG3_CP);
  const windowText_ = cpWindowText()
    .replace('mode = "ATOMIC_REVIEW"', 'mode = "LANDABLE"')
    .replace('acceptance_block_id = "A2"', 'acceptance_block_id = ""');
  applied(cpWindowText(), windowText_, "mode -> LANDABLE");
  const windowPath = write("window-d1d2-1.toml", windowText_);
  const state = write(
    "state-d1d2-1.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 1 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 2,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", ["--stack-window", windowPath]);
  expectCheck(
    SWL_FILE,
    "not ATOMIC_REVIEW — a PRIVATE_CHECKPOINT predecessor is legalized only",
    r,
  );
});

test("[SWL via PS] window acceptance_block_id names a different block", () => {
  const dag = write("dag-d1d2-2.toml", DAG3_CP);
  const windowText_ = cpWindowText({ acceptanceId: "SOMETHING_ELSE" });
  const windowPath = write("window-d1d2-2.toml", windowText_);
  const snap = createHash("sha256").update(windowText_).digest("hex");
  const state = write(
    "state-d1d2-2.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: snap,
        stack_layer: 2,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", ["--stack-window", windowPath]);
  expectCheck(PS_FILE, "did not establish the checkpoint exception", r);
  expectCheck(SWL_FILE, "the exception is granted", r);
});

test("[SWL via PS] window declares no layer for the predecessor block", () => {
  const dag = write("dag-d1d2-3.toml", DAG3_CP);
  const base = cpWindowText();
  const windowText_ = applied(
    base,
    base.replace('block_id = "A1"', 'block_id = "RENAMED"'),
    "rename A1 layer's block_id",
  );
  const windowPath = write("window-d1d2-3.toml", windowText_);
  const snap = createHash("sha256").update(windowText_).digest("hex");
  const state = write(
    "state-d1d2-3.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: snap,
        stack_layer: 2,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", ["--stack-window", windowPath]);
  expectCheck(SWL_FILE, "declares no layer for predecessor block", r);
});

test("[SWL via PS] predecessor layer is independently mergeable", () => {
  const dag = write("dag-d1d2-4.toml", DAG3_CP);
  const windowText_ = cpWindowText({ a1Kind: "mergeable" });
  const windowPath = write("window-d1d2-4.toml", windowText_);
  const snap = createHash("sha256").update(windowText_).digest("hex");
  const state = write(
    "state-d1d2-4.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: snap,
        stack_layer: 2,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", ["--stack-window", windowPath]);
  expectCheck(
    SWL_FILE,
    "a checkpoint predecessor's layer must never be independently mergeable",
    r,
  );
});

test("[SWL via PS] predecessor layer index not below the acceptance layer index", () => {
  const dag = write("dag-d1d2-5.toml", DAG3_CP);
  // A1 (predecessor/checkpoint) at index 2, A2 (acceptance) at index 1 — reversed.
  const windowText_ = cpWindowText({ a1Index: 2, a2Index: 1 });
  const windowPath = write("window-d1d2-5.toml", windowText_);
  const snap = createHash("sha256").update(windowText_).digest("hex");
  const state = write(
    "state-d1d2-5.toml",
    header({ current: "A2", dagDigest: DAG3_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 2 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: snap,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", ["--stack-window", windowPath]);
  expectCheck(SWL_FILE, "is not below acceptance layer index", r);
});

// =====================================================================
// Coverage — the anti-rot assertion. Runs last (node:test executes tests
// declared in a single file sequentially, in declaration order).
// =====================================================================

test("coverage: every derived check was tripped by at least one mutation", () => {
  const uncovered = registry.uncovered();
  assert.equal(
    uncovered.length,
    0,
    `${uncovered.length} derived check(s) have NO mutation proving they fire:\n` +
      uncovered.map((c) => `  ${c.file}:${c.line}: ${c.literal}`).join("\n"),
  );
  assert.ok(
    registry.checks.length > 100,
    `expected a large derived inventory, got ${registry.checks.length} — extraction may be broken`,
  );
});
