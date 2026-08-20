// Tests for scripts/validate-program-state.mjs:
//   node --test scripts/validate-program-state.test.mjs
//
// Positives (template, live, legal stacking, NOT_REQUIRED, proven checkpoint)
// and negatives that assert both a non-zero exit and the specific violation
// text — including fail-closed PRIVATE_CHECKPOINT-predecessor rejection
// (stackless and with otherwise-perfect stack fields) and evidence_digest
// content binding (unresolvable evidence_root, mismatched/missing artifact).
// A validator stubbed to always exit 0 cannot pass.
//
// Git identity fixture: `before()` builds a REAL temporary git repository
// (gitRoot) with a genuine commit chain — SHA_BASE (root) -> SHA (tip, HEAD
// of the repo) — plus SHA_DANGLING, a commit made on a since-deleted branch
// off SHA_BASE: a real object, but unreachable from the tip and NOT an
// ancestor of it. This is the exact shape of the A5 defect this validator
// change fixes (a dangling commit recorded as an accepted identity). Every
// live-mode test in this file runs with cwd=gitRoot by default, so the
// pre-existing "happy path" fixtures (SHA/TREE below) now exercise the real
// git checks, not just the old regex shape check.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";

const VALIDATOR = join(dirname(fileURLToPath(import.meta.url)), "validate-program-state.mjs");

let dir;
let gitRoot; // real repo: SHA_BASE -> SHA (tip); SHA_DANGLING off SHA_BASE, unreachable from tip
let notGitDir; // plain directory, never `git init`-ed
let SHA_BASE, TREE_BASE;
let SHA, TREE; // the repo's tip commit + its real tree (replaces the old fake constant)
let SHA_DANGLING, TREE_DANGLING;
const SHA_NONEXISTENT = "abcdef1234567890abcdef1234567890abcdef12"; // well-formed, never committed

function git(args, cwd, input) {
  return execFileSync("git", args, { cwd, input, encoding: "utf8" }).trim();
}

before(() => {
  dir = mkdtempSync(join(tmpdir(), "validate-program-state-"));
  notGitDir = mkdtempSync(join(tmpdir(), "validate-program-state-nogit-"));

  gitRoot = mkdtempSync(join(tmpdir(), "validate-program-state-git-"));
  git(["init", "-q"], gitRoot);
  git(["symbolic-ref", "HEAD", "refs/heads/main"], gitRoot); // portable across init.defaultBranch configs
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

  // A real commit, made on its own branch off SHA_BASE, then orphaned by
  // deleting that branch — a loose object that exists but is reachable from
  // no ref. Sibling of SHA (not its ancestor), exactly the A5 shape.
  git(["checkout", "-q", "-b", "scratch", SHA_BASE], gitRoot);
  writeFileSync(join(gitRoot, "scratch.txt"), "scratch\n");
  git(["add", "-A"], gitRoot);
  git(["commit", "-q", "-m", "scratch"], gitRoot);
  SHA_DANGLING = git(["rev-parse", "HEAD"], gitRoot);
  TREE_DANGLING = git(["rev-parse", "HEAD^{tree}"], gitRoot);
  git(["checkout", "-q", "main"], gitRoot);
  git(["branch", "-D", "scratch"], gitRoot);
});

// Amendment authority gate fixtures. The validator resolves
// `<amendments-dir>/<AMD-ID>-*.md` relative to the DAG file's own directory
// (mirroring the real repo, where docs/arch/refactor/rev11/amendments/ is a
// sibling of docs/arch/refactor/rev11/program-dag.toml) — every DAG fixture
// below is written via write() into `dir`, so one shared `dir/amendments/`
// serves every test in this file.
let amendmentsDir;
function writeAmendmentFixtures() {
  amendmentsDir = join(dir, "amendments");
  mkdirSync(amendmentsDir, { recursive: true });
  writeFileSync(
    join(amendmentsDir, "AMD-900-not-ratified-fixture.md"),
    "# AMD-900 — test fixture\n\n**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.\n",
  );
  writeFileSync(
    join(amendmentsDir, "AMD-901-ratified-fixture.md"),
    "# AMD-901 — test fixture\n\n**Status:** RATIFIED (see §1). Landed at cafebabe12.\n",
  );
  // Deliberately no **Status:** line at all — the unparseable case.
  writeFileSync(
    join(amendmentsDir, "AMD-902-no-status-fixture.md"),
    "# AMD-902 — test fixture\n\nThis file never declares a Status field.\n",
  );
  // AMD-999 intentionally has no file at all — the not-found case.
}
before(writeAmendmentFixtures);
after(() => {
  rmSync(dir, { recursive: true, force: true });
  rmSync(gitRoot, { recursive: true, force: true });
  rmSync(notGitDir, { recursive: true, force: true });
});

function write(name, content) {
  const p = join(dir, name);
  writeFileSync(p, content, "utf8");
  return p;
}

// The authority-registry check (--authority) is covered by its own dedicated
// tests below. Every OTHER test in this file predates that check, exercises
// unrelated behavior, and never sets up an authority-registry.toml fixture —
// since --authority is now mandatory-by-default in live mode, those tests opt
// out via the explicit --no-authority escape unless the caller already named
// --authority or --no-authority itself.
function run(dagPath, statePath, mode, cwd, extraArgs = []) {
  const args =
    extraArgs.includes("--authority") || extraArgs.includes("--no-authority")
      ? extraArgs
      : [...extraArgs, "--no-authority"];
  const res = spawnSync(
    process.execPath,
    [VALIDATOR, "--dag", dagPath, "--state", statePath, "--mode", mode, ...args],
    {
      encoding: "utf8",
      cwd: cwd ?? gitRoot,
    },
  );
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "" };
}

const DAG = `schema = 1
revision = 11
entry_gate = "A2"
final_gate = "A2"

[[block]]
id = "A0"
name = "root"
class = "foundational"
predecessors = []

[[block]]
id = "A1"
name = "mid"
class = "foundational"
predecessors = ["A0"]

[[block]]
id = "A2"
name = "leaf"
class = "foundational"
predecessors = ["A0", "A1"]
`;

// Same shape as DAG but A1 is the plan's private-checkpoint class (the D1
// analogue), so PRIVATE_CHECKPOINT is class-legal on A1 here and nowhere else.
const DAG_CP = DAG.replace(
  'id = "A1"\nname = "mid"\nclass = "foundational"',
  'id = "A1"\nname = "mid"\nclass = "foundational-private-checkpoint"',
);
// Same shape as DAG but A1 is subsystem-class, where governance.md §2.2 permits
// architecture_review = "NOT_REQUIRED".
const DAG_SUB = DAG.replace(
  'id = "A1"\nname = "mid"\nclass = "foundational"',
  'id = "A1"\nname = "mid"\nclass = "subsystem"',
);

// SHA is now assigned in before() from the real gitRoot fixture (see above).
const SHA2 = "0000000000000000000000000000000000000001";
const DIGEST = "68c2140d3be29de0b8737771aa80d30c17be7cf55aa249a7cfaa3b47f384cd21";
const DIGEST2 = "1111111111111111111111111111111111111111111111111111111111111111";
// The base DAG's real SHA-256 — a legal live ledger MUST bind the digest of the
// DAG it validates against (an empty program_dag_digest silently disables the
// ledger-to-DAG binding and is a live-mode violation). The two class variants
// bind their own digests. The replaces above are asserted to have applied so a
// drifted stanza cannot silently turn the class-variant tests into base-DAG runs.
if (DAG_CP === DAG || DAG_SUB === DAG) throw new Error("DAG class-variant replace did not apply");
const DAG_DIGEST = createHash("sha256").update(DAG).digest("hex");
const DAG_CP_DIGEST = createHash("sha256").update(DAG_CP).digest("hex");
const DAG_SUB_DIGEST = createHash("sha256").update(DAG_SUB).digest("hex");

// One ledger block row. `overrides` replaces the listed field lines verbatim.
function block(id, status, overrides = {}) {
  const fields = {
    charter_digest: "",
    context_packet_digest: "",
    base_sha: "",
    candidate_sha: "",
    candidate_tree: "",
    accepted_sha: "",
    accepted_tree: "",
    landing_equivalence_digest: "",
    evidence_digest: "",
    stack_id: "",
    stack_snapshot_digest: "",
    stack_layer: 0,
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

// A fully-legal ACCEPTED row: exact identities, all three mandates PASS,
// recorded maintainer acceptance, an accepted identity equal to the reviewed
// candidate identity, and — although equality means no landing-equivalence
// artifact is REQUIRED (governance.md:283 demands one only when the accepted
// identity diverges) — an explicitly bound landing_equivalence_digest, so the
// fixture is legal under both readings. entry_lock_digest is bound because the
// DAG root (A0 in every fixture DAG) requires it at the gated statuses; on a
// non-root row the field is a harmless well-formed extra.
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

// A fully-proven PRIVATE_CHECKPOINT row: exact identities, charter/context/
// evidence digests, all three mandates PASS — and deliberately NO accepted
// identity and NO maintainer acceptance (a checkpoint never merges or releases
// independently, program.md §7, so there is no accepted landing to record).
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

function header({ status, current, repoSha, dagDigest = DAG_DIGEST, evidenceRoot, evidenceRoots }) {
  let orchestration = "";
  if (evidenceRoots !== undefined) {
    const list = evidenceRoots.map((r) => `"${r}"`).join(", ");
    orchestration = `
[orchestration]
evidence_roots = [${list}]
`;
  } else if (evidenceRoot !== undefined) {
    orchestration = `
[orchestration]
evidence_root = "${evidenceRoot}"
`;
  }
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

[repository]
remote = "https://example.invalid/repo"
branch = "main"
head_sha = "${repoSha}"
head_tree = "${repoSha}"
dirty = false
untracked_count = 0
${orchestration}`;
}

test("template mode: template-shaped state with REQUIRED_ placeholders passes", () => {
  const dag = write("dag-ok.toml", DAG);
  // Placeholders and a TEMPLATE status are EXPECTED in template mode and must
  // not be reported as errors.
  const state = write(
    "state-template.toml",
    `schema = 1
revision = 11
status = "TEMPLATE"
authority_package_digest = "REQUIRED_PACKAGE_DIGEST"
current_block = "A0"

[repository]
remote = "REQUIRED_REMOTE"
branch = "REQUIRED_BRANCH"
head_sha = "REQUIRED_FULL_SHA"
head_tree = "REQUIRED_TREE_OID"
dirty = false
untracked_count = 0

` +
      block("A0", "READY") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "template");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /^OK: state-template\.toml /);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("live mode: fully-resolved state with a legal ACCEPTED predecessor and IN_PROGRESS block passes", () => {
  const dag = write("dag-ok2.toml", DAG);
  const state = write(
    "state-live-ok.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("sequencing invariant: IN_PROGRESS with an unaccepted predecessor is rejected", () => {
  const dag = write("dag-seq.toml", DAG);
  // A1 IN_PROGRESS while its direct predecessor A0 is only READY — the core
  // governance.md sequencing-authority violation.
  const state = write(
    "state-seq-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      block("A0", "READY") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "sequencing violation must fail");
  assert.match(
    r.err,
    /sequencing violation .*block A1 is IN_PROGRESS but direct predecessor\(s\) not ACCEPTED: \[A0\]/,
  );
});

test("stacked-work exception: a bare stack_id with no established stack is REJECTED", () => {
  const dag = write("dag-stack.toml", DAG);
  // A1 REVIEW claims the contingent stacked-work exception with only a
  // non-empty stack_id: no snapshot digest, layer 0, and its unaccepted
  // predecessor A0 is stackless. governance.md:6 requires "the same validated
  // immutable stack snapshot" — none of that can be established here.
  const state = write(
    "state-stack-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
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
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "unestablished stacked-work exception must fail");
  assert.match(
    r.err,
    /block A1 is REVIEW with unaccepted direct predecessor\(s\) \[A0\] and the contingent stacked-work exception is REJECTED/,
  );
  assert.match(r.err, /stack_snapshot_digest .* is not a 64-char lowercase SHA-256/);
  assert.match(r.err, /unaccepted predecessor A0 does not carry the same non-empty stack_id "S1"/);
});

test("status gate: ACCEPTED with PENDING mandates and no accepted identity is rejected", () => {
  const dag = write("dag-acc.toml", DAG);
  // A0 marked ACCEPTED while every review mandate and maintainer_decision is
  // still PENDING and accepted_sha/accepted_tree are empty — the transition
  // governance.md:181 exists to gate.
  const state = write(
    "state-acc-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "ACCEPTED") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "unreviewed ACCEPTED must fail");
  assert.match(r.err, /state block A0 is ACCEPTED but conformance_review is "PENDING"/);
  assert.match(
    r.err,
    /state block A0 is ACCEPTED but accepted_sha is not a non-empty 40-char lowercase git object id/,
  );
  assert.match(r.err, /state block A0 is ACCEPTED but maintainer_decision is "PENDING"/);
});

test("strict TOML reader: unbalanced quoting is a loud parse failure, never a silent mis-read", () => {
  const dag = write("dag-toml.toml", DAG);
  // Case 1: `status = "ACT"#IVE"` must NOT silently parse as "ACT".
  const state1 = write(
    "state-toml-bad1.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }).replace(
      'status = "ACTIVE"',
      'status = "ACT"#IVE"',
    ) +
      block("A0", "IN_PROGRESS") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r1 = run(dag, state1, "live");
  assert.notEqual(r1.status, 0, "unbalanced quotes must fail loudly");
  assert.match(r1.err, /unparseable TOML/);
  assert.match(r1.err, /trailing comment after string contains a double-quote/);

  // Case 2: `release_report_digest = ""#REQUIRED_RELEASE_REPORT_DIGEST"` must
  // NOT parse as an empty string (which would bypass the live-mode REQUIRED_
  // placeholder scan).
  const state2 = write(
    "state-toml-bad2.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }).replace(
      'release_report_digest = ""',
      'release_report_digest = ""#REQUIRED_RELEASE_REPORT_DIGEST"',
    ) +
      block("A0", "IN_PROGRESS") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r2 = run(dag, state2, "live");
  assert.notEqual(r2.status, 0, "quote-bearing trailing comment must fail loudly");
  assert.match(r2.err, /trailing comment after string contains a double-quote/);
});

test("block-set mismatch: state missing a DAG block and carrying an extra one is rejected with the symmetric difference", () => {
  const dag = write("dag-set.toml", DAG);
  // A2 missing, Z9 extra.
  const state = write(
    "state-set-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "IN_PROGRESS") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("Z9", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "block-set mismatch must fail");
  assert.match(r.err, /state block set does not equal DAG block set/);
  assert.match(r.err, /missing from state: \[A2\]/);
  assert.match(r.err, /in state but not in DAG: \[Z9\]/);
});

test("zero blocks validated is a FAILURE, not a pass", () => {
  const dag = write("dag-zero.toml", DAG);
  const state = write("state-zero.toml", header({ status: "ACTIVE", current: "A0", repoSha: SHA }));
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "zero-block run must fail");
  assert.match(r.err, /zero blocks validated — a run that validates zero blocks is a FAILURE/);
});

test("mandate class gate: NOT_REQUIRED on a foundational-class block is rejected", () => {
  const dag = write("dag-notreq.toml", DAG);
  // A0 (class = "foundational") ACCEPTED with all three mandates NOT_REQUIRED
  // and everything else legal — governance.md:106 requires all three review
  // mandates on a foundational block; NOT_REQUIRED is permitted only for
  // architecture_review on a subsystem-class block (governance.md §2.2).
  const state = write(
    "state-notreq-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      acceptedBlock("A0", {
        conformance_review: "NOT_REQUIRED",
        architecture_review: "NOT_REQUIRED",
        adversarial_review: "NOT_REQUIRED",
      }) +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "NOT_REQUIRED on a foundational block must fail");
  assert.match(
    r.err,
    /state block A0 is ACCEPTED but conformance_review is NOT_REQUIRED and DAG class "foundational" does not permit it/,
  );
  assert.match(
    r.err,
    /architecture_review is NOT_REQUIRED and DAG class "foundational" does not permit it/,
  );
});

test("sequencing invariant: a stackless READY block with an unaccepted predecessor is rejected", () => {
  const dag = write("dag-ready.toml", DAG);
  // READY is a begun status (governance.md:6 names contingent READY work as
  // stacked-exception-only) — a stackless READY A1 while A0 is unaccepted must
  // NOT pass.
  const state = write(
    "state-ready-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "READY") +
      "\n" +
      block("A1", "READY") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "stackless READY with unaccepted predecessor must fail");
  assert.match(
    r.err,
    /sequencing violation .*block A1 is READY but direct predecessor\(s\) not ACCEPTED: \[A0\] \(no stack_id/,
  );
});

test("stacked-work exception: a predecessor citing a DIFFERENT stack snapshot digest is rejected", () => {
  const dag = write("dag-snap.toml", DAG);
  // A1 REVIEW claims the exception; its unaccepted predecessor A0 shares the
  // stack_id and a lower layer but cites a DIFFERENT snapshot digest — not
  // "the same validated immutable stack snapshot" (governance.md:6).
  const state = write(
    "state-snap-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
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
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "mismatched stack snapshot digest must fail");
  assert.match(r.err, /contingent stacked-work exception is REJECTED/);
  assert.match(
    r.err,
    /unaccepted predecessor A0 stack_snapshot_digest .* is not the same well-formed snapshot digest as block A1/,
  );
});

test("live mode: an empty program_dag_digest is a violation, not a silent skip", () => {
  const dag = write("dag-dagdigest.toml", DAG);
  const state = write(
    "state-dagdigest-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, dagDigest: "" }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "empty program_dag_digest in live mode must fail");
  assert.match(
    r.err,
    /live state program_dag_digest "" is not a resolved 64-char lowercase SHA-256 — an empty\/malformed value silently disables the ledger-to-DAG binding/,
  );
});

test("landing equivalence: ACCEPTED with a diverged accepted identity and no landing_equivalence_digest is rejected", () => {
  const dag = write("dag-landeq.toml", DAG);
  // accepted_sha differs from candidate_sha and no landing-equivalence artifact
  // is bound — governance.md:283 / contracts/stacked-prs.md:140 make the
  // divergence legal ONLY with a repository-validated equivalence artifact.
  const state = write(
    "state-landeq-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0", { accepted_sha: SHA2, landing_equivalence_digest: "" }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "diverged accepted identity without landing equivalence must fail");
  assert.match(
    r.err,
    /state block A0 is ACCEPTED with an accepted identity diverging from the reviewed candidate identity but landing_equivalence_digest ""/,
  );
});

test("private checkpoint: a PROVEN checkpoint over accepted predecessors passes (no fail-closed false positive)", () => {
  const dag = write("dag-cp-ok.toml", DAG_CP);
  // A1 (class foundational-private-checkpoint) in PRIVATE_CHECKPOINT with exact
  // identities and all three mandates PASS, over an ACCEPTED A0 — the legal D1
  // analogue. No accepted identity and no maintainer acceptance is required.
  const state = write(
    "state-cp-ok.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("private checkpoint: a PREMATURE checkpoint (unaccepted predecessor) is rejected", () => {
  const dag = write("dag-cp-seq.toml", DAG_CP);
  // PRIVATE_CHECKPOINT is a begun status: A1 fully proven but its direct
  // predecessor A0 is only READY — the sequencing loop must not skip it.
  const state = write(
    "state-cp-seq-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
      block("A0", "READY") +
      "\n" +
      checkpointBlock("A1") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "premature checkpoint must fail");
  assert.match(
    r.err,
    /sequencing violation .*block A1 is PRIVATE_CHECKPOINT but direct predecessor\(s\) not ACCEPTED: \[A0\]/,
  );
});

test("private checkpoint: an UNPROVEN checkpoint (PENDING mandates, empty identities) is rejected", () => {
  const dag = write("dag-cp-proof.toml", DAG_CP);
  // A1 in PRIVATE_CHECKPOINT with the block() defaults: empty identity fields
  // and all mandates PENDING — a checkpoint is review-APPROVED work and must be
  // identity- and evidence-bound.
  const state = write(
    "state-cp-proof-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "PRIVATE_CHECKPOINT") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "unproven checkpoint must fail");
  assert.match(
    r.err,
    /state block A1 is PRIVATE_CHECKPOINT but candidate_sha is not a non-empty 40-char lowercase git object id/,
  );
  assert.match(
    r.err,
    /state block A1 is PRIVATE_CHECKPOINT but evidence_digest is not a non-empty 64-char lowercase SHA-256/,
  );
  assert.match(r.err, /state block A1 is PRIVATE_CHECKPOINT but conformance_review is "PENDING"/);
});

test("private checkpoint: the status on a WRONG-CLASS block is rejected", () => {
  const dag = write("dag-cp-class.toml", DAG);
  // Base DAG: A1 is class "foundational", not "foundational-private-checkpoint".
  // Even a fully-proven checkpoint row is a fabricated checkpoint there.
  const state = write(
    "state-cp-class-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "wrong-class checkpoint must fail");
  assert.match(
    r.err,
    /state block A1 is PRIVATE_CHECKPOINT but its DAG class is "foundational" — the PRIVATE_CHECKPOINT status is permitted only for a block whose DAG class is "foundational-private-checkpoint"/,
  );
});

test("live mode: a non-ACTIVE top-level status is rejected", () => {
  const dag = write("dag-active.toml", DAG);
  const state = write(
    "state-active-bad.toml",
    header({ status: "PAUSED", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "non-ACTIVE live status must fail");
  assert.match(
    r.err,
    /live state top-level status is "PAUSED" \(ORCHESTRATOR\.md:83 requires the live ledger to carry status = "ACTIVE"\)/,
  );
});

test("stacked-work exception: a fully-ESTABLISHED same-snapshot stack passes", () => {
  const dag = write("dag-stack-ok.toml", DAG);
  // The legal stacked shape: A1 REVIEW above IN_PROGRESS A0, same non-empty
  // stack_id, identical well-formed snapshot digest, strictly increasing layers.
  const state = write(
    "state-stack-ok.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "IN_PROGRESS", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
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
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("stacked-work exception: an equal (non-lower) predecessor stack_layer is rejected", () => {
  const dag = write("dag-stack-layer.toml", DAG);
  // Identical to the legal stack above except A0 sits at the SAME layer as A1 —
  // a predecessor must be strictly below its successor in the stack.
  const state = write(
    "state-stack-layer-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
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
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "equal predecessor stack_layer must fail");
  assert.match(r.err, /contingent stacked-work exception is REJECTED/);
  assert.match(
    r.err,
    /unaccepted predecessor A0 stack_layer 1 is not below block A1 stack_layer 1/,
  );
});

test("stacked-work exception: a TERMINATED (ABORTED) predecessor inside the claimed stack is rejected", () => {
  const dag = write("dag-stack-term.toml", DAG);
  const state = write(
    "state-stack-term-bad.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
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
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "terminated stacked predecessor must fail");
  assert.match(r.err, /contingent stacked-work exception is REJECTED/);
  assert.match(
    r.err,
    /unaccepted predecessor A0 is "ABORTED" — a predecessor that has not begun \(or has terminated\) cannot be a lower layer of the same validated stack snapshot/,
  );
});

test("mandate class gate: architecture_review NOT_REQUIRED on a subsystem-class block passes", () => {
  const dag = write("dag-sub-ok.toml", DAG_SUB);
  // A1 is subsystem-class in DAG_SUB: governance.md §2.2 permits skipping
  // exactly architecture review there; the other two mandates still PASS.
  const state = write(
    "state-sub-ok.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_SUB_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { architecture_review: "NOT_REQUIRED", architecture_reviewed_sha: "" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("entry lock: the DAG root block ACCEPTED with an EMPTY or MISSING entry_lock_digest is rejected", () => {
  const dag = write("dag-entrylock.toml", DAG);
  // Empty variant: an otherwise fully-legal ACCEPTED root row (exact identities,
  // all mandates PASS, maintainer acceptance) with entry_lock_digest emptied —
  // exactly the ledger edit that would un-bind the entry-lock record while
  // sailing through review to acceptance.
  const stateEmpty = write(
    "state-entrylock-empty.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0", { entry_lock_digest: "" }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r1 = run(dag, stateEmpty, "live");
  assert.notEqual(r1.status, 0, "ACCEPTED root with empty entry_lock_digest must fail");
  assert.match(
    r1.err,
    /state block A0 is ACCEPTED but entry_lock_digest "" is not a non-empty 64-char lowercase SHA-256 — A0 is the DAG's entry \(root\) block/,
  );

  // Missing variant: the entry_lock_digest LINE deleted outright. The strip is
  // asserted to have applied so a drifted fixture cannot silently turn this
  // into a duplicate of the empty variant.
  const row = acceptedBlock("A0");
  const stripped = row.replace(/entry_lock_digest = "[^"]*"\n/, "");
  assert.notEqual(stripped, row, "entry_lock_digest line strip did not apply");
  const stateMissing = write(
    "state-entrylock-missing.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      stripped +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r2 = run(dag, stateMissing, "live");
  assert.notEqual(r2.status, 0, "ACCEPTED root with the entry_lock_digest line absent must fail");
  assert.match(
    r2.err,
    /state block A0 is ACCEPTED but entry_lock_digest "" is not a non-empty 64-char lowercase SHA-256 — A0 is the DAG's entry \(root\) block/,
  );
});

test("entry lock: the DAG root block in REVIEW requires the digest; a bound digest passes", () => {
  const dag = write("dag-entrylock-rev.toml", DAG);
  // The root's identity/digest fields are all resolved but entry_lock_digest is
  // empty — REVIEW is the FIRST gated status, so the binding must already hold.
  const reviewRoot = (entryLock) =>
    block("A0", "REVIEW", {
      entry_lock_digest: entryLock,
      base_sha: SHA,
      candidate_sha: SHA,
      candidate_tree: TREE,
      charter_digest: DIGEST,
      context_packet_digest: DIGEST,
      evidence_digest: DIGEST,
    });
  const stateBad = write(
    "state-entrylock-review-bad.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      reviewRoot("") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r1 = run(dag, stateBad, "live");
  assert.notEqual(r1.status, 0, "REVIEW root with empty entry_lock_digest must fail");
  assert.match(
    r1.err,
    /state block A0 is REVIEW but entry_lock_digest "" is not a non-empty 64-char lowercase SHA-256 — A0 is the DAG's entry \(root\) block/,
  );

  // Positive bound case: identical state with the digest bound validates green.
  const stateOk = write(
    "state-entrylock-review-ok.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      reviewRoot(DIGEST) +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r2 = run(dag, stateOk, "live");
  assert.equal(r2.status, 0, `expected pass, got:\n${r2.err}\n${r2.out}`);
  assert.match(r2.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("private-checkpoint predecessor: a STACKLESS REVIEW successor over a PRIVATE_CHECKPOINT predecessor is rejected with the fail-closed stack-window message", () => {
  const dag = write("dag-cp-pred.toml", DAG_CP);
  // REVIEW over a proven PRIVATE_CHECKPOINT. Assert the fail-closed
  // stack-window message, not merely a non-zero exit — the generic
  // stackless sequencing violation also fires here.
  const state = write(
    "state-cp-pred-stackless.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
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
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "REVIEW successor over a PRIVATE_CHECKPOINT predecessor must fail");
  assert.match(
    r.err,
    /block A2 is REVIEW with predecessor A1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block \(contracts\/stacked-prs\.md\), which this validator does not model — fail closed/,
  );
});

test("private-checkpoint predecessor: a REVIEW successor with OTHERWISE-PERFECT stack fields over a PRIVATE_CHECKPOINT predecessor is still rejected", () => {
  const dag = write("dag-cp-pred-stack.toml", DAG_CP);
  // PRIVATE_CHECKPOINT is begun, so a claimed stack over a checkpoint
  // predecessor establishes cleanly. The fail-closed predecessor check is
  // then the only rejection — neutralizing it would accept this state.
  const state = write(
    "state-cp-pred-stacked.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 0 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = run(dag, state, "live");
  assert.notEqual(
    r.status,
    0,
    "stacked REVIEW successor over a PRIVATE_CHECKPOINT predecessor must fail",
  );
  assert.match(
    r.err,
    /block A2 is REVIEW with predecessor A1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block \(contracts\/stacked-prs\.md\), which this validator does not model — fail closed/,
  );
});

// -- AMD-001 §1 discriminating D1/D2 transition test (A1/A2 stand in for
// D1/D2, same convention as the two fail-closed tests above). Each negative
// case fails for its OWN reason — see scripts/validate-stack-window.test.mjs
// for the underlying model's own unit coverage; this proves the real
// end-to-end wiring through validate-program-state.mjs's --stack-window flag.
function stackWindowText({ acceptanceId = "A2", a1Kind = "NON_MERGEABLE_PRIVATE_LAYER" } = {}) {
  return `schema = 1
revision = 11
status = "ACTIVE"
mode = "ATOMIC_REVIEW"
stack_id = "S1"
acceptance_block_id = "${acceptanceId}"
authority_package_digest = "${DIGEST}"
implementation_lock_digest = "${DIGEST}"
program_state_basis_digest = "${DIGEST}"
previous_stack_snapshot_digest = "NOT_APPLICABLE"
root_branch = "main"
root_base_sha = "${SHA}"
root_base_tree = "${SHA}"
stack_tool = "LOCAL_BRANCH_CHAIN"
stack_tool_version = "git 2.x"
landing_mode = "bottom-up"
max_open_layers = 2
owner = "orchestrator"
evidence_root = "docs/arch/refactor/rev11/evidence"
shared_writer_surfaces = []
integration_commands = []
notes = ""

[[layer]]
index = 1
layer_id = "A1"
block_id = "A1"
charter_digest = "${DIGEST}"
kind = "${a1Kind}"
branch = "a1-branch"
base_branch = "main"
worktree = "wt-a1"
worker = "w1"
pr_number = 0
pr_url = ""
base_sha = "${SHA}"
base_tree = "${SHA}"
head_sha = ""
head_tree = ""
patch_digest = ""
generated_digest = ""
evidence_digest = ""
ci_state = "PENDING"
review_state = "PENDING"
mergeable = ${a1Kind === "mergeable"}
notes = ""

[[layer]]
index = 2
layer_id = "A2"
block_id = "A2"
charter_digest = "${DIGEST}"
kind = "mergeable"
branch = "a2-branch"
base_branch = "a1-branch"
worktree = "wt-a2"
worker = "w2"
pr_number = 0
pr_url = ""
base_sha = "${SHA}"
base_tree = "${SHA}"
head_sha = ""
head_tree = ""
patch_digest = ""
generated_digest = ""
evidence_digest = ""
ci_state = "PENDING"
review_state = "PENDING"
mergeable = true
notes = ""
`;
}

function digestOf(text) {
  return createHash("sha256").update(text).digest("hex");
}

test("D1/D2 transition (AMD-001): PRIVATE_CHECKPOINT A1 inside a validated ATOMIC_REVIEW window with A2 as acceptance_block_id VALIDATES", () => {
  const dag = write("dag-cp-d1d2-ok.toml", DAG_CP);
  const windowText = stackWindowText();
  const windowPath = write("stack-window-ok.toml", windowText);
  const snap = digestOf(windowText);
  const state = write(
    "state-d1d2-ok.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
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
  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("D1/D2 transition (AMD-001), negative (a): no --stack-window given REJECTS with the fail-closed message", () => {
  const dag = write("dag-cp-d1d2-a.toml", DAG_CP);
  const state = write(
    "state-d1d2-a.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
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
  const r = run(dag, state, "live"); // no --stack-window
  assert.notEqual(r.status, 0);
  assert.match(
    r.err,
    /block A2 is REVIEW with predecessor A1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block \(contracts\/stacked-prs\.md\), which this validator does not model — fail closed/,
  );
});

test("D1/D2 transition (AMD-001), negative (b): mismatched snapshot digest REJECTS", () => {
  const dag = write("dag-cp-d1d2-b.toml", DAG_CP);
  const windowText = stackWindowText();
  const windowPath = write("stack-window-b.toml", windowText);
  const snap = digestOf(windowText);
  const wrongSnap = digestOf("not the window contents");
  const state = write(
    "state-d1d2-b.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
      "\n" +
      block("A2", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: wrongSnap, // MISMATCH
        stack_layer: 2,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /composite stack-window validation via --stack-window/);
  assert.match(
    r.err,
    /block A2 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/,
  );
});

test("D1/D2 transition (AMD-001), negative (c): acceptance_block_id names a block OTHER than A2 REJECTS", () => {
  const dag = write("dag-cp-d1d2-c.toml", DAG_CP);
  const windowText = stackWindowText({ acceptanceId: "SOMETHING_ELSE" });
  const windowPath = write("stack-window-c.toml", windowText);
  const snap = digestOf(windowText);
  const state = write(
    "state-d1d2-c.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
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
  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /composite stack-window validation via --stack-window/);
  assert.match(r.err, /acceptance_block_id is "SOMETHING_ELSE", not the successor block "A2"/);
});

test("D1/D2 transition (AMD-001), negative (d): A1 landed independently (its layer is kind = mergeable, not NON_MERGEABLE_PRIVATE_LAYER) REJECTS", () => {
  const dag = write("dag-cp-d1d2-d.toml", DAG_CP);
  const windowText = stackWindowText({ a1Kind: "mergeable" });
  const windowPath = write("stack-window-d.toml", windowText);
  const snap = digestOf(windowText);
  const state = write(
    "state-d1d2-d.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
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
  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /composite stack-window validation via --stack-window/);
  assert.match(
    r.err,
    /layer for predecessor block "A1" has kind "mergeable", not NON_MERGEABLE_PRIVATE_LAYER — a checkpoint predecessor's layer must never be independently mergeable/,
  );
});

function writeLandingRecord(root, id, body) {
  const blockDir = join(root, id);
  mkdirSync(blockDir, { recursive: true });
  const artifact = join(blockDir, "landing-record.md");
  writeFileSync(artifact, body);
  return artifact;
}

test("live mode: a well-formed but WRONG program_dag_digest is a violation", () => {
  const dag = write("dag-dagdigest-mismatch.toml", DAG);
  const state = write(
    "state-dagdigest-mismatch.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, dagDigest: DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "mismatched program_dag_digest must fail");
  assert.match(r.err, /state program_dag_digest .* does not match the SHA-256 of the DAG file/);
});

test("live mode: an unresolvable evidence_root cannot bind evidence_digest — fail closed", () => {
  const dag = write("dag-evroot-placeholder.toml", DAG);
  const state = write(
    "state-evroot-placeholder.toml",
    header({
      status: "ACTIVE",
      current: "A1",
      repoSha: SHA,
      evidenceRoot: "<EVIDENCE>",
    }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(
    r.status,
    0,
    "unresolvable evidence_root with a bound evidence_digest must fail, not print OK",
  );
  assert.match(
    r.err,
    /live state orchestration\.evidence_root "<EVIDENCE>" is not a resolvable directory — evidence_digest bindings cannot be verified/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("live mode: a well-formed evidence_digest that does not match the bound artifact is a violation", () => {
  const evidenceRoot = join(dir, "evidence-mismatch");
  const artifact = writeLandingRecord(evidenceRoot, "A0", "landing record body\n");
  const actual = createHash("sha256").update(readFileSync(artifact)).digest("hex");
  assert.notEqual(actual, DIGEST, "fixture digest must differ from the artifact hash");

  const dag = write("dag-evbind-mismatch.toml", DAG);
  const state = write(
    "state-evbind-mismatch.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "mismatched evidence_digest must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A0 evidence_digest ${DIGEST} does not match the SHA-256 of .*landing-record\\.md \\(${actual}\\)`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("live mode: a bound evidence_digest with no artifact under evidence_root is a violation", () => {
  const evidenceRoot = join(dir, "evidence-missing");
  mkdirSync(evidenceRoot, { recursive: true });
  const dag = write("dag-evbind-missing.toml", DAG);
  const state = write(
    "state-evbind-missing.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "missing evidence artifact must fail");
  assert.match(r.err, /state block A0 has evidence_digest .* but no evidence artifact under/);
});

test("live mode: an evidence_digest that matches the bound landing-record artifact passes", () => {
  const evidenceRoot = join(dir, "evidence-match");
  const artifact = writeLandingRecord(evidenceRoot, "A0", "exact landing record\n");
  const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

  const dag = write("dag-evbind-ok.toml", DAG);
  const state = write(
    "state-evbind-ok.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0", { evidence_digest: digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("multiple evidence roots: a block's artifact resolves from the second declared root when the first root does not carry it", () => {
  const rootA = join(dir, "roots-resolve-a");
  const rootB = join(dir, "roots-resolve-b");
  mkdirSync(rootA, { recursive: true }); // exists, but has no A0 subdirectory at all
  const artifact = writeLandingRecord(rootB, "A0", "second-root landing record\n");
  const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

  const dag = write("dag-roots-resolve.toml", DAG);
  const state = write(
    "state-roots-resolve.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoots: [rootA, rootB] }) +
      acceptedBlock("A0", { evidence_digest: digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("multiple evidence roots: a mismatched digest against the resolved artifact still fails", () => {
  const rootA = join(dir, "roots-mismatch-a");
  const rootB = join(dir, "roots-mismatch-b");
  const artifact = writeLandingRecord(rootA, "A0", "root-a landing record\n");
  const actual = createHash("sha256").update(readFileSync(artifact)).digest("hex");
  assert.notEqual(actual, DIGEST, "fixture digest must differ from the artifact hash");
  mkdirSync(rootB, { recursive: true });

  const dag = write("dag-roots-mismatch.toml", DAG);
  const state = write(
    "state-roots-mismatch.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoots: [rootA, rootB] }) +
      acceptedBlock("A0") + // acceptedBlock() carries evidence_digest = DIGEST, which differs from `actual`
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(
    r.status,
    0,
    "mismatched evidence_digest under a multi-root declaration must fail",
  );
  assert.match(
    r.err,
    new RegExp(
      `state block A0 evidence_digest ${DIGEST} does not match the SHA-256 of .*landing-record\\.md \\(${actual}\\)`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("multiple evidence roots: a block with no artifact under ANY declared root still fails", () => {
  const rootA = join(dir, "roots-none-a");
  const rootB = join(dir, "roots-none-b");
  mkdirSync(rootA, { recursive: true });
  mkdirSync(rootB, { recursive: true });

  const dag = write("dag-roots-none.toml", DAG);
  const state = write(
    "state-roots-none.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoots: [rootA, rootB] }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "an artifact absent from every declared root must fail");
  assert.match(
    r.err,
    /state block A0 has evidence_digest .* but no evidence artifact under \[.*roots-none-a.*roots-none-b.*\]/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("multiple evidence roots: one unresolvable declared root still fails closed even when another root resolves the artifact", () => {
  const rootA = join(dir, "roots-unresolvable-a"); // deliberately never created
  const rootB = join(dir, "roots-unresolvable-b");
  const artifact = writeLandingRecord(rootB, "A0", "root-b landing record\n");
  const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

  const dag = write("dag-roots-unresolvable.toml", DAG);
  const state = write(
    "state-roots-unresolvable.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoots: [rootA, rootB] }) +
      acceptedBlock("A0", { evidence_digest: digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(
    r.status,
    0,
    "a declared-but-unresolvable root must fail even though a later root resolves the block's artifact",
  );
  assert.match(
    r.err,
    new RegExp(
      `live state orchestration\\.evidence_roots\\[0\\] ${JSON.stringify(rootA)} is not a resolvable directory`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

// -- Extended artifact-name convention: <id>-summary.md, landing-equivalence.md,
// and one nested level (<root>/<id>/*/landing-record.md). Each fixture writer
// below deliberately writes ONLY its own named file into a fresh block dir, so
// a passing test proves that specific name resolves — not that some other
// convention member happened to also be present.

function writeNamedArtifact(root, id, filename, body) {
  const blockDir = join(root, id);
  mkdirSync(blockDir, { recursive: true });
  const artifact = join(blockDir, filename);
  writeFileSync(artifact, body);
  return artifact;
}

function writeNestedLandingRecord(root, id, subdir, body) {
  const nestedDir = join(root, id, subdir);
  mkdirSync(nestedDir, { recursive: true });
  const artifact = join(nestedDir, "landing-record.md");
  writeFileSync(artifact, body);
  return artifact;
}

// Root-level sibling to the block dir (<root>/<id>-summary.md, NOT
// <root>/<id>/<id>-summary.md) — the real on-disk shape for A4/A5/A6.
function writeSiblingArtifact(root, id, body) {
  mkdirSync(root, { recursive: true });
  const artifact = join(root, `${id}-summary.md`);
  writeFileSync(artifact, body);
  return artifact;
}

test("extended artifact convention: root-level sibling <id>-summary.md (not nested under <id>/) resolves", () => {
  const evidenceRoot = join(dir, "evidence-sibling-summary");
  const artifact = writeSiblingArtifact(evidenceRoot, "A0", "sibling summary body\n");
  const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

  const dag = write("dag-sibling-summary.toml", DAG);
  const state = write(
    "state-sibling-summary.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0", { evidence_digest: digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("extended artifact convention: root-level sibling <id>-summary.md with a mismatched digest still fails", () => {
  const evidenceRoot = join(dir, "evidence-sibling-summary-mismatch");
  const artifact = writeSiblingArtifact(evidenceRoot, "A0", "sibling summary body, wrong digest\n");
  const actual = createHash("sha256").update(readFileSync(artifact)).digest("hex");
  assert.notEqual(actual, DIGEST, "fixture digest must differ from the artifact hash");

  const dag = write("dag-sibling-summary-mismatch.toml", DAG);
  const state = write(
    "state-sibling-summary-mismatch.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0") + // carries DIGEST, which differs from `actual`
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a mismatched digest against the sibling summary must still fail");
  assert.match(
    r.err,
    /state block A0 evidence_digest .* does not match the SHA-256 of .*A0-summary\.md/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

for (const [label, filename] of [
  ["<id>-summary.md", "A0-summary.md"],
  ["landing-equivalence.md", "landing-equivalence.md"],
]) {
  test(`extended artifact convention: ${label} resolves and its digest matches`, () => {
    const evidenceRoot = join(dir, `evidence-${filename}`);
    const artifact = writeNamedArtifact(evidenceRoot, "A0", filename, `${label} body\n`);
    const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

    const dag = write(`dag-${filename}.toml`, DAG);
    const state = write(
      `state-${filename}.toml`,
      header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
        acceptedBlock("A0", { evidence_digest: digest }) +
        "\n" +
        block("A1", "IN_PROGRESS") +
        "\n" +
        block("A2", "LOCKED"),
    );
    const r = run(dag, state, "live");
    assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
    assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
  });

  test(`extended artifact convention: ${label} with a mismatched digest still fails`, () => {
    const evidenceRoot = join(dir, `evidence-mismatch-${filename}`);
    const artifact = writeNamedArtifact(
      evidenceRoot,
      "A0",
      filename,
      `${label} body, wrong digest\n`,
    );
    const actual = createHash("sha256").update(readFileSync(artifact)).digest("hex");
    assert.notEqual(actual, DIGEST, "fixture digest must differ from the artifact hash");

    const dag = write(`dag-mismatch-${filename}.toml`, DAG);
    const state = write(
      `state-mismatch-${filename}.toml`,
      header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
        acceptedBlock("A0") + // carries DIGEST, which differs from `actual`
        "\n" +
        block("A1", "IN_PROGRESS") +
        "\n" +
        block("A2", "LOCKED"),
    );
    const r = run(dag, state, "live");
    assert.notEqual(r.status, 0, `a mismatched digest against ${label} must still fail`);
    assert.match(
      r.err,
      new RegExp(
        `state block A0 evidence_digest ${DIGEST} does not match the SHA-256 of .*${filename.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\(${actual}\\)`,
      ),
    );
    assert.doesNotMatch(r.out, /^OK:/);
  });
}

test("extended artifact convention: one nested match (<root>/<id>/*/landing-record.md) resolves", () => {
  const evidenceRoot = join(dir, "evidence-nested-single");
  const a0Artifact = writeLandingRecord(evidenceRoot, "A0", "A0 landing record\n");
  const a0Digest = createHash("sha256").update(readFileSync(a0Artifact)).digest("hex");
  const artifact = writeNestedLandingRecord(
    evidenceRoot,
    "A2",
    "reopen4",
    "nested landing record\n",
  );
  const digest = createHash("sha256").update(readFileSync(artifact)).digest("hex");

  const dag = write("dag-nested-single.toml", DAG);
  const state = write(
    "state-nested-single.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0", { evidence_digest: a0Digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      // LOCKED (not ACCEPTED): keeps this test isolated to the evidence-digest
      // content-binding check, which runs over every well-formed evidence_digest
      // regardless of status, without also having to satisfy A2's own
      // ACCEPTED-predecessor sequencing (A1 is only IN_PROGRESS here).
      block("A2", "LOCKED", { evidence_digest: digest }),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("extended artifact convention: MULTIPLE nested matches are ambiguous and fail closed", () => {
  const evidenceRoot = join(dir, "evidence-nested-ambiguous");
  const a0Artifact = writeLandingRecord(evidenceRoot, "A0", "A0 landing record\n");
  const a0Digest = createHash("sha256").update(readFileSync(a0Artifact)).digest("hex");
  const a1 = writeNestedLandingRecord(
    evidenceRoot,
    "A2",
    "reopen1",
    "first nested landing record\n",
  );
  const a2 = writeNestedLandingRecord(
    evidenceRoot,
    "A2",
    "reopen4",
    "second nested landing record\n",
  );
  // Deliberately no named candidate (landing-record.md / *-exact-candidate-record.md /
  // *-summary.md / landing-equivalence.md) at the block-dir level — otherwise that
  // would resolve first and the nested search would never run.
  const digest = createHash("sha256").update(readFileSync(a2)).digest("hex");

  const dag = write("dag-nested-ambiguous.toml", DAG);
  const state = write(
    "state-nested-ambiguous.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA, evidenceRoot }) +
      acceptedBlock("A0", { evidence_digest: a0Digest }) +
      "\n" +
      block("A1", "IN_PROGRESS") +
      "\n" +
      block("A2", "LOCKED", { evidence_digest: digest }),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "multiple nested matches must fail closed, never silently pick one");
  assert.match(
    r.err,
    /state block A2 has evidence_digest .* but multiple nested evidence artifacts resolve for it, ambiguous: \[.*reopen1.*landing-record\.md.*reopen4.*landing-record\.md.*\]/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
  // Sanity: both fixture files are real and distinct, so this isn't an
  // artifact of one write clobbering the other.
  assert.notEqual(readFileSync(a1, "utf8"), readFileSync(a2, "utf8"));
});

// -- Git identity verification (the fix this file was extended for). Every
// test below uses the real gitRoot fixture built in before(): SHA_BASE (root)
// -> SHA (tip), plus SHA_DANGLING (a real commit, unreachable from the tip,
// off SHA_BASE on a since-deleted branch) — the exact shape of the corrected
// A5 ledger row (a dangling commit recorded as both candidate and accepted
// identity). Each test isolates ONE of the four new checks so a failure here
// is attributable to that one check, and each was run against the
// un-hardened validator (git stashed) to confirm it does NOT catch these —
// see the session report.

test("git identity: a well-formed but NEVER-COMMITTED accepted_sha is rejected", () => {
  const dag = write("dag-git-nonexistent.toml", DAG);
  const state = write(
    "state-git-nonexistent.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { accepted_sha: SHA_NONEXISTENT }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a never-committed accepted_sha must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 field accepted_sha = ${SHA_NONEXISTENT} does not resolve to an existing git commit object \\(git reports: missing\\)`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("git identity: a real but DANGLING accepted_sha (unreachable from the repository tip) is rejected — the A5 case", () => {
  const dag = write("dag-git-dangling.toml", DAG);
  const state = write(
    "state-git-dangling.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", {
        base_sha: SHA_BASE, // real ancestor of SHA_DANGLING — isolates this test to check 3
        candidate_sha: SHA_DANGLING,
        candidate_tree: TREE_DANGLING,
        accepted_sha: SHA_DANGLING,
        accepted_tree: TREE_DANGLING,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a dangling accepted_sha must fail — it is not genuinely landed");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 is ACCEPTED with accepted_sha ${SHA_DANGLING} but that commit is not reachable from the repository tip .* — it is not genuinely landed`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("git identity: a candidate_tree that is the tree of a DIFFERENT commit is rejected", () => {
  const dag = write("dag-git-tree-mismatch.toml", DAG);
  const state = write(
    "state-git-tree-mismatch.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "REVIEW", {
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: TREE_BASE, // real tree, but of SHA_BASE, not of candidate_sha (SHA)
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a tree belonging to a different commit must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 field candidate_tree = ${TREE_BASE} is not the tree of candidate_sha ${SHA} — git reports that commit's tree as ${TREE}`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("git identity: an ACCEPTED base_sha that is NOT an ancestor of accepted_sha is rejected", () => {
  const dag = write("dag-git-base-not-ancestor.toml", DAG);
  const state = write(
    "state-git-base-not-ancestor.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", {
        base_sha: SHA_DANGLING, // real commit, but a sibling of SHA — not its ancestor
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a base_sha that is not an ancestor of accepted_sha must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 is ACCEPTED but base_sha ${SHA_DANGLING} is not an ancestor of accepted_sha ${SHA}`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("git identity: a genuine multi-commit ancestry chain (base strictly precedes accepted) passes", () => {
  const dag = write("dag-git-real-ancestry-ok.toml", DAG);
  const state = write(
    "state-git-real-ancestry-ok.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { base_sha: SHA_BASE }) + // SHA_BASE is a real, strict ancestor of SHA
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("live mode: NOT a git repository fails loudly, never a silent skip that greens the run", () => {
  const dag = write("dag-git-norepo.toml", DAG);
  const state = write(
    "state-git-norepo.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live", notGitDir);
  assert.notEqual(r.status, 0, "live mode outside a git repository must fail, not silently pass");
  assert.match(
    r.err,
    /live mode requires a git repository to verify base_sha\/candidate_sha\/accepted_sha\/candidate_tree\/accepted_tree against real git objects, but git is unavailable at .* — this is a loud setup failure, never a silent skip of identity verification/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("template mode: passes with placeholder identities and NO git repository at all", () => {
  const dag = write("dag-git-template-norepo.toml", DAG);
  const state = write(
    "state-git-template-norepo.toml",
    `schema = 1
revision = 11
status = "TEMPLATE"
authority_package_digest = "REQUIRED_PACKAGE_DIGEST"
current_block = "A0"

[repository]
remote = "REQUIRED_REMOTE"
branch = "REQUIRED_BRANCH"
head_sha = "REQUIRED_FULL_SHA"
head_tree = "REQUIRED_TREE_OID"
dirty = false
untracked_count = 0

` +
      block("A0", "READY") +
      "\n" +
      block("A1", "LOCKED") +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "template", notGitDir);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

// -- Amendment authority gate (enabling_amendment) --
//
// The BV1 shape: a block introduced by a PROPOSED (not yet ratified) amendment
// must not advance past LOCKED, and must not carry maintainer_decision =
// ACCEPTED, until that amendment's own Status line is ratified.

test("amendment authority gate: READY block with an unratified enabling_amendment is a violation (BV1 shape)", () => {
  const dag = write("dag-amd-ready.toml", DAG);
  const state = write(
    "state-amd-ready.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "READY", { enabling_amendment: "AMD-900" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "an unratified enabling_amendment must fail a begun block");
  assert.match(
    r.err,
    /state block A1 has enabling_amendment AMD-900 but .*AMD-900-not-ratified-fixture\.md is not ratified \(Status: \*\*Status:\*\* PROPOSED — NOT RATIFIED\. This candidate has no execution authority\.\) — an unratified enabling amendment has no execution authority, so the block must not advance beyond LOCKED: status is READY/,
  );
});

test("amendment authority gate: the same block at LOCKED with an unratified enabling_amendment passes", () => {
  const dag = write("dag-amd-locked.toml", DAG);
  const state = write(
    "state-amd-locked.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "LOCKED") +
      "\n" +
      block("A1", "LOCKED", { enabling_amendment: "AMD-900" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(
    r.status,
    0,
    `LOCKED must not be gated by enabling_amendment, got:\n${r.err}\n${r.out}`,
  );
});

test("amendment authority gate: ACCEPTED block with an unratified enabling_amendment is a violation", () => {
  const dag = write("dag-amd-accepted.toml", DAG);
  const state = write(
    "state-amd-accepted.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { enabling_amendment: "AMD-900" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "ACCEPTED with an unratified enabling_amendment must fail");
  assert.match(
    r.err,
    /state block A1 has enabling_amendment AMD-900 but .*AMD-900-not-ratified-fixture\.md is not ratified .* status is ACCEPTED, maintainer_decision is ACCEPTED/,
  );
});

test("amendment authority gate: a ratified enabling_amendment imposes no restriction at READY or ACCEPTED", () => {
  const dag = write("dag-amd-ratified.toml", DAG);
  const stateReady = write(
    "state-amd-ratified-ready.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "READY", { enabling_amendment: "AMD-901" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const rReady = run(dag, stateReady, "live");
  assert.equal(rReady.status, 0, `expected pass, got:\n${rReady.err}\n${rReady.out}`);

  const stateAccepted = write(
    "state-amd-ratified-accepted.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { enabling_amendment: "AMD-901" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const rAccepted = run(dag, stateAccepted, "live");
  assert.equal(rAccepted.status, 0, `expected pass, got:\n${rAccepted.err}\n${rAccepted.out}`);
});

test("amendment authority gate: enabling_amendment naming a file that does not exist is a violation, even at LOCKED", () => {
  const dag = write("dag-amd-missing.toml", DAG);
  const state = write(
    "state-amd-missing.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "LOCKED") +
      "\n" +
      block("A1", "LOCKED", { enabling_amendment: "AMD-999" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a nonexistent enabling_amendment file must fail closed, not guess");
  assert.match(
    r.err,
    /state block A1 declares enabling_amendment "AMD-999" but expected exactly one file matching AMD-999-\*\.md under .*amendments, found 0/,
  );
});

test("amendment authority gate: an unparseable Status line is a violation, not a silent pass, even at LOCKED", () => {
  const dag = write("dag-amd-unparseable.toml", DAG);
  const state = write(
    "state-amd-unparseable.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "LOCKED") +
      "\n" +
      block("A1", "LOCKED", { enabling_amendment: "AMD-902" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "an unparseable Status line must fail closed, not pass silently");
  assert.match(
    r.err,
    /state block A1 declares enabling_amendment "AMD-902" but amendment file .*AMD-902-no-status-fixture\.md has no \*\*Status:\*\* line — its ratification state cannot be parsed/,
  );
});

test("amendment authority gate: an empty enabling_amendment is unaffected", () => {
  const dag = write("dag-amd-empty.toml", DAG);
  const state = write(
    "state-amd-empty.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      block("A1", "READY", { enabling_amendment: "" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
});

test("amendment authority gate: an ambiguous enabling_amendment match (multiple files) is a violation", () => {
  // Amendments dir is resolved relative to the DAG file's own directory, so
  // this fixture gets its own directory (distinct from the shared `dir`) with
  // the DAG living beside a fresh `amendments/` subdirectory.
  const dupDir = join(dir, "amendments-dup");
  const dupAmendmentsDir = join(dupDir, "amendments");
  mkdirSync(dupAmendmentsDir, { recursive: true });
  writeFileSync(join(dupAmendmentsDir, "AMD-950-one.md"), "**Status:** RATIFIED.\n");
  writeFileSync(join(dupAmendmentsDir, "AMD-950-two.md"), "**Status:** RATIFIED.\n");
  const dagPath = join(dupDir, "dag-amd-ambiguous.toml");
  writeFileSync(dagPath, DAG);
  const state = join(dupDir, "state-amd-ambiguous.toml");
  writeFileSync(
    state,
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "LOCKED") +
      "\n" +
      block("A1", "LOCKED", { enabling_amendment: "AMD-950" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dagPath, state, "live");
  assert.notEqual(r.status, 0, "an ambiguous enabling_amendment match must fail closed, not guess");
  assert.match(
    r.err,
    /state block A1 declares enabling_amendment "AMD-950" but expected exactly one file matching AMD-950-\*\.md under .*amendments, found 2/,
  );
});

// Review verdict identity binding: conformance_reviewed_sha/architecture_
// reviewed_sha/adversarial_reviewed_sha bind a PASS mandate to the exact
// candidate_sha it was issued against — the stale-verdict defect this change
// closes (a fix round/rebase/restack advances candidate_sha and the old PASS
// silently carries over).

test("review verdict binding: a PASS mandate whose reviewed SHA differs from candidate_sha is a stale verdict — REJECTED", () => {
  const dag = write("dag-reviewed-stale.toml", DAG);
  const state = write(
    "state-reviewed-stale.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      // candidate_sha stays SHA (acceptedBlock default); conformance_reviewed_sha
      // is bound to SHA_BASE — a DIFFERENT, real commit — exactly the shape of a
      // PASS verdict left over after candidate_sha advanced past a fix round.
      acceptedBlock("A1", { conformance_reviewed_sha: SHA_BASE }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a reviewed SHA that differs from candidate_sha must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 has conformance_review = PASS but conformance_reviewed_sha = ${SHA_BASE} does not equal candidate_sha = ${SHA} — the verdict was issued against a different candidate and is stale`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("review verdict binding: a PASS mandate with an EMPTY reviewed SHA is rejected", () => {
  const dag = write("dag-reviewed-empty.toml", DAG);
  const state = write(
    "state-reviewed-empty.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      acceptedBlock("A1", { adversarial_reviewed_sha: "" }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a PASS mandate with an empty reviewed SHA must fail");
  assert.match(
    r.err,
    /state block A1 has adversarial_review = PASS but adversarial_reviewed_sha is not a non-empty 40-char lowercase git object id: "" — a PASS verdict must bind the exact candidate it was issued against/,
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("review verdict binding: a PENDING mandate that wrongly carries a reviewed SHA is rejected", () => {
  const dag = write("dag-reviewed-pending-carries.toml", DAG);
  const state = write(
    "state-reviewed-pending-carries.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      // A1 stays IN_PROGRESS (no gated-status obligation), conformance_review
      // is PENDING (block() default) but conformance_reviewed_sha is
      // wrongly bound anyway — a mandate that has not passed cannot have a
      // reviewed candidate.
      block("A1", "IN_PROGRESS", { conformance_reviewed_sha: SHA }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a non-PASS mandate carrying a reviewed SHA must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 has conformance_review = "PENDING" \\(not PASS\\) but conformance_reviewed_sha = "${SHA}" is non-empty — a non-PASS mandate must not carry a reviewed candidate SHA`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("review verdict binding: NOT_REQUIRED wrongly carrying a reviewed SHA is rejected", () => {
  const dag = write("dag-reviewed-notreq-carries.toml", DAG_SUB);
  const state = write(
    "state-reviewed-notreq-carries.toml",
    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_SUB_DIGEST }) +
      acceptedBlock("A0") +
      "\n" +
      // architecture_review NOT_REQUIRED is legal on this subsystem-class row,
      // but binding a reviewed SHA to a mandate that was never run is not.
      acceptedBlock("A1", { architecture_review: "NOT_REQUIRED", architecture_reviewed_sha: SHA }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "NOT_REQUIRED carrying a reviewed SHA must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 has architecture_review = "NOT_REQUIRED" \\(not PASS\\) but architecture_reviewed_sha = "${SHA}" is non-empty — a non-PASS mandate must not carry a reviewed candidate SHA`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("review verdict binding: a reviewed SHA naming a non-existent commit is rejected (live git existence)", () => {
  const dag = write("dag-reviewed-nonexistent.toml", DAG);
  const state = write(
    "state-reviewed-nonexistent.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      // candidate_sha and conformance_reviewed_sha are EQUAL (so the structural
      // stale-verdict check above is satisfied) but name a commit that was
      // never committed — only the live git-existence batch can catch this.
      acceptedBlock("A1", {
        candidate_sha: SHA_NONEXISTENT,
        conformance_reviewed_sha: SHA_NONEXISTENT,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "a never-committed reviewed SHA must fail");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 field conformance_reviewed_sha = ${SHA_NONEXISTENT} does not resolve to an existing git commit object \\(git reports: missing\\)`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});

test("review verdict binding: all three mandates correctly bound to a NON-DEFAULT candidate_sha passes", () => {
  const dag = write("dag-reviewed-ok.toml", DAG);
  const state = write(
    "state-reviewed-ok.toml",
    header({ status: "ACTIVE", current: "A1", repoSha: SHA }) +
      acceptedBlock("A0") +
      "\n" +
      // candidate_sha/candidate_tree moved off the acceptedBlock() default (SHA)
      // onto a DIFFERENT real commit (SHA_BASE); every reviewed SHA is bound to
      // that same candidate — proving the check follows the live value, not a
      // coincidental shared default. accepted_sha stays SHA (a real descendant
      // of SHA_BASE), so base_sha/accepted_sha ancestry still holds and the
      // already-bound landing_equivalence_digest covers the divergence.
      acceptedBlock("A1", {
        candidate_sha: SHA_BASE,
        candidate_tree: TREE_BASE,
        conformance_reviewed_sha: SHA_BASE,
        architecture_reviewed_sha: SHA_BASE,
        adversarial_reviewed_sha: SHA_BASE,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /validated 3 blocks \(non-zero work asserted\)/);
});

test("review verdict binding: applies in template mode too (structural, not git-dependent)", () => {
  const dag = write("dag-reviewed-template.toml", DAG);
  const state = write(
    "state-reviewed-template.toml",
    `schema = 1
revision = 11
status = "TEMPLATE"
authority_package_digest = "REQUIRED_PACKAGE_DIGEST"
current_block = "A0"

[repository]
remote = "REQUIRED_REMOTE"
branch = "REQUIRED_BRANCH"
head_sha = "REQUIRED_FULL_SHA"
head_tree = "REQUIRED_TREE_OID"
dirty = false
untracked_count = 0

` +
      block("A0", "READY") +
      "\n" +
      // A stale PASS/reviewed-SHA mismatch — the structural check is
      // independent of --mode and of git; template mode must still reject it.
      block("A1", "LOCKED", {
        conformance_review: "PASS",
        candidate_sha: SHA,
        conformance_reviewed_sha: SHA_BASE,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "template");
  assert.notEqual(r.status, 0, "a stale reviewed SHA must fail even in template mode");
  assert.match(
    r.err,
    new RegExp(
      `state block A1 has conformance_review = PASS but conformance_reviewed_sha = ${SHA_BASE} does not equal candidate_sha = ${SHA} — the verdict was issued against a different candidate and is stale`,
    ),
  );
  assert.doesNotMatch(r.out, /^OK:/);
});
