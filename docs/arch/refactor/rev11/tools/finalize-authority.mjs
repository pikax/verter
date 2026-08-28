#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority } from "./lib.mjs";

function value(block, key) {
  const match = block.match(new RegExp(`^${key} = (.+)$`, "m"));
  return match ? JSON.parse(match[1]) : undefined;
}

function replace(block, key, raw) {
  const expression = new RegExp(`^${key} = .+$`, "m");
  if (!expression.test(block)) throw new Error(`missing ${key} in ${value(block, "id")}`);
  return block.replace(expression, `${key} = ${raw}`);
}

function sourceLedger() {
  const source = fs.readFileSync(path.join(PACKAGE_ROOT, "sources/successor-expansion.md"), "utf8");
  const marker = "The TOML below is the sole canonical graph and node-classification ledger.";
  const start = source.indexOf("```toml", source.indexOf(marker));
  const end = source.indexOf("\n```", start);
  const toml = source.slice(start + "```toml\n".length, end);
  const predecessors = new Map();
  const metadata = new Map();
  let section = "";
  for (const line of toml.split("\n")) {
    const header = line.match(/^\[([^\]]+)\]$/);
    if (header) { section = header[1]; continue; }
    if (!line || line.startsWith("schema =")) continue;
    if (section === "predecessors") {
      const match = line.match(/^([A-Z0-9]+) = (\[.*\])$/);
      if (match) predecessors.set(match[1], JSON.parse(match[2]));
    } else if (section === "node") {
      const match = line.match(/^([A-Z0-9]+) = \{ kind = "([^"]+)", product = "([^"]+)", release_gating = "([^"]+)" \}$/);
      if (match) metadata.set(match[1], { kind: match[2], product: match[3], release_gating: match[4] });
    }
  }
  if (!predecessors.size || predecessors.size !== metadata.size) throw new Error(`incomplete successor ledger ${predecessors.size}/${metadata.size}`);
  return { predecessors, metadata };
}

const authority = loadAuthority(PACKAGE_ROOT);
const removed = new Set(authority.nodes.filter((node) => ["proposal-subblock", "split"].includes(node.class)).map((node) => node.id));
removed.add("BR0P");
removed.add("VCB0");
const byId = new Map(authority.nodes.map((node) => [node.id, node]));
const expand = (id, seen = new Set()) => {
  if (!removed.has(id)) return [id];
  if (seen.has(id)) throw new Error(`removed-node cycle at ${id}`);
  const node = byId.get(id);
  if (!node) throw new Error(`missing removed node ${id}`);
  const next = new Set(seen); next.add(id);
  return node.predecessors.flatMap((predecessor) => expand(predecessor, next));
};
const canonical = sourceLedger();

for (const moduleRelative of authority.metadata.modules) {
  const file = path.join(PACKAGE_ROOT, "authority", moduleRelative);
  const text = fs.readFileSync(file, "utf8");
  const header = text.split("\n[[node]]\n", 1)[0];
  const blocks = text.split("\n[[node]]\n").slice(1).map((body) => `[[node]]\n${body}`);
  const kept = [];
  for (let block of blocks) {
    const id = value(block, "id");
    if (removed.has(id)) continue;
    let predecessors = value(block, "predecessors").flatMap((predecessor) => expand(predecessor));
    predecessors = [...new Set(predecessors.filter((predecessor) => predecessor !== id))];
    if (canonical.predecessors.has(id)) predecessors = canonical.predecessors.get(id);
    block = replace(block, "predecessors", JSON.stringify(predecessors));
    if (canonical.metadata.has(id)) {
      const meta = canonical.metadata.get(id);
      block = replace(block, "kind", JSON.stringify(meta.kind));
      block = replace(block, "product", JSON.stringify(meta.product));
      block = replace(block, "release_gating", JSON.stringify(meta.release_gating));
      block = replace(block, "semantic_role", JSON.stringify(meta.kind === "convergence" ? "convergence" : "delivery"));
      block = replace(block, "class", JSON.stringify("successor"));
      block = replace(block, "external_requirements", JSON.stringify(id === "BR0" ? ["maintainer_rev11_repair_freeze_lift", "maintainer_successor_genesis"] : []));
    }
    kept.push(block.trimEnd());
  }
  if (!kept.length) {
    fs.unlinkSync(file);
    continue;
  }
  fs.writeFileSync(file, `${header.trimEnd()}\n\n${kept.join("\n\n")}\n`);
}

for (const id of removed) {
  const node = byId.get(id);
  if (node) fs.unlinkSync(path.join(PACKAGE_ROOT, node.charter));
}

const rootFile = path.join(PACKAGE_ROOT, "authority/root.toml");
let root = fs.readFileSync(rootFile, "utf8");
root = root.replace(/^private_successor_gate = .*\n/m, "");
root = root.replace(/^successor_promotion_gate = .*$/m, 'successor_promotion_gate = "BR0"');
const survivingModules = authority.metadata.modules.filter((relative) => fs.existsSync(path.join(PACKAGE_ROOT, "authority", relative)));
root = root.replace(/^modules = .*$/m, `modules = ${JSON.stringify(survivingModules)}`);
fs.writeFileSync(rootFile, root);

const finalAuthority = loadAuthority(PACKAGE_ROOT);
const charterFields = ["id", "name", "phase", "train", "product", "kind", "semantic_role", "class", "predecessors", "conditional_predecessors", "owner", "conflict_domains", "resource_class", "review_profile", "gate_profile", "size", "dispatchable", "optional", "release_gating", "source_refs", "external_requirements", "activation_gate", "charter", "max_production_loc", "max_production_files", "max_related_packages", "rescope_loc", "rescope_files", "rescope_unrelated_packages", "initial_state"];
for (const node of finalAuthority.nodes) {
  const file = path.join(PACKAGE_ROOT, node.charter);
  let charter = fs.readFileSync(file, "utf8");
  for (const field of charterFields) {
    const raw = Array.isArray(node[field]) ? node[field].join(",") : (node[field] ?? "").toString();
    const expression = new RegExp(`^${field}=.*$`, "m");
    if (!expression.test(charter)) throw new Error(`${node.id} charter missing metadata ${field}`);
    charter = charter.replace(expression, `${field}=${raw}`);
  }
  fs.writeFileSync(file, charter);
}

console.log(`collapsed ${removed.size} non-executable split nodes; ${authority.nodes.length - removed.size} nodes remain; successor ledger ${canonical.predecessors.size} rows`);
