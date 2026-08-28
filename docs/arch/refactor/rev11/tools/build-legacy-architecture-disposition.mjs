#!/usr/bin/env node
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const repository = childProcess
  .execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: PACKAGE_ROOT, encoding: "utf8" })
  .trim();
const authority = loadAuthority(PACKAGE_ROOT);
const byId = new Map(authority.nodes.map((node) => [node.id, node]));
const transferMap = readToml(path.join(PACKAGE_ROOT, "catalogs/legacy-arch-transfer-map.toml"));
const transfers = new Map((transferMap.transfer || []).map((row) => [row.path, row]));
const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");
const outputFile = path.join(PACKAGE_ROOT, "catalogs/legacy-arch-disposition.toml");

const actualTree = childProcess
  .execFileSync("git", ["rev-parse", `${transferMap.source_commit}^{tree}`], {
    cwd: repository,
    encoding: "utf8",
  })
  .trim();
if (actualTree !== transferMap.source_tree)
  throw new Error("legacy transfer map source tree does not match source commit");

const rows = childProcess
  .execFileSync("git", ["ls-tree", "-r", transferMap.source_commit, "--", "docs/arch"], {
    cwd: repository,
    encoding: "utf8",
  })
  .trim()
  .split("\n")
  .filter(Boolean)
  .map((line) => {
    const match = /^\d+ blob ([0-9a-f]{40})\t(.+)$/u.exec(line);
    if (!match) throw new Error(`unexpected ls-tree row ${line}`);
    return { blob_sha: match[1], path: match[2] };
  })
  .filter((row) => !row.path.startsWith("docs/arch/refactor/rev11/"));
const nonEvidence = rows.filter(
  (row) => !row.path.startsWith(transferMap.historical_evidence_prefix),
);
const expectedMap = nonEvidence.map((row) => `${row.path}:${row.blob_sha}`).sort();
const actualMap = [...transfers.values()].map((row) => `${row.path}:${row.blob_sha}`).sort();
if (
  new Set(transfers.keys()).size !== transfers.size ||
  JSON.stringify(actualMap) !== JSON.stringify(expectedMap)
) {
  throw new Error(
    "reviewed legacy transfer map differs from the exact non-ledger source inventory",
  );
}

function targetDigests(targets) {
  return targets.map((id) => {
    const node = byId.get(id);
    if (!node) throw new Error(`legacy disposition target does not exist: ${id}`);
    return `${id}:${sha256(fs.readFileSync(path.join(PACKAGE_ROOT, node.charter)))}`;
  });
}

function replacementDigest(relative) {
  const absolute = path.join(repository, relative);
  if (!fs.existsSync(absolute) || !fs.statSync(absolute).isFile())
    throw new Error(`legacy replacement source is missing: ${relative}`);
  return `${relative}:${sha256(fs.readFileSync(absolute))}`;
}

function renderEntry(row) {
  const transfer = transfers.get(row.path);
  const historical = row.path.startsWith(transferMap.historical_evidence_prefix);
  if (!transfer && !historical)
    throw new Error(`${row.path}: non-ledger source lacks a reviewed transfer`);
  if (transfer && transfer.blob_sha !== row.blob_sha)
    throw new Error(`${row.path}: reviewed transfer blob is stale`);
  const exactSource = transfer
    ? `docs/arch/refactor/rev11/sources/legacy-architecture-transfers/${row.path.slice("docs/arch/".length)}`
    : "";
  const targets = transfer?.targets || [];
  const replacements = transfer ? [exactSource, ...(transfer.relocations || [])] : [];
  const atom = transfer ? `SRC-LEGACY-TRANSFER-${row.blob_sha.slice(0, 12).toUpperCase()}` : "";
  return [
    "[[entry]]",
    `path = ${JSON.stringify(row.path)}`,
    `blob_sha = ${JSON.stringify(row.blob_sha)}`,
    `disposition = ${JSON.stringify(historical ? "historical_evidence" : "transferred_exact_source")}`,
    `evidence_class = ${JSON.stringify(historical ? "immutable_audit_ledger" : "")}`,
    `source_sha256 = ${JSON.stringify(transfer ? sha256(fs.readFileSync(path.join(repository, exactSource))) : "")}`,
    `targets = ${JSON.stringify(targets)}`,
    `target_charter_digests = ${JSON.stringify(targetDigests(targets))}`,
    `replacement_sources = ${JSON.stringify(replacements)}`,
    `replacement_sha256 = ${JSON.stringify(replacements.map(replacementDigest))}`,
    `requirement_atoms = ${JSON.stringify(atom ? [atom] : [])}`,
    "delete_in_same_amendment = true",
    `rationale = ${JSON.stringify(
      historical
        ? "Immutable audit-ledger evidence is deleted from the current documentation tree; its exact Git blob remains bound in this catalog and retained by repository history."
        : "Every durable clause is retained byte-for-byte under Rev11 sources and transferred by an exact source atom to the named current authority; the legacy route is deleted in the same amendment.",
    )}`,
  ].join("\n");
}

const header = [
  "schema = 2",
  'catalog = "legacy-arch-disposition"',
  `source_commit = ${JSON.stringify(transferMap.source_commit)}`,
  `source_tree = ${JSON.stringify(transferMap.source_tree)}`,
  'legacy_root = "docs/arch"',
  'authority_root = "docs/arch/refactor/rev11"',
  'exclude = ["docs/arch/refactor/rev11/**"]',
  `historical_evidence_prefix = ${JSON.stringify(transferMap.historical_evidence_prefix)}`,
  'allowed_dispositions = ["transferred_exact_source", "historical_evidence"]',
  "",
].join("\n");
const output = `${header}${rows.map(renderEntry).join("\n\n")}\n`;

if (check) {
  if (!fs.existsSync(outputFile) || fs.readFileSync(outputFile, "utf8") !== output) {
    console.error(
      `STALE legacy architecture disposition: ${path.relative(PACKAGE_ROOT, outputFile)}`,
    );
    process.exit(1);
  }
  console.log(
    `build-legacy-architecture-disposition: PASS (${rows.length} exact paths; ${transfers.size} reviewed transfers)`,
  );
} else {
  fs.writeFileSync(outputFile, output);
  console.log(
    `build-legacy-architecture-disposition: wrote ${rows.length} exact paths; ${transfers.size} reviewed transfers`,
  );
}
