#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const sourceName = "legacy-arch-reconciliation.md";
const sourceFile = path.join(PACKAGE_ROOT, "sources", sourceName);
const amendmentSourceName = "successor-dag-amendment.md";
const amendmentSourceFile = path.join(PACKAGE_ROOT, "sources", amendmentSourceName);
const existingAmendmentSourceName = "existing-node-amendments.md";
const existingAmendmentSourceFile = path.join(PACKAGE_ROOT, "sources", existingAmendmentSourceName);
const legacyTransferSourceName = "legacy-architecture-transfers.md";
const legacyTransferSourceFile = path.join(PACKAGE_ROOT, "sources", legacyTransferSourceName);
const legacyTransferMap = readToml(
  path.join(PACKAGE_ROOT, "catalogs/legacy-arch-transfer-map.toml"),
);
const githubSourceName = "github-control-plane-program.md";
const githubSourceFile = path.join(PACKAGE_ROOT, "sources", githubSourceName);
const coverageFile = path.join(PACKAGE_ROOT, "provenance/source-coverage.toml");
const sourceBytes = fs.readFileSync(sourceFile);
const sourceSha = crypto.createHash("sha256").update(sourceBytes).digest("hex");
const lines = sourceBytes.toString("utf8").split("\n");
const authority = loadAuthority(PACKAGE_ROOT);
const nodeIds = new Set(authority.nodes.map((node) => node.id));
const familyIds = [...nodeIds].filter((id) => id.startsWith("NCF-")).sort();
const successorIds = [...nodeIds].filter((id) => /^(?:NCK|NCF-|LSO|EPR)/u.test(id)).sort();

const exactTargets = {
  "EXISTING-CACHE-001": ["G1", "G2", "E4", "H1", "NCK2", "LSO2", "LSO4", "EPR4"],
  "EXISTING-FLOW-001": ["D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "NCK1", "NCK3"],
  "EXISTING-TYPEINFO-001": [
    "E1",
    "E2",
    "E3",
    "E4",
    "TCM3",
    "TCM4",
    "TIF0",
    "TIF1",
    "UAO0",
    "PUB0",
    "NCK0",
  ],
  "EXISTING-SCHED-001": ["G3", "G5", "H2", "H3", "EPR2", "EPR5"],
};

function targetsFor(id, text) {
  if (exactTargets[id]) return exactTargets[id];
  const targets = [...text.matchAll(/`([A-Z][A-Z0-9-]*)`/gu)]
    .map((match) => match[1])
    .filter((candidate) => nodeIds.has(candidate));
  if (/NCF-\*/u.test(text)) targets.push(...familyIds);
  if (/all LSO implementations/u.test(text))
    targets.push(...[...nodeIds].filter((candidate) => /^LSO\d+$/u.test(candidate)).sort());
  if (/amendments to `B2`\/`PAR0`/u.test(text)) targets.push("B2", "PAR0");
  return [...new Set(targets)].filter((candidate) => nodeIds.has(candidate));
}

const atoms = [];
for (let index = 0; index < lines.length; index += 1) {
  const heading = /^### ([A-Z][A-Z0-9-]+)(?: — .+)?$/u.exec(lines[index]);
  if (!heading) continue;
  let end = index + 1;
  while (end < lines.length && !/^### |^---$/u.test(lines[end])) end += 1;
  while (end > index + 1 && lines[end - 1].trim() === "") end -= 1;
  const text = lines
    .slice(index, end)
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
  const applicable = targetsFor(heading[1], text);
  if (!applicable.length) throw new Error(`${heading[1]} has no exact applicable node`);
  atoms.push({
    id: `SRC-LEGACY-${heading[1]}`,
    sourceId: heading[1],
    from: index + 1,
    to: end,
    text,
    digest: crypto.createHash("sha256").update(`${text}\n`).digest("hex"),
    applicable,
  });
}

const amendmentBytes = fs.readFileSync(amendmentSourceFile);
const amendmentSourceSha = crypto.createHash("sha256").update(amendmentBytes).digest("hex");
const amendmentText = amendmentBytes.toString("utf8").split("\n")[0].trim();
const amendmentAtom = {
  id: "SRC-SUCCESSOR-DAG-AMENDMENT",
  from: 1,
  to: 1,
  text: amendmentText,
  digest: crypto.createHash("sha256").update(`${amendmentText}\n`).digest("hex"),
  applicable: successorIds,
};

const existingAmendmentBytes = fs.readFileSync(existingAmendmentSourceFile);
const existingAmendmentLines = existingAmendmentBytes.toString("utf8").split("\n");
const existingAmendmentSha = crypto
  .createHash("sha256")
  .update(existingAmendmentBytes)
  .digest("hex");
const existingSectionTargets = [
  ["B2", ["B2"]],
  ["PAR0", ["PAR0"]],
  ["TCM3", ["TCM3"]],
  ["TCM4", ["TCM4"]],
  ["IDX0", ["IDX0"]],
  ["LRA0", ["LRA0"]],
  ["PUB0", ["PUB0"]],
  ["VIM0 / VIM1", ["VIM0", "VIM1"]],
  ["PER0", ["PER0"]],
  ["H2 / H3", ["H2", "H3"]],
  ["COX0", ["COX0"]],
  ["CLI2", ["CLI2"]],
  ["CLI4", ["CLI4"]],
];
const existingAtoms = [];
for (const [heading, targets] of existingSectionTargets) {
  const start = existingAmendmentLines.findIndex((line) => line.startsWith(`## ${heading} `));
  if (start < 0) throw new Error(`missing existing-node amendment section ${heading}`);
  let end = start + 1;
  while (end < existingAmendmentLines.length && !/^## /u.test(existingAmendmentLines[end]))
    end += 1;
  while (end > start + 1 && existingAmendmentLines[end - 1].trim() === "") end -= 1;
  const text = existingAmendmentLines
    .slice(start, end)
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
  for (const target of targets)
    existingAtoms.push({
      id: `SRC-EXISTING-NODE-AMENDMENT-${target}`,
      from: start + 1,
      to: end,
      text,
      digest: crypto.createHash("sha256").update(`${text}\n`).digest("hex"),
      applicable: [target],
    });
}

const legacyTransferBytes = fs.readFileSync(legacyTransferSourceFile);
const legacyTransferLines = legacyTransferBytes.toString("utf8").split("\n");
const legacyTransferSha = crypto.createHash("sha256").update(legacyTransferBytes).digest("hex");
const legacyTransferTargets = new Map(
  (legacyTransferMap.transfer || []).map((row) => [
    `LEGACY-TRANSFER-${row.blob_sha.slice(0, 12).toUpperCase()}`,
    row.targets,
  ]),
);
const legacyTransferAtoms = [];
for (let index = 0; index < legacyTransferLines.length; index += 1) {
  const heading = /^### (LEGACY-TRANSFER-[0-9A-F]{12})$/u.exec(legacyTransferLines[index]);
  if (!heading) continue;
  let end = index + 1;
  while (end < legacyTransferLines.length && !/^### /u.test(legacyTransferLines[end])) end += 1;
  while (end > index + 1 && legacyTransferLines[end - 1].trim() === "") end -= 1;
  const text = legacyTransferLines
    .slice(index, end)
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
  const applicable = legacyTransferTargets.get(heading[1]);
  if (!applicable?.length) throw new Error(`${heading[1]} has no reviewed legacy transfer target`);
  legacyTransferAtoms.push({
    id: `SRC-${heading[1]}`,
    from: index + 1,
    to: end,
    text,
    digest: crypto.createHash("sha256").update(`${text}\n`).digest("hex"),
    applicable,
  });
}
if (legacyTransferAtoms.length !== legacyTransferTargets.size)
  throw new Error("legacy transfer source/index inventory differs from reviewed map");

const githubBytes = fs.readFileSync(githubSourceFile);
const githubLines = githubBytes.toString("utf8").split("\n");
const githubSha = crypto.createHash("sha256").update(githubBytes).digest("hex");
const githubNodeIds = [
  "GH0",
  "GH1",
  "GH2",
  "GH3",
  "GH4",
  "GH5",
  "GH6",
  "FB0",
  "FB1",
  "FB2",
  "REL0",
  "REL1",
  "REL2",
];
for (const id of githubNodeIds)
  if (!nodeIds.has(id))
    throw new Error(`GitHub control-plane source target is not registered: ${id}`);
const githubAtoms = [];
function addGithubAtom(id, start, end, applicable, kind = "requirement") {
  while (end > start + 1 && githubLines[end - 1].trim() === "") end -= 1;
  const text = githubLines
    .slice(start, end)
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
  githubAtoms.push({
    id,
    from: start + 1,
    to: end,
    text,
    digest: crypto.createHash("sha256").update(`${text}\n`).digest("hex"),
    applicable,
    kind,
  });
}
const githubBlockStarts = githubLines
  .map((line, index) => ({ line, index }))
  .filter(({ line }) => /^(?:GH[0-6]|FB[0-2]|REL[0-2]) — /u.test(line) || /^## GH6 — /u.test(line));
for (let index = 0; index < githubBlockStarts.length; index += 1) {
  const { line, index: start } = githubBlockStarts[index];
  const id = /(?:^|## )(GH[0-6]|FB[0-2]|REL[0-2]) /u.exec(line)?.[1];
  const end =
    githubBlockStarts[index + 1]?.index ??
    githubLines.findIndex((candidate) => candidate === "# Synchronization ownership matrix");
  addGithubAtom(`SRC-GITHUB-${id}-BLOCK`, start, end, [id]);
}
function githubHeadingRange(id, startHeading, endHeading, applicable, kind = "requirement") {
  const start = githubLines.findIndex((line) => line === startHeading);
  const end = endHeading
    ? githubLines.findIndex((line) => line === endHeading)
    : githubLines.length;
  if (start < 0 || end <= start) throw new Error(`missing GitHub source range ${startHeading}`);
  addGithubAtom(id, start, end, applicable, kind);
}
githubHeadingRange(
  "SRC-GITHUB-PROGRAM-GOAL",
  "Implement a new post-ORC0 GitHub control-plane program for Rev11.",
  "GH0 — GitHub control-plane contract and authority matrix",
  githubNodeIds,
  "context",
);
githubHeadingRange(
  "SRC-GITHUB-OWNERSHIP-MATRIX",
  "# Synchronization ownership matrix",
  "# GitHub write discipline",
  ["GH0", "GH2", "FB0", "REL0"],
);
githubHeadingRange("SRC-GITHUB-WRITE-DISCIPLINE", "# GitHub write discipline", "# PR policy", [
  "GH1",
  "GH2",
]);
githubHeadingRange("SRC-GITHUB-PR-POLICY", "# PR policy", "# Required tests", [
  "GH3",
  "GH4",
  "GH5",
]);
githubHeadingRange(
  "SRC-GITHUB-REQUIRED-TESTS",
  "# Required tests",
  "# Existing surfaces to inspect before implementation",
  ["GH6"],
  "acceptance",
);
githubHeadingRange(
  "SRC-GITHUB-PRESCOPE",
  "# Existing surfaces to inspect before implementation",
  "# End state",
  githubNodeIds,
);
githubHeadingRange(
  "SRC-GITHUB-END-STATE",
  "# End state",
  "# Binding finding-retention invariant",
  ["GH6"],
  "acceptance",
);
githubHeadingRange(
  "SRC-GITHUB-FINDING-RETENTION",
  "# Binding finding-retention invariant",
  null,
  githubNodeIds,
);

function render(row) {
  return [
    "[[requirement]]",
    `id = ${JSON.stringify(row.id)}`,
    'kind = "requirement"',
    `source = ${JSON.stringify(sourceName)}`,
    `source_sha256 = ${JSON.stringify(sourceSha)}`,
    `from_line = ${row.from}`,
    `to_line = ${row.to}`,
    `text_sha256 = ${JSON.stringify(row.digest)}`,
    `text = ${JSON.stringify(row.text)}`,
    `applicable_nodes = ${JSON.stringify(row.applicable)}`,
    'disposition = "transferred"',
    `target = ${JSON.stringify(`node:${row.applicable[0]}`)}`,
  ].join("\n");
}

function renderAmendment(row) {
  return [
    "[[requirement]]",
    `id = ${JSON.stringify(row.id)}`,
    'kind = "context"',
    `source = ${JSON.stringify(amendmentSourceName)}`,
    `source_sha256 = ${JSON.stringify(amendmentSourceSha)}`,
    `from_line = ${row.from}`,
    `to_line = ${row.to}`,
    `text_sha256 = ${JSON.stringify(row.digest)}`,
    `text = ${JSON.stringify(row.text)}`,
    `applicable_nodes = ${JSON.stringify(row.applicable)}`,
    'disposition = "transferred"',
    'target = "node:NCK0"',
  ].join("\n");
}

function renderExistingAmendment(row) {
  return [
    "[[requirement]]",
    `id = ${JSON.stringify(row.id)}`,
    'kind = "requirement"',
    `source = ${JSON.stringify(existingAmendmentSourceName)}`,
    `source_sha256 = ${JSON.stringify(existingAmendmentSha)}`,
    `from_line = ${row.from}`,
    `to_line = ${row.to}`,
    `text_sha256 = ${JSON.stringify(row.digest)}`,
    `text = ${JSON.stringify(row.text)}`,
    `applicable_nodes = ${JSON.stringify(row.applicable)}`,
    'disposition = "transferred"',
    `target = ${JSON.stringify(`node:${row.applicable[0]}`)}`,
  ].join("\n");
}

function renderLegacyTransfer(row) {
  return [
    "[[requirement]]",
    `id = ${JSON.stringify(row.id)}`,
    'kind = "requirement"',
    `source = ${JSON.stringify(legacyTransferSourceName)}`,
    `source_sha256 = ${JSON.stringify(legacyTransferSha)}`,
    `from_line = ${row.from}`,
    `to_line = ${row.to}`,
    `text_sha256 = ${JSON.stringify(row.digest)}`,
    `text = ${JSON.stringify(row.text)}`,
    `applicable_nodes = ${JSON.stringify(row.applicable)}`,
    'disposition = "transferred"',
    `target = ${JSON.stringify(`node:${row.applicable[0]}`)}`,
  ].join("\n");
}

function renderGithub(row) {
  return [
    "[[requirement]]",
    `id = ${JSON.stringify(row.id)}`,
    `kind = ${JSON.stringify(row.kind)}`,
    `source = ${JSON.stringify(githubSourceName)}`,
    `source_sha256 = ${JSON.stringify(githubSha)}`,
    `from_line = ${row.from}`,
    `to_line = ${row.to}`,
    `text_sha256 = ${JSON.stringify(row.digest)}`,
    `text = ${JSON.stringify(row.text)}`,
    `applicable_nodes = ${JSON.stringify(row.applicable)}`,
    'disposition = "transferred"',
    `target = ${JSON.stringify(`node:${row.applicable[0]}`)}`,
  ].join("\n");
}

const original = fs.readFileSync(coverageFile, "utf8");
const prefix = original.split("\n[[requirement]]\n", 1)[0].trimEnd();
const retained = original
  .split("\n[[requirement]]\n")
  .slice(1)
  .map((body) => `[[requirement]]\n${body}`)
  .filter(
    (body) =>
      !body.includes(`source = ${JSON.stringify(sourceName)}`) &&
      !body.includes(`source = ${JSON.stringify(amendmentSourceName)}`) &&
      !body.includes(`source = ${JSON.stringify(existingAmendmentSourceName)}`) &&
      !body.includes(`source = ${JSON.stringify(legacyTransferSourceName)}`) &&
      !body.includes(`source = ${JSON.stringify(githubSourceName)}`),
  )
  .map((body) => body.trim());
const generatedCount =
  atoms.length + existingAtoms.length + legacyTransferAtoms.length + githubAtoms.length + 1;
const output = `${prefix}\n\n${[...retained, renderAmendment(amendmentAtom), ...atoms.map(render), ...legacyTransferAtoms.map(renderLegacyTransfer), ...githubAtoms.map(renderGithub), ...existingAtoms.map(renderExistingAmendment)].join("\n\n")}\n`;

if (check) {
  if (output !== original) {
    console.error(`STALE successor source atoms: ${coverageFile}`);
    process.exit(1);
  }
  console.log(`build-successor-source-atoms: PASS (${generatedCount} exact atoms)`);
} else {
  fs.writeFileSync(coverageFile, output);
  console.log(`build-successor-source-atoms: wrote ${generatedCount} exact atoms`);
}
