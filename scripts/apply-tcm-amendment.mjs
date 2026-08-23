#!/usr/bin/env node
// Apply the TypeScript content-mapper amendment: add TCM0-TCM4 to the DAG, add
// matching LOCKED rows to the ledger, and refresh the DAG digest.
//
// Run this ONLY after block/ledger-ratification has landed — it edits
// program-state.toml, which that block currently owns.
//
// usage: node scripts/apply-tcm-amendment.mjs [--check]
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";

const DAG = "docs/arch/refactor/rev11/program-dag.toml";
const STATE = "docs/arch/architecture-lock/ledger/program-state.toml";
const check = process.argv.includes("--check");

const BLOCKS = [
  ["TCM0", "Current TypeScript contract and dual-plane architecture lock", "foundational", ["A6"]],
  ["TCM1", "Compact mapping products inside CodeTransform", "foundational", ["TCM0"]],
  ["TCM2", "Content-mapper projection plane (dormant until TCM4)", "framework subsystem", ["TCM0", "TCM1"]],
  ["TCM3", "TypeScript semantic capability closure (dormant until TCM4)", "framework subsystem", ["TCM0", "TCM1"]],
  ["TCM4", "Atomic activation and deletion", "foundational", ["TCM0", "TCM1", "TCM2", "TCM3"]],
];

let dag = readFileSync(DAG, "utf8");
if (dag.includes('"TCM0"')) { console.log("DAG already amended"); }
else {
  dag = dag.replace(/\n+$/, "\n") + `
# ── TypeScript content-mapper amendment train (DISC-2026-08-22) ──────────────
# NO block here may be dispatched until it holds a digest-bound authorization
# record in authority-registry.toml — MAINTAINER-RULING-2026-08-22-BV2-B5-J1 §6.
` + BLOCKS.map(([id, name, cls, preds]) =>
`
[[block]]
id = "${id}"
name = "${name}"
class = "${cls}"
predecessors = [${preds.map((p) => `"${p}"`).join(", ")}]
`).join("");
  if (!check) writeFileSync(DAG, dag);
  console.log(`DAG: +${BLOCKS.length} blocks`);
}

// LOCKED rows mirror the shape of existing locked entries: every identity,
// evidence and review field empty. LOCKED with empty fields is the accurate
// state — these blocks are drafted, not authorised.
let state = readFileSync(STATE, "utf8");
const rowFor = (id) => `
[[block]]
id = "${id}"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
conformance_reviewed_sha = ""
architecture_review = "PENDING"
architecture_reviewed_sha = ""
adversarial_review = "PENDING"
adversarial_reviewed_sha = ""
maintainer_decision = "PENDING"
enabling_amendment = "DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS"
notes = "Drafted by the content-mapper amendment train. Charter at docs/arch/refactor/rev11/charters/${id}.md. NOT authorised: requires a digest-bound authority-registry record before dispatch."
`;
for (const [id] of BLOCKS) {
  if (state.includes(`id = "${id}"`)) continue;
  state = state.replace(/\n+$/, "\n") + rowFor(id);
}
// refresh the DAG digest the state pins
const digest = createHash("sha256").update(readFileSync(DAG)).digest("hex");
state = state.replace(/^(\s*program_dag_digest\s*=\s*)".*"$/m, `$1"${digest}"`);
if (!check) writeFileSync(STATE, state);
console.log(`state: +rows, program_dag_digest -> ${digest.slice(0, 16)}…`);
console.log(check ? "(--check: nothing written)" : "applied");
