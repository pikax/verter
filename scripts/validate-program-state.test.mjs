// Tests for scripts/validate-program-state.mjs, run with:
//   node --test scripts/validate-program-state.test.mjs
//
// Fixture set (maintainer-scoped): positive fixtures (template + live + legal
// same-snapshot stacking + subsystem NOT_REQUIRED + proven private checkpoint)
// and discriminating negative controls for the checks that carry weight — the
// sequencing invariant (including stackless READY and a premature
// PRIVATE_CHECKPOINT), the stacked-work exception gate (unestablished stack;
// mismatched snapshot digest; equal predecessor layer; terminated predecessor),
// the block-set match, the status-dependent review/identity gates (including
// the NOT_REQUIRED class gate, the diverged-accepted-identity
// landing-equivalence gate, and the PRIVATE_CHECKPOINT class/proof gates), the
// live-mode status = "ACTIVE" and program_dag_digest bindings, evidence_digest
// content binding (unresolvable evidence_root, mismatched/missing artifact),
// the DAG-root
// entry-lock digest gate (empty/missing rejected at the gated statuses; a bound
// digest passes), the fail-closed PRIVATE_CHECKPOINT-predecessor rejection
// (both stackless and with otherwise-perfect stack fields — the stacked variant
// is the ONLY check standing between a claimed stack and a silently accepted
// checkpoint predecessor, see AMD-001), the strict TOML
// reader, and the zero-blocks-validated case. Every negative asserts BOTH a
// non-zero exit AND the specific violation text, and every positive asserts
// the validator's own OK output line, so none of these tests can pass against
// a validator stubbed to always exit 0.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const VALIDATOR = join(dirname(fileURLToPath(import.meta.url)), "validate-program-state.mjs");

let dir;
before(() => {
  dir = mkdtempSync(join(tmpdir(), "validate-program-state-"));
});
after(() => {
  rmSync(dir, { recursive: true, force: true });
});

function write(name, content) {
  const p = join(dir, name);
  writeFileSync(p, content, "utf8");
  return p;
}

function run(dagPath, statePath, mode) {
  const res = spawnSync(process.execPath, [VALIDATOR, "--dag", dagPath, "--state", statePath, "--mode", mode], {
    encoding: "utf8",
  });
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

const SHA = "9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0";
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
    architecture_review: "PENDING",
    adversarial_review: "PENDING",
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
    candidate_tree: SHA,
    accepted_sha: SHA,
    accepted_tree: SHA,
    landing_equivalence_digest: DIGEST,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    architecture_review: "PASS",
    adversarial_review: "PASS",
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
    candidate_tree: SHA,
    evidence_digest: DIGEST,
    conformance_review: "PASS",
    architecture_review: "PASS",
    adversarial_review: "PASS",
    ...overrides,
  });
}

function header({ status, current, repoSha, dagDigest = DAG_DIGEST, evidenceRoot }) {
  const orchestration =
    evidenceRoot === undefined
      ? ""
      : `
[orchestration]
evidence_root = "${evidenceRoot}"
`;
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
  assert.match(r.err, /sequencing violation .*block A1 is IN_PROGRESS but direct predecessor\(s\) not ACCEPTED: \[A0\]/);
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
        candidate_tree: SHA,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }) +
      "\n" +
      block("A2", "LOCKED"),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "unestablished stacked-work exception must fail");
  assert.match(r.err, /block A1 is REVIEW with unaccepted direct predecessor\(s\) \[A0\] and the contingent stacked-work exception is REJECTED/);
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
  assert.match(r.err, /state block A0 is ACCEPTED but accepted_sha is not a non-empty 40-char lowercase git object id/);
  assert.match(r.err, /state block A0 is ACCEPTED but maintainer_decision is "PENDING"/);
});

test("strict TOML reader: unbalanced quoting is a loud parse failure, never a silent mis-read", () => {
  const dag = write("dag-toml.toml", DAG);
  // Case 1: `status = "ACT"#IVE"` must NOT silently parse as "ACT".
  const state1 = write(
    "state-toml-bad1.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }).replace('status = "ACTIVE"', 'status = "ACT"#IVE"') +
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
  assert.match(r.err, /architecture_review is NOT_REQUIRED and DAG class "foundational" does not permit it/);
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
        candidate_tree: SHA,
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
  assert.match(r.err, /state block A1 is PRIVATE_CHECKPOINT but candidate_sha is not a non-empty 40-char lowercase git object id/);
  assert.match(r.err, /state block A1 is PRIVATE_CHECKPOINT but evidence_digest is not a non-empty 64-char lowercase SHA-256/);
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
  assert.match(r.err, /live state top-level status is "PAUSED" \(ORCHESTRATOR\.md:83 requires the live ledger to carry status = "ACTIVE"\)/);
});

test("stacked-work exception: a fully-ESTABLISHED same-snapshot stack passes", () => {
  const dag = write("dag-stack-ok.toml", DAG);
  // The legal stacked shape: A1 REVIEW above IN_PROGRESS A0, same non-empty
  // stack_id, identical well-formed snapshot digest, strictly increasing layers.
  const state = write(
    "state-stack-ok.toml",
    header({ status: "ACTIVE", current: "A0", repoSha: SHA }) +
      block("A0", "IN_PROGRESS", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 0 }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: SHA,
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
      block("A0", "IN_PROGRESS", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 1 }) +
      "\n" +
      block("A1", "REVIEW", {
        stack_id: "S1",
        stack_snapshot_digest: DIGEST,
        stack_layer: 1,
        base_sha: SHA,
        candidate_sha: SHA,
        candidate_tree: SHA,
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
  assert.match(r.err, /unaccepted predecessor A0 stack_layer 1 is not below block A1 stack_layer 1/);
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
        candidate_tree: SHA,
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
      acceptedBlock("A1", { architecture_review: "NOT_REQUIRED" }) +
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
      candidate_tree: SHA,
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
  // A2 (predecessors [A0, A1]) in REVIEW while A1 sits in a fully-proven
  // PRIVATE_CHECKPOINT. The AMD-001 fail-closed rejection must name the
  // unmodelled stack-window path — asserting THAT message (not merely a
  // non-zero exit) keeps this test discriminating: the generic stackless
  // sequencing violation also fires here, so exit code alone cannot tell the
  // fail-closed check from the generic one.
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
        candidate_tree: SHA,
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
  // The AMD-001 D1/D2 interaction: PRIVATE_CHECKPOINT is a begun status, so a
  // claimed stack over a checkpoint predecessor ESTABLISHES cleanly (same
  // stack_id, identical well-formed snapshot digest, strictly lower layer,
  // begun predecessor) — the fail-closed PRIVATE_CHECKPOINT-predecessor check
  // is then the ONLY thing rejecting this state. Neutralising that check makes
  // the validator ACCEPT it (exit 0), so this test fails — the mutation
  // coverage the stackless variant cannot provide.
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
        candidate_tree: SHA,
        charter_digest: DIGEST,
        context_packet_digest: DIGEST,
        evidence_digest: DIGEST,
      }),
  );
  const r = run(dag, state, "live");
  assert.notEqual(r.status, 0, "stacked REVIEW successor over a PRIVATE_CHECKPOINT predecessor must fail");
  assert.match(
    r.err,
    /block A2 is REVIEW with predecessor A1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block \(contracts\/stacked-prs\.md\), which this validator does not model — fail closed/,
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
  assert.match(
    r.err,
    /state program_dag_digest .* does not match the SHA-256 of the DAG file/,
  );
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
  assert.match(
    r.err,
    /state block A0 has evidence_digest .* but no evidence artifact under/,
  );
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
