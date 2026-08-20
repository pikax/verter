// Tests for scripts/effective-state.mjs:
//   node --test scripts/effective-state.test.mjs
//
// Every detection class gets a fixture pair: one that TRIPS it and one clean
// fixture (usually the shared BASE fixtures below) that does NOT. A test
// that passes against both a good and a bad fixture detects nothing — see
// each `test()` below for the paired assertion.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const HERE = dirname(fileURLToPath(import.meta.url));
const GENERATOR = join(HERE, "effective-state.mjs");

let dir;
before(() => {
  dir = mkdtempSync(join(tmpdir(), "effective-state-test-"));
});
after(() => {
  rmSync(dir, { recursive: true, force: true });
});

let seq = 0;
function freshDir(label) {
  const p = join(dir, `${label}-${seq++}`);
  mkdirSync(p, { recursive: true });
  return p;
}

function writeFile(root, name, content) {
  const p = join(root, name);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, content, "utf8");
  return p;
}

// -- Minimal fixture builders

function dagToml(blocks) {
  let out = 'schema = 1\nrevision = 11\nentry_gate = "A0"\nfinal_gate = "A0"\n\n';
  for (const b of blocks) {
    out += `[[block]]\nid = "${b.id}"\nname = "${b.id} fixture"\nclass = "${b.class ?? "foundational"}"\npredecessors = [${(b.predecessors ?? []).map((p) => `"${p}"`).join(", ")}]\n\n`;
  }
  return out;
}

function stateToml(blocks, { evidenceRoots } = {}) {
  let out = `schema = 1\nrevision = 11\nstatus = "ACTIVE"\ncurrent_block = "${blocks[0]?.id ?? ""}"\n\n`;
  if (evidenceRoots) {
    out += `[orchestration]\nevidence_roots = [${evidenceRoots.map((r) => `"${r}"`).join(", ")}]\n\n`;
  }
  for (const b of blocks) {
    out += `[[block]]\nid = "${b.id}"\nstatus = "${b.status ?? "LOCKED"}"\n`;
    out += `stack_id = "${b.stack_id ?? ""}"\n`;
    if (b.evidence_digest) out += `evidence_digest = "${b.evidence_digest}"\n`;
    if (b.enabling_amendment) out += `enabling_amendment = "${b.enabling_amendment}"\n`;
    out += "\n";
  }
  return out;
}

// A minimal, well-formed ruling frontmatter block, with sensible defaults
// for every field the parser/generator reads.
function rulingFile({
  ruling_id,
  binds = [],
  supersedes = [],
  superseded_by = [],
  bodyExtra = "",
}) {
  const seqPart = (arr) =>
    arr.length === 0
      ? "[]"
      : "\n" +
        arr
          .map(
            (e) =>
              `  - ${Object.entries(e)
                .map(([k, v], i) => `${i === 0 ? "" : "    "}${k}: "${v}"`)
                .join("\n")}`,
          )
          .join("\n");
  return `---
ruling_id: "${ruling_id}"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "stated"
binds: [${binds.map((b) => `"${b}"`).join(", ")}]
source_file: "${ruling_id}.md"
summary: "fixture ruling ${ruling_id}"
supersedes:${seqPart(supersedes)}
superseded_by:${seqPart(superseded_by)}
contradicts: []
notes: "fixture"
---

# ${ruling_id}

fixture body.${bodyExtra}
`;
}

function run(args) {
  const res = spawnSync(process.execPath, [GENERATOR, "--json", ...args], { encoding: "utf8" });
  let json = null;
  try {
    json = JSON.parse(res.stdout);
  } catch {
    // leave json null — some tests assert on stderr/status instead
  }
  return { status: res.status, out: res.stdout ?? "", err: res.stderr ?? "", json };
}

function findingsOfType(json, type) {
  return (json?.findings ?? []).filter((f) => f.type === type);
}

// Standard args pointing at a fixture root's dag.toml/state.toml/rulings/amendments.
function stdArgs(root, overrides = {}) {
  return [
    "--dag",
    overrides.dag ?? join(root, "dag.toml"),
    "--state",
    overrides.state ?? join(root, "state.toml"),
    "--rulings-dir",
    overrides.rulingsDir ?? join(root, "rulings"),
    "--amendments-dir",
    overrides.amendmentsDir ?? join(root, "amendments"),
    "--authority-registry",
    overrides.authorityRegistry ?? join(root, "authority-registry.toml"),
  ];
}

function baseFixture(root, { dagBlocks, stateBlocks, evidenceRoots } = {}) {
  const blocks = dagBlocks ?? [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(
    root,
    "state.toml",
    stateToml(stateBlocks ?? blocks.map((b) => ({ id: b.id, status: "ACCEPTED" })), {
      evidenceRoots,
    }),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  return blocks;
}

// -- 1. DAG edge referencing an unknown block

test("DAG_EDGE_UNKNOWN_BLOCK: trips on an edge to a nonexistent block", () => {
  const root = freshDir("dag-unknown-edge");
  baseFixture(root, {
    dagBlocks: [
      { id: "A0", predecessors: [] },
      { id: "A1", predecessors: ["A0", "ZZ9"] },
    ],
  });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "DAG_EDGE_UNKNOWN_BLOCK").length, 1);
});

test("DAG_EDGE_UNKNOWN_BLOCK: clean fixture does not trip it", () => {
  const root = freshDir("dag-unknown-edge-clean");
  baseFixture(root);
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 0);
  assert.equal(findingsOfType(json, "DAG_EDGE_UNKNOWN_BLOCK").length, 0);
});

// -- 2. block set disagreement (DAG vs ledger) — covers "referenced but absent"
// at the DAG/ledger level (the ruling-level pair is class 4 below).

test("LEDGER_BLOCK_MISSING: trips when the ledger drops a DAG block", () => {
  const root = freshDir("ledger-missing");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(root, "state.toml", stateToml([{ id: "A0", status: "ACCEPTED" }]));
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "LEDGER_BLOCK_MISSING").length, 1);
});

test("LEDGER_BLOCK_MISSING: clean fixture does not trip it", () => {
  const root = freshDir("ledger-missing-clean");
  baseFixture(root);
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "LEDGER_BLOCK_MISSING").length, 0);
});

test("DAG_BLOCK_MISSING: trips when the ledger carries an extra block the DAG doesn't know", () => {
  const root = freshDir("dag-missing");
  const blocks = [{ id: "A0", predecessors: [] }];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(
    root,
    "state.toml",
    stateToml([
      { id: "A0", status: "ACCEPTED" },
      { id: "ZZ9", status: "LOCKED" },
    ]),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "DAG_BLOCK_MISSING").length, 1);
});

test("DAG_BLOCK_MISSING: clean fixture does not trip it", () => {
  const root = freshDir("dag-missing-clean");
  baseFixture(root);
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "DAG_BLOCK_MISSING").length, 0);
});

// -- 3. a block whose status is inconsistent with its DAG predecessors

test("STATUS_PREDECESSOR_INCONSISTENT: trips when a begun block's predecessor is not ACCEPTED", () => {
  const root = freshDir("status-pred");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(
    root,
    "state.toml",
    stateToml([
      { id: "A0", status: "LOCKED" },
      { id: "A1", status: "IN_PROGRESS" },
    ]),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "STATUS_PREDECESSOR_INCONSISTENT").length, 1);
});

test("STATUS_PREDECESSOR_INCONSISTENT: clean fixture (predecessor ACCEPTED) does not trip it", () => {
  const root = freshDir("status-pred-clean");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(
    root,
    "state.toml",
    stateToml([
      { id: "A0", status: "ACCEPTED" },
      { id: "A1", status: "IN_PROGRESS" },
    ]),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 0);
  assert.equal(findingsOfType(json, "STATUS_PREDECESSOR_INCONSISTENT").length, 0);
});

test("STATUS_PREDECESSOR_INCONSISTENT: a non-empty stack_id is left to the stack-window validator, not flagged here", () => {
  const root = freshDir("status-pred-stacked");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(
    root,
    "state.toml",
    stateToml([
      { id: "A0", status: "IN_PROGRESS" },
      { id: "A1", status: "IN_PROGRESS", stack_id: "stack-1" },
    ]),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "STATUS_PREDECESSOR_INCONSISTENT").length, 0);
});

// -- 4. a block referenced by a ruling but absent from the ledger (and vice versa)

test("RULING_BLOCK_UNKNOWN: trips when a ruling binds a block id the DAG has never heard of", () => {
  const root = freshDir("ruling-unknown-block");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1", binds: ["ZZ9"] }));
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "RULING_BLOCK_UNKNOWN").length, 1);
});

test("RULING_BLOCK_UNKNOWN: clean fixture (binds a real block) does not trip it", () => {
  const root = freshDir("ruling-unknown-block-clean");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1", binds: ["A0"] }));
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "RULING_BLOCK_UNKNOWN").length, 0);
});

test("RULING_BLOCK_MISSING_FROM_LEDGER: trips when a ruling binds a DAG-known block the ledger dropped", () => {
  const root = freshDir("ruling-missing-ledger");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  // A1 exists in the DAG but has no ledger row.
  writeFile(root, "state.toml", stateToml([{ id: "A0", status: "ACCEPTED" }]));
  mkdirSync(join(root, "amendments"), { recursive: true });
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1", binds: ["A1"] }));
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  // Both LEDGER_BLOCK_MISSING (class 2) and RULING_BLOCK_MISSING_FROM_LEDGER
  // (class 4) fire on this fixture — they are independent detectors over the
  // same underlying gap, which is exactly the point: two authorities (the
  // DAG's own block list, and a ruling's binds) both independently notice
  // the same ledger drop.
  assert.equal(findingsOfType(json, "RULING_BLOCK_MISSING_FROM_LEDGER").length, 1);
});

test("RULING_BLOCK_MISSING_FROM_LEDGER: clean fixture (block present in ledger) does not trip it", () => {
  const root = freshDir("ruling-missing-ledger-clean");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1", binds: ["A1"] }));
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "RULING_BLOCK_MISSING_FROM_LEDGER").length, 0);
});

// -- resolved supersedes chains: an unknown target and a genuine cycle
//
// NOTE: a ruling's `superseded_by`/`supersedes` claim is deliberately
// one-sided in this corpus — see docs/arch/refactor/rev11/rulings/INDEX.md:
// "Do not treat `superseded_by = —` as proof a ruling is uncontested; it
// means no OTHER migrated ruling's own text names it as superseded." A
// newer ruling is under no obligation to reciprocate an older ruling's
// `superseded_by` claim in its own `supersedes` list (its `supersedes`
// field is reserved for citing pre-corpus/non-migrated documents). An
// asymmetry-based detector was tried and removed: it fired on 5 of the 6
// asymmetric declarations actually present in the real corpus, all false
// positives against the documented convention. The one thing actually worth
// detecting on a claim edge — a `ruling` target that doesn't exist in the
// corpus at all — is covered below by RULING_SUPERSESSION_TARGET_UNKNOWN.

test("RULING_SUPERSESSION_TARGET_UNKNOWN: trips when a supersedes entry names a nonexistent ruling_id", () => {
  const root = freshDir("ruling-target-unknown");
  baseFixture(root);
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({ ruling_id: "R1", supersedes: [{ ruling: "GHOST", claim: "x" }] }),
  );
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "RULING_SUPERSESSION_TARGET_UNKNOWN").length, 1);
});

test("RULING_SUPERSESSION_CYCLE: a cycle is reported as a finding, not a crash", () => {
  const root = freshDir("ruling-cycle");
  baseFixture(root);
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({
      ruling_id: "R1",
      supersedes: [{ ruling: "R2", claim: "x" }],
      superseded_by: [{ ruling: "R2", claim: "y" }],
    }),
  );
  writeFile(
    root,
    "rulings/R2.md",
    rulingFile({
      ruling_id: "R2",
      supersedes: [{ ruling: "R1", claim: "y" }],
      superseded_by: [{ ruling: "R1", claim: "x" }],
    }),
  );
  const { json, status } = run(stdArgs(root));
  // Resolved without crashing (a well-formed JSON report, not a stack
  // overflow / uncaught exception), and the cycle is reported.
  assert.ok(json, "generator must not crash on a supersession cycle");
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "RULING_SUPERSESSION_CYCLE").length, 1);
});

test("RULING_SUPERSESSION_CYCLE / TARGET_UNKNOWN: clean fixture (acyclic, known targets) does not trip either", () => {
  const root = freshDir("ruling-cycle-clean");
  baseFixture(root);
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({ ruling_id: "R1", superseded_by: [{ ruling: "R2", claim: "x" }] }),
  );
  writeFile(
    root,
    "rulings/R2.md",
    rulingFile({ ruling_id: "R2", supersedes: [{ ruling: "R1", claim: "x" }] }),
  );
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "RULING_SUPERSESSION_CYCLE").length, 0);
  assert.equal(findingsOfType(json, "RULING_SUPERSESSION_TARGET_UNKNOWN").length, 0);
});

// -- duplicate ruling_id

test("DUPLICATE_RULING_ID: trips when two files declare the same ruling_id", () => {
  const root = freshDir("ruling-dup");
  baseFixture(root);
  writeFile(root, "rulings/R1a.md", rulingFile({ ruling_id: "SAME" }));
  writeFile(root, "rulings/R1b.md", rulingFile({ ruling_id: "SAME" }));
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "DUPLICATE_RULING_ID").length, 1);
});

test("DUPLICATE_RULING_ID: clean fixture (distinct ids) does not trip it", () => {
  const root = freshDir("ruling-dup-clean");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1" }));
  writeFile(root, "rulings/R2.md", rulingFile({ ruling_id: "R2" }));
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "DUPLICATE_RULING_ID").length, 0);
});

// -- 6. an artifact digest cited by a ledger row whose file is missing

const FAKE_DIGEST = "a".repeat(64);

test("ARTIFACT_DIGEST_FILE_MISSING: trips when evidence_digest is set but nothing resolves under the evidence roots", () => {
  const root = freshDir("artifact-missing");
  const blocks = [{ id: "A0", predecessors: [] }];
  writeFile(root, "dag.toml", dagToml(blocks));
  const evidenceRoot = join(root, "evidence");
  mkdirSync(evidenceRoot, { recursive: true });
  writeFile(
    root,
    "state.toml",
    stateToml([{ id: "A0", status: "ACCEPTED", evidence_digest: FAKE_DIGEST }], {
      evidenceRoots: [evidenceRoot],
    }),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "ARTIFACT_DIGEST_FILE_MISSING").length, 1);
});

test("ARTIFACT_DIGEST_FILE_MISSING: clean fixture (artifact present) does not trip it", () => {
  const root = freshDir("artifact-missing-clean");
  const blocks = [{ id: "A0", predecessors: [] }];
  writeFile(root, "dag.toml", dagToml(blocks));
  const evidenceRoot = join(root, "evidence");
  writeFile(evidenceRoot, "A0-summary.md", "fixture artifact\n");
  writeFile(
    root,
    "state.toml",
    stateToml([{ id: "A0", status: "ACCEPTED", evidence_digest: FAKE_DIGEST }], {
      evidenceRoots: [evidenceRoot],
    }),
  );
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "ARTIFACT_DIGEST_FILE_MISSING").length, 0);
});

// -- 7. a missing DAG edge implied by a ruling but absent from program-dag.toml
// (this is the class that must detect the real corpus's known B6 -> C2 gap;
// the fixture pair proves the mechanism generically first.)

test("MISSING_DAG_EDGE_IMPLIED_BY_RULING: trips when a ruling says to add an edge the DAG doesn't have", () => {
  const root = freshDir("missing-edge");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: [] }, // deliberately NOT listing A0 as a predecessor
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(root, "state.toml", stateToml(blocks.map((b) => ({ id: b.id, status: "ACCEPTED" }))));
  mkdirSync(join(root, "amendments"), { recursive: true });
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({
      ruling_id: "R1",
      bodyExtra: "\n\nRuling text: add DAG edge A0->A1 — requires a formal DAG amendment.\n",
    }),
  );
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 1);
  assert.equal(findingsOfType(json, "MISSING_DAG_EDGE_IMPLIED_BY_RULING").length, 1);
});

test("MISSING_DAG_EDGE_IMPLIED_BY_RULING: clean fixture (edge already present in the DAG) does not trip it", () => {
  const root = freshDir("missing-edge-clean");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: ["A0"] }, // edge already present
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(root, "state.toml", stateToml(blocks.map((b) => ({ id: b.id, status: "ACCEPTED" }))));
  mkdirSync(join(root, "amendments"), { recursive: true });
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({
      ruling_id: "R1",
      bodyExtra: "\n\nRuling text: add DAG edge A0->A1 for clarity.\n",
    }),
  );
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "MISSING_DAG_EDGE_IMPLIED_BY_RULING").length, 0);
});

test("MISSING_DAG_EDGE_IMPLIED_BY_RULING: incidental 'X -> Y' prose without 'add ... edge' phrasing does not trip it", () => {
  const root = freshDir("missing-edge-incidental");
  const blocks = [
    { id: "A0", predecessors: [] },
    { id: "A1", predecessors: [] },
  ];
  writeFile(root, "dag.toml", dagToml(blocks));
  writeFile(root, "state.toml", stateToml(blocks.map((b) => ({ id: b.id, status: "ACCEPTED" }))));
  mkdirSync(join(root, "amendments"), { recursive: true });
  writeFile(
    root,
    "rulings/R1.md",
    rulingFile({
      ruling_id: "R1",
      bodyExtra:
        "\n\nStatus transition A0 -> A1 happened during review, unrelated to DAG structure.\n",
    }),
  );
  const { json } = run(stdArgs(root));
  assert.equal(findingsOfType(json, "MISSING_DAG_EDGE_IMPLIED_BY_RULING").length, 0);
});

// -- Robustness / usage

test("unparseable DAG TOML fails loudly with exit 2, not a silent partial run", () => {
  const root = freshDir("bad-toml");
  writeFile(root, "dag.toml", "this is not { valid toml\n");
  writeFile(root, "state.toml", stateToml([{ id: "A0", status: "ACCEPTED" }]));
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { status, err } = run(stdArgs(root));
  assert.equal(status, 2);
  assert.match(err, /VIOLATION|unparseable/);
});

test("unparseable ruling frontmatter fails loudly with exit 2", () => {
  const root = freshDir("bad-frontmatter");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", "no frontmatter fence at all\n");
  const { status, err } = run(stdArgs(root));
  assert.equal(status, 2);
  assert.match(err, /FrontmatterError|frontmatter fence|VIOLATION/);
});

test("authority-registry.toml absence is tolerated, not an error", () => {
  const root = freshDir("no-authority-registry");
  baseFixture(root);
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 0);
  assert.equal(json.authorityRegistry.present, false);
});

test("authority-registry.toml presence is tolerated too", () => {
  const root = freshDir("with-authority-registry");
  baseFixture(root);
  writeFile(root, "authority-registry.toml", "schema = 1\n");
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 0);
  assert.equal(json.authorityRegistry.present, true);
});

test("a run with zero rulings and zero blocks still produces a well-formed report (no crash)", () => {
  const root = freshDir("empty");
  writeFile(root, "dag.toml", dagToml([]));
  writeFile(root, "state.toml", stateToml([]));
  mkdirSync(join(root, "rulings"), { recursive: true });
  mkdirSync(join(root, "amendments"), { recursive: true });
  const { json, status } = run(stdArgs(root));
  assert.equal(status, 0);
  assert.deepEqual(json.blocks, []);
  assert.deepEqual(json.rulings, []);
});

test("output is deterministic across repeated runs against the same input", () => {
  const root = freshDir("determinism");
  baseFixture(root);
  writeFile(root, "rulings/R1.md", rulingFile({ ruling_id: "R1", binds: ["ZZ9"] }));
  writeFile(root, "rulings/R2.md", rulingFile({ ruling_id: "R2", binds: ["ZZ8"] }));
  const first = run(stdArgs(root));
  const second = run(stdArgs(root));
  assert.equal(first.out, second.out);
});

// -- Integration: the real program tree

test("real program tree: generator runs to completion and reports the known B6 -> C2 gap", () => {
  const res = spawnSync(process.execPath, [GENERATOR, "--json"], { encoding: "utf8" });
  assert.ok(res.stdout, "generator produced no stdout against the real tree");
  const json = JSON.parse(res.stdout);
  assert.ok(
    json.blocks.length > 0,
    "zero blocks derived from the real ledger — non-vacuous work check",
  );
  assert.ok(json.rulings.length > 0, "zero rulings derived from the real corpus");
  const missingEdge = findingsOfType(json, "MISSING_DAG_EDGE_IMPLIED_BY_RULING").find(
    (f) => f.from === "B6" && f.to === "C2",
  );
  assert.ok(
    missingEdge,
    "expected the known missing B6 -> C2 DAG edge to be detected against the real corpus",
  );
});
