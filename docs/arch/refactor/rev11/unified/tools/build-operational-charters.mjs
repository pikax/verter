#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority } from "./lib.mjs";

const check = process.argv.includes("--check");
const authority = loadAuthority(PACKAGE_ROOT);
const byId = new Map(authority.nodes.map((node) => [node.id, node]));
const outputs = new Map();

function predecessorSection(node) {
  const receipts = node.predecessors.length
    ? node.predecessors.map((id) => `- **${id}:** exact current receipt ID and digest for “${byId.get(id).name}”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.`)
    : ["- **Direct DAG predecessors:** none. This is a source-canonical entry; its external requirements remain mandatory and are not predecessor substitutes."];
  const external = node.external_requirements.length
    ? node.external_requirements.map((requirement) => `- **External custody ${requirement}:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.`)
    : ["- **External custody:** no node-specific external authorization beyond the package activation boundary."];
  return `## Exact predecessor contracts\n\n${[...receipts, ...external].join("\n")}`;
}

for (const node of authority.nodes) {
  const file = path.join(PACKAGE_ROOT, node.charter);
  let text = fs.readFileSync(file, "utf8");
  if (node.review_profile !== "history") {
    text = text.replace(/^## Exact predecessor contracts\n[\s\S]*?(?=^## )/m, `${predecessorSection(node)}\n\n`);
    text = text.replace(/^- Mutation boundary: only the exact files, symbols, routes, and migration rows assigned to `[A-Z][A-Z0-9-]*::[a-z][a-z0-9_]+`; sibling ownership is excluded\.$/m,
      "- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.");
    text = text.replace(/^- \*\*Leaf boundary:\*\* `[A-Z][A-Z0-9-]*::[a-z][a-z0-9_]+` is the exclusive acceptance subset for “[^”]+”; it owns no sibling API, migration population, corpus, or deletion unit\.$/m,
      "- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.");
  }
  outputs.set(file, text);
}

const stale = [...outputs].filter(([file, text]) => fs.readFileSync(file, "utf8") !== text).map(([file]) => path.relative(PACKAGE_ROOT, file));
if (check) {
  if (stale.length) { console.error(`STALE operational charters: ${stale.join(", ")}`); process.exit(1); }
  console.log(`build-operational-charters: PASS (${outputs.size} exact charters)`);
} else {
  for (const [file, text] of outputs) fs.writeFileSync(file, text);
  console.log(`build-operational-charters: wrote ${outputs.size} exact charters`);
}
