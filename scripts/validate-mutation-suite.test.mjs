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
    candidate_tree: "",
    accepted_sha: "",
    accepted_tree: "",
    landing_equivalence_digest: "",
    evidence_digest: "",
    entry_lock_digest: "",
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

test("[PS] repository tip cannot be resolved (zero commits)", () => {
  const dag = write("dag-git6.toml", DAG1);
  const state = write("state-git6.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { cwd: emptyGitRepo });
  expectCheck(PS_FILE, "could not resolve the repository tip", r);
});

test("[PS] rev-list subprocess failure", () => {
  const dag = write("dag-git7.toml", DAG1);
  const state = write("state-git7.toml", header({ current: "A0" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("rev-list") });
  expectCheck(PS_FILE, "could not enumerate commits reachable from the repository tip", r);
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

test("[PS] merge-base subprocess failure", () => {
  const dag = write("dag-git9.toml", DAG1);
  const state = write(
    "state-git9.toml",
    header({ current: "A0" }) +
      acceptedBlock("A0", {
        base_sha: SHA_BASE,
        accepted_sha: SHA,
        landing_equivalence_digest: DIGEST,
      }),
  );
  const r = runPS(dag, state, "live", [], { env: fakeGitEnv("merge-base") });
  expectCheck(PS_FILE, "ancestry against accepted_sha", r);
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
// PROGRAM-STATE — single IN_PROGRESS / current_block binding
// =====================================================================

test("[PS] more than one block IN_PROGRESS", () => {
  const dag = write(
    "dag-cur1.toml",
    dagText([dagBlock({ id: "A0", predecessors: [] }), dagBlock({ id: "B0", predecessors: [] })]),
  );
  const state = write(
    "state-cur1.toml",
    header({
      current: "A0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) +
      block("A0", "IN_PROGRESS") +
      "\n" +
      block("B0", "IN_PROGRESS"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "more than one block IN_PROGRESS", r);
});

test("[PS] current_block names no state block", () => {
  const dag = write("dag-cur2.toml", DAG1);
  const state = write("state-cur2.toml", header({ current: "ZZZ" }) + block("A0", "LOCKED"));
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "names no state block", r);
});

test("[PS] IN_PROGRESS block disagrees with current_block", () => {
  const dag = write(
    "dag-cur3.toml",
    dagText([dagBlock({ id: "A0", predecessors: [] }), dagBlock({ id: "B0", predecessors: [] })]),
  );
  const state = write(
    "state-cur3.toml",
    header({
      current: "B0",
      dagDigest: createHash("sha256").update(readFileSync(dag)).digest("hex"),
    }) +
      block("A0", "IN_PROGRESS") +
      "\n" +
      block("B0", "LOCKED"),
  );
  const r = runPS(dag, state, "live");
  expectCheck(PS_FILE, "is IN_PROGRESS but current_block is", r);
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
