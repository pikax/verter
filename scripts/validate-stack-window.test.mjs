// Tests for scripts/validate-stack-window.mjs (AMD-001 §1 — the Node
// stack-window validator) and scripts/lib/stack-window-lib.mjs (the shared
// model reused by scripts/validate-program-state.mjs's composite checkpoint
// exception — see scripts/validate-program-state.test.mjs for the D1/D2
// transition fixture exercised through THAT entry point).
//
//   node --test scripts/validate-stack-window.test.mjs
//
// Every negative case asserts both a non-zero exit AND the specific
// violation text — a validator stubbed to always exit non-zero, or one that
// merges every reason into one generic message, would not survive these.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const VALIDATOR = join(dirname(fileURLToPath(import.meta.url)), "validate-stack-window.mjs");

let dir;
before(() => {
  dir = mkdtempSync(join(tmpdir(), "validate-stack-window-"));
});
after(() => {
  rmSync(dir, { recursive: true, force: true });
});

function write(name, content) {
  const p = join(dir, name);
  writeFileSync(p, content, "utf8");
  return p;
}

function run(args) {
  const res = spawnSync(process.execPath, [VALIDATOR, ...args], { encoding: "utf8" });
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "" };
}

const DIGEST = createHash("sha256").update("stack-window-test-digest").digest("hex");
const DIGEST2 = createHash("sha256").update("stack-window-test-digest-2").digest("hex");
const SHA = "0000000000000000000000000000000000000001";

// One [[layer]] block as a TOML fragment.
function layer(overrides = {}) {
  const fields = {
    index: 1,
    layer_id: "L1",
    block_id: "B1",
    charter_digest: DIGEST,
    kind: "mergeable",
    branch: "b1",
    base_branch: "main",
    worktree: "wt1",
    worker: "w1",
    pr_number: 0,
    pr_url: "",
    base_sha: SHA,
    base_tree: SHA,
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

// Top-level window fields, `mode`-parameterized. `extraLayers` is raw TOML
// text appended after the required first layer.
function window({ mode = "LANDABLE", acceptanceBlockId = "", status = "ACTIVE", overrides = {}, extraLayers = "" } = {}) {
  const fields = {
    schema: 1,
    revision: 11,
    status,
    mode,
    stack_id: "S1",
    acceptance_block_id: acceptanceBlockId,
    authority_package_digest: DIGEST,
    implementation_lock_digest: DIGEST,
    program_state_basis_digest: DIGEST,
    previous_stack_snapshot_digest: "NOT_APPLICABLE",
    root_branch: "main",
    root_base_sha: SHA,
    root_base_tree: SHA,
    stack_tool: "LOCAL_BRANCH_CHAIN",
    stack_tool_version: "git 2.x",
    landing_mode: "bottom-up",
    max_open_layers: 4,
    owner: "orchestrator",
    evidence_root: "docs/arch/refactor/rev11/evidence",
    ...overrides,
  };
  const lines = Object.entries(fields).map(([k, val]) => (typeof val === "number" ? `${k} = ${val}` : `${k} = "${val}"`));
  return (
    `${lines.join("\n")}\n` +
    `shared_writer_surfaces = []\nintegration_commands = []\nnotes = ""\n\n` +
    layer() +
    extraLayers
  );
}

// -- Basic template-mode acceptance against the real repository template.
test("template mode accepts a self-contained stack-window fixture", () => {
  const template = write("stack-window.template.toml", window({ status: "TEMPLATE", overrides: {
    stack_id: "REQUIRED_STACK_ID",
    authority_package_digest: "REQUIRED_PACKAGE_SHA256",
    implementation_lock_digest: "REQUIRED_A6_LOCK_SHA256",
    program_state_basis_digest: "REQUIRED_PRE_STACK_PROGRAM_STATE_SHA256",
    root_branch: "REQUIRED_ROOT_BRANCH",
    root_base_sha: "REQUIRED_FULL_SHA",
    root_base_tree: "REQUIRED_TREE_OID",
    stack_tool: "REQUIRED_STACK_TOOL",
    stack_tool_version: "REQUIRED_STACK_TOOL_VERSION",
    owner: "REQUIRED_ORCHESTRATOR_OR_STACK_OWNER",
    evidence_root: "REQUIRED_REPOSITORY_RELATIVE_PATH",
  } }));
  const r = run([
    "--window",
    template,
    "--mode",
    "template",
  ]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  assert.match(r.out, /StackSnapshotId \(SHA-256\) = [0-9a-f]{64}/);
});

// -- Usage / IO failures
test("usage failure: missing required flags exits 2", () => {
  const r = run(["--window", write("w1.toml", window())]);
  assert.equal(r.status, 2);
  assert.match(r.err, /--window and --mode are both required/);
});

test("unreadable window file exits 2", () => {
  const r = run(["--window", join(dir, "does-not-exist.toml"), "--mode", "live"]);
  assert.equal(r.status, 2);
  assert.match(r.err, /cannot read stack-window file/);
});

test("unparseable TOML exits 1 with a VIOLATION line, not a silent pass", () => {
  const p = write("bad.toml", 'mode = "LANDABLE\n'); // unterminated string
  const r = run(["--window", p, "--mode", "live"]);
  assert.equal(r.status, 1);
  assert.match(r.err, /VIOLATION:.*unterminated string/);
  assert.match(r.err, /FAIL: 0 checks completed/);
});

// -- Mode-agnostic structural rules
test("unknown top-level mode value is rejected", () => {
  const p = write("bad-mode.toml", window({ mode: "SOMETHING_ELSE" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /top-level mode "SOMETHING_ELSE" is outside the declared enum \{LANDABLE, ATOMIC_REVIEW\}/);
});

test("live mode rejects status = TEMPLATE", () => {
  const p = write("still-template.toml", window({ status: "TEMPLATE" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /live window still carries status = "TEMPLATE"/);
});

test("live mode rejects an unresolved digest field", () => {
  const p = write("bad-digest.toml", window({ overrides: { authority_package_digest: "REQUIRED_X" } }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /top-level authority_package_digest is not a resolved 64-char lowercase SHA-256/);
});

test("template mode accepts a REQUIRED_ placeholder digest", () => {
  const p = write("template-digest.toml", window({ status: "TEMPLATE", overrides: { authority_package_digest: "REQUIRED_X" } }));
  const r = run(["--window", p, "--mode", "template"]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
});

test("max_open_layers outside [2, 6] is rejected", () => {
  const p = write("bad-max.toml", window({ overrides: { max_open_layers: 7 } }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /max_open_layers 7 is not an integer in \[2, 6\]/);
});

test("layer count exceeding the window's own max_open_layers is rejected", () => {
  const p = write(
    "too-many-layers.toml",
    window({ overrides: { max_open_layers: 2 }, extraLayers: layer({ index: 2, layer_id: "L2", block_id: "B2" }) + layer({ index: 3, layer_id: "L3", block_id: "B3" }) }),
  );
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /declares 3 layer\(s\), exceeding its own max_open_layers = 2/);
});

test("duplicate layer_id is rejected", () => {
  const p = write("dup-layer-id.toml", window({ extraLayers: layer({ index: 2, layer_id: "L1", block_id: "B2" }) }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /declares duplicate layer_id "L1"/);
});

test("duplicate layer index is rejected", () => {
  const p = write("dup-index.toml", window({ extraLayers: layer({ index: 1, layer_id: "L2", block_id: "B2" }) }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /declares duplicate layer index 1/);
});

test("malformed layer charter_digest is rejected", () => {
  const p = write("bad-layer-digest.toml", window({ extraLayers: "" }).replace(`charter_digest = "${DIGEST}"`, 'charter_digest = "not-a-digest"'));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /layer\[0\] charter_digest is not a resolved SHA-256/);
});

// -- LANDABLE-mode rules (contracts/stacked-prs.md 3.1)
test("LANDABLE: non-empty acceptance_block_id is rejected", () => {
  const p = write("landable-bad-acceptance.toml", window({ mode: "LANDABLE", acceptanceBlockId: "B1" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /mode is LANDABLE but acceptance_block_id is non-empty "B1"/);
});

test("LANDABLE: duplicate block_id across layers is rejected", () => {
  const p = write(
    "landable-dup-block.toml",
    window({ mode: "LANDABLE", extraLayers: layer({ index: 2, layer_id: "L2", block_id: "B1" }) }),
  );
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /mode is LANDABLE but block_id "B1" appears 2 times/);
});

test("LANDABLE: two distinct block_ids passes", () => {
  const p = write(
    "landable-ok.toml",
    window({ mode: "LANDABLE", extraLayers: layer({ index: 2, layer_id: "L2", block_id: "B2", base_branch: "b1" }) }),
  );
  const r = run(["--window", p, "--mode", "live"]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
});

// -- ATOMIC_REVIEW-mode rules (contracts/stacked-prs.md 3.2)
function atomicWindow({ acceptanceBlockId = "D2", d1Kind = "NON_MERGEABLE_PRIVATE_LAYER", d2Kind = "mergeable", overrides = {} } = {}) {
  return window({
    mode: "ATOMIC_REVIEW",
    acceptanceBlockId,
    overrides: { max_open_layers: 2, ...overrides },
    extraLayers: "", // replaced below — base layer() is not D1/D2 shaped
  })
    .split("[[layer]]")[0]
    .concat(
      layer({ index: 1, layer_id: "D1", block_id: "D1", kind: d1Kind, mergeable: d1Kind === "mergeable", base_branch: "main", branch: "d1" }),
      layer({ index: 2, layer_id: "D2", block_id: "D2", kind: d2Kind, mergeable: d2Kind === "mergeable", base_branch: "d1", branch: "d2" }),
    );
}

test("ATOMIC_REVIEW: empty acceptance_block_id is rejected", () => {
  const p = write("atomic-empty-acceptance.toml", atomicWindow({ acceptanceBlockId: "" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /mode is ATOMIC_REVIEW but acceptance_block_id is empty/);
});

test("ATOMIC_REVIEW: zero mergeable layers is rejected", () => {
  const p = write("atomic-zero-mergeable.toml", atomicWindow({ d2Kind: "NON_MERGEABLE_PRIVATE_LAYER" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /found 0 mergeable layer\(s\)/);
});

test("ATOMIC_REVIEW: two mergeable layers is rejected", () => {
  const p = write("atomic-two-mergeable.toml", atomicWindow({ d1Kind: "mergeable" }));
  const r = run(["--window", p, "--mode", "live"]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /found 2 mergeable layer\(s\)/);
  assert.match(r.err, /layer "D1".*kind is "mergeable", not NON_MERGEABLE_PRIVATE_LAYER/);
});

test("ATOMIC_REVIEW: a valid D1/D2 window passes structurally", () => {
  const p = write("atomic-ok.toml", atomicWindow());
  const r = run(["--window", p, "--mode", "live"]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
});

test("ATOMIC_REVIEW with --dag: a private layer naming a non-checkpoint block is rejected", () => {
  const dag = write(
    "dag.toml",
    `schema = 1\nrevision = 11\n\n[[block]]\nid = "D1"\nclass = "foundational"\npredecessors = []\n\n[[block]]\nid = "D2"\nclass = "foundational"\npredecessors = ["D1"]\n`,
  );
  const p = write("atomic-bad-class.toml", atomicWindow());
  const r = run(["--window", p, "--mode", "live", "--dag", dag]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /DAG class is "foundational" — a private ATOMIC_REVIEW layer must repeat the acceptance block's own id or name a block whose DAG class is "foundational-private-checkpoint"/);
});

test("ATOMIC_REVIEW with --dag: a private layer naming a foundational-private-checkpoint block passes", () => {
  const dag = write(
    "dag-ok.toml",
    `schema = 1\nrevision = 11\n\n[[block]]\nid = "D1"\nclass = "foundational-private-checkpoint"\npredecessors = []\n\n[[block]]\nid = "D2"\nclass = "foundational"\npredecessors = ["D1"]\n`,
  );
  const p = write("atomic-good-class.toml", atomicWindow());
  const r = run(["--window", p, "--mode", "live", "--dag", dag]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
});

// -- --current-program-state composite cross-validation
function ledger({ d1Snapshot, d2Snapshot, d1StackId = "S1", d2StackId = "S1", d1Layer = 1, d2Layer = 2, d1Status = "PRIVATE_CHECKPOINT" } = {}) {
  const row = (id, status, stackId, snapshot, layerIdx) =>
    `[[block]]\nid = "${id}"\nstatus = "${status}"\nstack_id = "${stackId}"\nstack_snapshot_digest = "${snapshot}"\nstack_layer = ${layerIdx}\n`;
  return `schema = 1\nrevision = 11\n\n${row("D1", d1Status, d1StackId, d1Snapshot, d1Layer)}\n${row("D2", "REVIEW", d2StackId, d2Snapshot, d2Layer)}\n`;
}

function snapshotOf(windowPath) {
  const text = readFileSync(windowPath, "utf8");
  return createHash("sha256").update(text).digest("hex");
}

test("--current-program-state requires --mode live", () => {
  const p = write("cps-template.toml", atomicWindow());
  const state = write("cps-state-a.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST }));
  const r = run(["--window", p, "--mode", "template", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /--current-program-state was given but --mode is "template"/);
});

test("--current-program-state: skipped with an explanatory note when structural validation fails first", () => {
  const p = write("cps-bad-structural.toml", atomicWindow({ acceptanceBlockId: "" }));
  const state = write("cps-state-b.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /cross-validation skipped — .* failed its own structural validation first/);
});

test("--current-program-state: matching ledger passes", () => {
  const p = write("cps-ok.toml", atomicWindow());
  const digest = readFileSync(p, "utf8");
  const snap = createHash("sha256").update(digest).digest("hex");
  const state = write("cps-state-ok.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
});

test("--current-program-state: stack_id mismatch is rejected", () => {
  const p = write("cps-stackid.toml", atomicWindow());
  const text = readFileSync(p, "utf8");
  const snap = createHash("sha256").update(text).digest("hex");
  const state = write("cps-state-stackid.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1StackId: "OTHER" }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /block D1 ledger stack_id "OTHER" does not match window stack_id "S1"/);
});

test("--current-program-state: stack_snapshot_digest mismatch is rejected", () => {
  const p = write("cps-snapshot.toml", atomicWindow());
  const state = write("cps-state-snapshot.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST2 }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /block D1 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/);
});

test("--current-program-state: stack_layer mismatch is rejected", () => {
  const p = write("cps-layer.toml", atomicWindow());
  const text = readFileSync(p, "utf8");
  const snap = createHash("sha256").update(text).digest("hex");
  const state = write("cps-state-layer.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1Layer: 5 }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /block D1 ledger stack_layer 5 does not match window layer index 1/);
});

test("--current-program-state: block missing from ledger is rejected", () => {
  const p = write("cps-missing.toml", atomicWindow());
  const state = write("cps-state-missing.toml", `schema = 1\nrevision = 11\n`);
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /names block_id "D1", which does not exist in the program-state ledger/);
});

test("--current-program-state: a checkpoint layer whose ledger row is not PRIVATE_CHECKPOINT is rejected", () => {
  const p = write("cps-not-checkpoint.toml", atomicWindow());
  const text = readFileSync(p, "utf8");
  const snap = createHash("sha256").update(text).digest("hex");
  const state = write("cps-state-not-checkpoint.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1Status: "ACCEPTED" }));
  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
  assert.notEqual(r.status, 0);
  assert.match(r.err, /block D1 is a NON_MERGEABLE_PRIVATE_LAYER checkpoint but ledger status is "ACCEPTED", not PRIVATE_CHECKPOINT/);
});
