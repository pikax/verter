#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const authority = loadAuthority(PACKAGE_ROOT);
const coverageFile = path.join(PACKAGE_ROOT, "provenance/source-coverage.toml");
const original = fs.readFileSync(coverageFile, "utf8");
const header = original.split("\n[[requirement]]\n", 1)[0];
const blocks = original.split("\n[[requirement]]\n").slice(1).map((body) => `[[requirement]]\n${body}`);

function scalar(block, key) {
  const match = block.match(new RegExp(`^${key} = (.+)$`, "m"));
  if (!match) throw new Error(`source atom missing ${key}`);
  return JSON.parse(match[1]);
}

const refs = new Map();
for (const node of authority.nodes.filter((candidate) => candidate.review_profile !== "history")) {
  for (const ref of node.source_refs.filter((value) => value.startsWith("source:"))) {
    const match = /^source:([^:]+):L(\d+)$/.exec(ref);
    if (!match) continue;
    refs.set(match[1], [...(refs.get(match[1]) || []), { id: node.id, line: Number(match[2]) }]);
  }
}

const renderedBlocks = [];
const targetRows = new Map();
const packetRows = new Map(authority.nodes.map((node) => [node.id, []]));
const nodesById = new Map(authority.nodes.map((node) => [node.id, node]));
for (let block of blocks) {
  let target = scalar(block, "target");
  if (target.startsWith("node:") && !nodesById.has(target.slice("node:".length))) {
    target = "contract:contracts/compiler-architecture.md";
    block = block.replace(/^target = .*$/m, `target = ${JSON.stringify(target)}`);
  }
  const source = scalar(block, "source");
  const line = scalar(block, "from_line");
  let applicable;
  if (target.startsWith("node:")) applicable = [target.slice("node:".length)];
  else {
    const candidates = refs.get(source) || [];
    if (!candidates.length) applicable = ["ORC0"];
    else {
      const distance = Math.min(...candidates.map((candidate) => Math.abs(candidate.line - line)));
      applicable = [...new Set(candidates.filter((candidate) => Math.abs(candidate.line - line) === distance).map((candidate) => candidate.id))].sort().slice(0, 3);
    }
  }
  const serialized = JSON.stringify(applicable);
  if (/^applicable_nodes = /m.test(block)) block = block.replace(/^applicable_nodes = .*$/m, `applicable_nodes = ${serialized}`);
  else block = block.replace(/^disposition = /m, `applicable_nodes = ${serialized}\ndisposition = `);
  renderedBlocks.push(block.trimEnd());
  const relative = target.startsWith("contract:") ? target.slice("contract:".length) : nodesById.get(target.slice("node:".length))?.charter;
  if (relative) {
    const row = {
      id: scalar(block, "id"), kind: scalar(block, "kind"), source, from: line, to: scalar(block, "to_line"),
      target, digest: scalar(block, "text_sha256"), text: scalar(block, "text"), applicable,
    };
    targetRows.set(relative, [...(targetRows.get(relative) || []), row]);
    for (const id of applicable) packetRows.set(id, [...(packetRows.get(id) || []), row]);
  }
}
const coverage = `${header.trimEnd()}\n\n${renderedBlocks.join("\n\n")}\n`;

for (const node of authority.nodes) if (!targetRows.has(node.charter)) targetRows.set(node.charter, []);

const targetOutputs = new Map();
const nodesByCharter = new Map(authority.nodes.map((node) => [node.charter, node]));
const liveLock = readToml(path.join(PACKAGE_ROOT, "provenance/live-source-lock.toml"));
const liveRows = new Map((liveLock.source || []).map((row) => [row.ref, row]));
for (const [relative, rows] of targetRows) {
  const file = path.join(PACKAGE_ROOT, relative);
  const current = fs.readFileSync(file, "utf8");
  const pieces = current.split(/^## Transferred source requirement atoms$/m);
  const operative = pieces[0].replace(/\n## Live authority inputs\n[\s\S]*?(?=\n## |\s*$)/g, "").trimEnd();
  const node = nodesByCharter.get(relative);
  const boundLive = (node?.source_refs || []).filter((ref) => ref.startsWith("live:")).map((ref) => {
    const row = liveRows.get(ref); if (!row) throw new Error(`${node.id}: missing live lock ${ref}`);
    return `- \`${ref}\` — ${row.bytes} bytes, SHA-256 \`${row.sha256}\``;
  });
  const suffix = boundLive.length ? `## Live authority inputs\n\n${boundLive.sort().join("\n")}` : "";
  const clauses = rows.length ? rows.map((row) => `### ${row.id}\n\n- Kind: \`${row.kind}\`\n- Source: \`${row.source}:${row.from}-${row.to}\`\n- Applicability: ${row.applicable.map((id) => `\`${id}\``).join(", ")}\n- Exact text SHA-256: \`${row.digest}\`\n\n~~~~markdown\n${row.text}\n~~~~`).join("\n\n") : "No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.";
  targetOutputs.set(file, `${operative}\n\n## Transferred source requirement atoms\n\nThese clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.\n\n${clauses}${suffix ? `\n\n${suffix}` : ""}\n`);
}

const stale = [];
if (original !== coverage) stale.push(path.relative(PACKAGE_ROOT, coverageFile));
for (const [file, output] of targetOutputs) if (fs.readFileSync(file, "utf8") !== output) stale.push(path.relative(PACKAGE_ROOT, file));
const packetRoot = path.join(PACKAGE_ROOT, "provenance/packet-source-clauses");
const packetOutputs = new Map();
for (const node of authority.nodes) {
  const rows = [...(packetRows.get(node.id) || [])].sort((left, right) => left.id.localeCompare(right.id));
  const clauses = rows.length ? rows.map((row) => `### ${row.id}\n\n- Kind: \`${row.kind}\`; source: \`${row.source}:${row.from}-${row.to}\`; target: \`${row.target}\`; text SHA-256: \`${row.digest}\`.\n\n~~~~markdown\n${row.text}\n~~~~`).join("\n\n") : "- none";
  packetOutputs.set(path.join(packetRoot, `${node.id}.md`), `# Exact operative source-clause attachment — ${node.id}\n\nSchema: 1. Node: \`${node.id}\`. Clause count: ${rows.length}. Generated from \`provenance/source-coverage.toml\`; every clause below is exact, operative, and applicable to this node.\n\n${clauses}\n`);
}
const actualPacketFiles = fs.existsSync(packetRoot) ? fs.readdirSync(packetRoot).filter((name) => name.endsWith(".md")).map((name) => path.join(packetRoot, name)) : [];
for (const file of actualPacketFiles) if (!packetOutputs.has(file)) stale.push(path.relative(PACKAGE_ROOT, file));
for (const [file, output] of packetOutputs) if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== output) stale.push(path.relative(PACKAGE_ROOT, file));
if (check) {
  if (stale.length) { console.error(`STALE source clauses: ${stale.join(", ")}`); process.exit(1); }
  console.log(`build-source-clauses: PASS (${blocks.length} clauses; ${targetOutputs.size} operative targets)`);
} else {
  fs.writeFileSync(coverageFile, coverage);
  for (const [file, output] of targetOutputs) fs.writeFileSync(file, output);
  fs.mkdirSync(packetRoot, { recursive: true });
  for (const file of actualPacketFiles) if (!packetOutputs.has(file)) fs.unlinkSync(file);
  for (const [file, output] of packetOutputs) fs.writeFileSync(file, output);
  console.log(`build-source-clauses: wrote ${blocks.length} clauses across ${targetOutputs.size} targets`);
}
