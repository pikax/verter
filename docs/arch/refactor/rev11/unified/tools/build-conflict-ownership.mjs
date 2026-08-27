#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority } from "./lib.mjs";

const check = process.argv.includes("--check");
const authority = loadAuthority(PACKAGE_ROOT);
const file = path.join(PACKAGE_ROOT, "catalogs/conflict-domains.toml");
const original = fs.readFileSync(file, "utf8");
const header = original.split("\n[[domain]]\n", 1)[0];
const blocks = original.split("\n[[domain]]\n").slice(1).map((body) => `[[domain]]\n${body}`);
const scalar = (block, key) => JSON.parse(block.match(new RegExp(`^${key} = (.+)$`, "m"))?.[1] || "null");
const domains = new Map(blocks.map((block) => [scalar(block, "id"), { block, roots: scalar(block, "path_roots").map((root) => root.replace(/\/+$/, "")) }]));
const intersects = (left, right) => left === right || left.startsWith(`${right}/`) || right.startsWith(`${left}/`);
let additions = 0;
for (const node of authority.nodes.filter((candidate) => candidate.review_profile !== "history")) {
  const charter = fs.readFileSync(path.join(PACKAGE_ROOT, node.charter), "utf8");
  const line = /^- Production surfaces: (.+)$/m.exec(charter)?.[1] || "";
  for (const surface of [...line.matchAll(/`([^`]+)`/g)].map((match) => match[1])) {
    const owned = node.conflict_domains.map((id) => domains.get(id)).filter(Boolean);
    if (owned.some((domain) => domain.roots.some((root) => surface === root || surface.startsWith(`${root}/`)))) continue;
    const target = owned.find((domain) => domain.roots.some((root) => intersects(surface, root))) || owned[0];
    if (!target) throw new Error(`${node.id}: no domain for ${surface}`);
    if (!target.roots.includes(surface)) { target.roots.push(surface); additions += 1; }
  }
}
const output = `${header.trimEnd()}\n${blocks.map((block) => {
  const id = scalar(block, "id");
  const roots = [...new Set(domains.get(id).roots)].sort();
  return block.replace(/^path_roots = .*$/m, `path_roots = ${JSON.stringify(roots)}`).trimEnd();
}).join("\n")}\n`;
if (check) {
  if (output !== original) { console.error("STALE conflict ownership catalog"); process.exit(1); }
  console.log(`build-conflict-ownership: PASS (${domains.size} concrete domains)`);
} else {
  fs.writeFileSync(file, output);
  console.log(`build-conflict-ownership: wrote ${domains.size} domains; ${additions} uncovered surfaces assigned`);
}
