#!/usr/bin/env node
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority } from "./lib.mjs";

const check = process.argv.includes("--check");
const recoveryCommit = "903f06b80e4416a19f4eeaf2f4ab7f02b09ec096";
const repository = childProcess
  .execFileSync("git", ["rev-parse", "--show-toplevel"], {
    cwd: PACKAGE_ROOT,
    encoding: "utf8",
    timeout: 30_000,
  })
  .trim();
const prefix = "docs/arch/refactor/rev11/unified/authority/dag/";
const files = childProcess
  .execFileSync("git", ["ls-tree", "-r", "--name-only", recoveryCommit, prefix], {
    cwd: repository,
    encoding: "utf8",
    timeout: 30_000,
  })
  .trim()
  .split("\n")
  .filter((file) => file.endsWith(".toml"));
const original = [];
for (const file of files) {
  const text = childProcess.execFileSync("git", ["show", `${recoveryCommit}:${file}`], {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    timeout: 30_000,
  });
  for (const body of text.split("\n[[node]]\n").slice(1)) {
    const id = /^id = "([^"]+)"$/m.exec(body)?.[1];
    const name = /^name = "([^"]+)"$/m.exec(body)?.[1];
    const predecessors = /^predecessors = (\[.*\])$/m.exec(body)?.[1];
    const charter = /^charter = "([^"]+)"$/m.exec(body)?.[1];
    if (id && name && predecessors && charter) {
      const repositoryRelative = `docs/arch/refactor/rev11/unified/${charter}`;
      const charterBytes = childProcess.execFileSync(
        "git",
        ["show", `${recoveryCommit}:${repositoryRelative}`],
        { cwd: repository, maxBuffer: 16 * 1024 * 1024, timeout: 30_000 },
      );
      original.push({
        id,
        name,
        predecessors: JSON.parse(predecessors),
        charter,
        charterSha256: crypto.createHash("sha256").update(charterBytes).digest("hex"),
      });
    }
  }
}
const authority = loadAuthority(PACKAGE_ROOT);
const current = new Set(authority.nodes.map((node) => node.id));
const originalById = new Map(original.map((node) => [node.id, node]));
const proposed = authority.nodes.filter(
  (node) =>
    node.source_refs.includes("source:successor-dag-amendment.md:L1") ||
    node.source_refs.includes("source:github-control-plane-program.md:L1"),
);
const proposedIds = new Set(proposed.map((node) => node.id));
const invented = authority.nodes.filter(
  (node) => !originalById.has(node.id) && !proposedIds.has(node.id),
);
if (invented.length)
  throw new Error(
    `current authority contains IDs absent from exact recovery input: ${invented.map((node) => node.id).join(", ")}`,
  );
const removed = original
  .filter((node) => !current.has(node.id))
  .sort((a, b) => a.id.localeCompare(b.id));
const currentIds = [...current].sort((a, b) => b.length - a.length || a.localeCompare(b));
const targetFor = (id) =>
  id === "BR0P"
    ? "BR0"
    : /^HFP\d+$/.test(id)
      ? "HWC1"
      : id === "VCB0"
        ? ""
        : currentIds.find((candidate) => id.startsWith(candidate) && /\d$/.test(candidate)) || "";
const rows = removed.map((node) => ({ ...node, target: targetFor(node.id) }));
const mapFile = path.join(PACKAGE_ROOT, "provenance/collapsed-node-map.toml");
const retained = authority.nodes
  .filter((node) => originalById.has(node.id))
  .map((node) => ({ current: node, original: originalById.get(node.id) }))
  .sort((left, right) => left.current.id.localeCompare(right.current.id));
const rendered = [
  `schema = 3`,
  `recovery_input_commit = ${JSON.stringify(recoveryCommit)}`,
  `recovery_input_node_count = ${original.length}`,
  `current_node_count = ${authority.nodes.length}`,
  `successor_sources = ["sources/successor-dag-amendment.md", "sources/github-control-plane-program.md"]`,
  ``,
  ...retained.flatMap(({ current: node, original: prior }) => [
    "[[retained]]",
    `id = ${JSON.stringify(node.id)}`,
    `original_name = ${JSON.stringify(prior.name)}`,
    `current_name = ${JSON.stringify(node.name)}`,
    `original_predecessors = ${JSON.stringify(prior.predecessors)}`,
    `current_predecessors = ${JSON.stringify(node.predecessors)}`,
    `original_charter = ${JSON.stringify(prior.charter)}`,
    `current_charter = ${JSON.stringify(node.charter)}`,
    `original_charter_sha256 = ${JSON.stringify(prior.charterSha256)}`,
    `disposition = "retained_reauthored_under_current_authority"`,
    "",
  ]),
  ...proposed
    .sort((left, right) => left.id.localeCompare(right.id))
    .flatMap((node) => [
      "[[addition]]",
      `id = ${JSON.stringify(node.id)}`,
      `name = ${JSON.stringify(node.name)}`,
      `module = ${JSON.stringify(node._module)}`,
      `charter = ${JSON.stringify(node.charter)}`,
      `source = ${JSON.stringify(node.source_refs.includes("source:github-control-plane-program.md:L1") ? "sources/github-control-plane-program.md" : "sources/successor-dag-amendment.md")}`,
      `disposition = ${JSON.stringify(node.source_refs.includes("source:github-control-plane-program.md:L1") ? "github_control_plane_addition_pending_authority_amendment" : "source_pack_successor_addition_pending_authority_amendment")}`,
      "",
    ]),
  ...rows.flatMap((row) => [
    "[[disposition]]",
    `id = ${JSON.stringify(row.id)}`,
    `name = ${JSON.stringify(row.name)}`,
    `disposition = ${JSON.stringify(row.target ? "collapsed_into_atomic_source_node" : "deleted_unratified")}`,
    `target = ${JSON.stringify(row.target)}`,
    "",
  ]),
].join("\n");

const byTarget = new Map();
for (const row of rows.filter((candidate) => candidate.target))
  byTarget.set(row.target, [...(byTarget.get(row.target) || []), row.id]);
const charterOutputs = new Map();
for (const node of authority.nodes) {
  const file = path.join(PACKAGE_ROOT, node.charter);
  let text = fs.readFileSync(file, "utf8");
  text = text.replace(
    /\n## Collapsed non-authoritative subblock disposition\n[\s\S]*?(?=\n## Transferred source requirement atoms)/m,
    "",
  );
  const ids = (byTarget.get(node.id) || []).sort();
  if (ids.length) {
    const section = `\n## Collapsed non-authoritative subblock disposition\n\nThe recovery candidate mechanically split this source-owned atomic node into the following labels: ${ids.map((id) => `\`${id}\``).join(", ")}. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **${node.id}**; ${node.id} alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.\n`;
    text = text.replace(
      /\n## Transferred source requirement atoms/m,
      `${section}\n## Transferred source requirement atoms`,
    );
  }
  charterOutputs.set(file, text);
}

const stale =
  !fs.existsSync(mapFile) ||
  fs.readFileSync(mapFile, "utf8") !== rendered ||
  [...charterOutputs].some(([file, output]) => fs.readFileSync(file, "utf8") !== output);
if (check) {
  if (stale) {
    console.error("STALE collapsed-node disposition map/charter notices");
    process.exit(1);
  }
  console.log(
    `build-collapse-map: PASS (${original.length} recovery nodes; ${rows.length} dispositions; ${authority.nodes.length} current)`,
  );
} else {
  fs.writeFileSync(mapFile, rendered);
  for (const [file, output] of charterOutputs) fs.writeFileSync(file, output);
  console.log(
    `build-collapse-map: wrote ${rows.length} exact dispositions across ${byTarget.size} atomic nodes`,
  );
}
