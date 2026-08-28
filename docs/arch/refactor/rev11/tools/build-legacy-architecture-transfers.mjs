#!/usr/bin/env node
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, exactRegularFileInventory, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const mapFile = path.join(PACKAGE_ROOT, "catalogs/legacy-arch-transfer-map.toml");
const outputRoot = path.join(PACKAGE_ROOT, "sources/legacy-architecture-transfers");
const indexFile = path.join(PACKAGE_ROOT, "sources/legacy-architecture-transfers.md");
const repository = childProcess
  .execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: PACKAGE_ROOT, encoding: "utf8" })
  .trim();
const model = readToml(mapFile);
const authority = loadAuthority(PACKAGE_ROOT);
const nodeIds = new Set(authority.nodes.map((node) => node.id));
const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");
const normalized = (value) => value.split(path.sep).join("/");

if (model.schema !== 1 || model.catalog !== "legacy-arch-transfer-map")
  throw new Error("legacy transfer map must be schema 1");
const actualTree = childProcess
  .execFileSync("git", ["rev-parse", `${model.source_commit}^{tree}`], {
    cwd: repository,
    encoding: "utf8",
  })
  .trim();
if (actualTree !== model.source_tree)
  throw new Error("legacy transfer map source tree does not match source commit");

const sourceRows = childProcess
  .execFileSync("git", ["ls-tree", "-r", model.source_commit, "--", "docs/arch"], {
    cwd: repository,
    encoding: "utf8",
  })
  .trim()
  .split("\n")
  .filter(Boolean)
  .map((line) => {
    const match = /^\d+ blob ([0-9a-f]{40})\t(.+)$/u.exec(line);
    if (!match) throw new Error(`unexpected legacy source-tree row ${line}`);
    return { blob_sha: match[1], path: match[2] };
  })
  .filter((row) => !row.path.startsWith("docs/arch/refactor/rev11/"));
const transferSourceRows = sourceRows.filter(
  (row) => !row.path.startsWith(model.historical_evidence_prefix),
);
const transfers = model.transfer || [];
const inventory = (rows) => rows.map((row) => `${row.path}:${row.blob_sha}`).sort();
if (new Set(transfers.map((row) => row.path)).size !== transfers.length)
  throw new Error("legacy transfer map contains duplicate paths");
if (JSON.stringify(inventory(transfers)) !== JSON.stringify(inventory(transferSourceRows))) {
  throw new Error("legacy transfer map must cover the exact non-ledger source path/blob inventory");
}

const batchInput = Buffer.from(
  [...transfers]
    .sort((left, right) => left.path.localeCompare(right.path))
    .map((row) => `${model.source_commit}:${row.path}\n`)
    .join(""),
);
const batchOutput = childProcess.execFileSync("git", ["cat-file", "--batch"], {
  cwd: repository,
  input: batchInput,
  encoding: "buffer",
  maxBuffer: 32 * 1024 * 1024,
});
const sourceObjects = new Map();
let batchOffset = 0;
for (const row of [...transfers].sort((left, right) => left.path.localeCompare(right.path))) {
  const headerEnd = batchOutput.indexOf(10, batchOffset);
  if (headerEnd < 0) throw new Error(`${row.path}: truncated git cat-file header`);
  const [objectId, type, sizeText] = batchOutput
    .subarray(batchOffset, headerEnd)
    .toString("utf8")
    .split(" ");
  const size = Number(sizeText);
  if (objectId !== row.blob_sha || type !== "blob" || !Number.isSafeInteger(size) || size < 0)
    throw new Error(`${row.path}: unexpected git cat-file object`);
  const start = headerEnd + 1;
  const end = start + size;
  if (batchOutput[end] !== 10) throw new Error(`${row.path}: truncated git cat-file body`);
  sourceObjects.set(row.path, batchOutput.subarray(start, end));
  batchOffset = end + 1;
}
if (batchOffset !== batchOutput.length)
  throw new Error("git cat-file returned unexpected trailing bytes");

const outputs = new Map();
const sections = [];
for (const row of [...transfers].sort((left, right) => left.path.localeCompare(right.path))) {
  if (
    !Array.isArray(row.targets) ||
    !row.targets.length ||
    new Set(row.targets).size !== row.targets.length
  )
    throw new Error(`${row.path}: transfer targets must be non-empty and unique`);
  for (const id of row.targets)
    if (!nodeIds.has(id)) throw new Error(`${row.path}: unknown transfer target ${id}`);
  const relative = row.path.slice("docs/arch/".length);
  if (!relative || relative.startsWith("../") || path.isAbsolute(relative))
    throw new Error(`${row.path}: unsafe transfer path`);
  const source = sourceObjects.get(row.path);
  const outputRelative = `sources/legacy-architecture-transfers/${relative}`;
  outputs.set(path.join(PACKAGE_ROOT, outputRelative), source);
  const atom = `LEGACY-TRANSFER-${row.blob_sha.slice(0, 12).toUpperCase()}`;
  sections.push(
    [
      `### ${atom}`,
      "",
      `- Original path: \`${row.path}\`; Git blob: \`${row.blob_sha}\`; exact source SHA-256: \`${sha256(source)}\`.`,
      `- Exact retained source: \`${outputRelative}\`.`,
      `- Applicable authority: ${row.targets.map((id) => `\`${id}\``).join(", ")}.`,
      `- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.`,
    ].join("\n"),
  );
}

const index = [
  "# Exact legacy architecture requirement transfers",
  "",
  "This generated source binds every non-ledger file removed from the legacy `docs/arch` route to an exact byte-for-byte retained source and named current authority. The reviewed map is `catalogs/legacy-arch-transfer-map.toml`; no filename heuristic or uncatalogued fallback exists.",
  "",
  ...sections.flatMap((section, index) => (index ? ["", section] : [section])),
  "",
].join("\n");

const stale = [];
const expectedRelative = new Set(
  [...outputs.keys()].map((file) => normalized(path.relative(outputRoot, file))),
);
const retainedInventory = fs.existsSync(outputRoot)
  ? exactRegularFileInventory(outputRoot, "exact retained legacy source inventory")
  : { files: [], errors: [] };
stale.push(...retainedInventory.errors);
for (const actual of retainedInventory.files)
  if (!expectedRelative.has(actual))
    stale.push(normalized(path.relative(PACKAGE_ROOT, path.join(outputRoot, actual))));
for (const [file, bytes] of outputs)
  if (!fs.existsSync(file) || !fs.readFileSync(file).equals(bytes))
    stale.push(normalized(path.relative(PACKAGE_ROOT, file)));
if (!fs.existsSync(indexFile) || fs.readFileSync(indexFile, "utf8") !== index)
  stale.push(normalized(path.relative(PACKAGE_ROOT, indexFile)));

if (check) {
  if (stale.length) {
    console.error(`STALE legacy architecture transfers: ${[...new Set(stale)].sort().join(", ")}`);
    process.exit(1);
  }
  console.log(
    `build-legacy-architecture-transfers: PASS (${outputs.size} exact sources; ${sourceRows.length - outputs.size} immutable ledger records)`,
  );
} else {
  if (retainedInventory.errors.length) throw new Error(retainedInventory.errors.join("; "));
  fs.mkdirSync(outputRoot, { recursive: true });
  for (const actual of retainedInventory.files)
    if (!expectedRelative.has(actual)) fs.unlinkSync(path.join(outputRoot, actual));
  for (const [file, bytes] of outputs) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, bytes);
  }
  fs.writeFileSync(indexFile, index);
  console.log(
    `build-legacy-architecture-transfers: wrote ${outputs.size} exact sources; retained ${sourceRows.length - outputs.size} ledger records in Git history only`,
  );
}
