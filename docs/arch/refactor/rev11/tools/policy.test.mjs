/** @ai-generated - Class-wide graph policy and deterministic projection tests. */
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { loadAuthority, readToml, validateCharters, validateGraphModel, writeGenerated } from "./lib.mjs";

const EFFORT_ROLES = ["implementation", "review", "verification", "confirmation"];
const EFFORTS = ["low", "medium", "high"];

function cloneNodes(nodes) {
  return nodes.map((node) => structuredClone(node));
}

function ancestorSet(nodes, id) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const found = new Set();
  const visit = (current) => {
    for (const predecessor of byId.get(current)?.predecessors || []) {
      if (!found.has(predecessor)) { found.add(predecessor); visit(predecessor); }
    }
  };
  visit(id);
  return found;
}

function fileInventory(root) {
  const result = new Map();
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(absolute);
      else result.set(path.relative(root, absolute).split(path.sep).join("/"), crypto.createHash("sha256").update(fs.readFileSync(absolute)).digest("hex"));
    }
  };
  walk(root);
  return [...result].sort(([left], [right]) => left.localeCompare(right));
}

test("every mechanical recovery split has one explicit non-authoritative disposition", () => {
  const authority = loadAuthority();
  const model = readToml(path.join(authority.packageRoot, "provenance/collapsed-node-map.toml"));
  assert.equal(model.recovery_input_commit, "903f06b80e4416a19f4eeaf2f4ab7f02b09ec096");
  assert.equal(model.recovery_input_node_count, 523);
  assert.equal(model.current_node_count, 197);
  assert.equal(model.disposition.length, 326);
  assert.equal(new Set(model.disposition.map((row) => row.id)).size, 326);
  assert.deepEqual(authority.nodes.filter((node) => ["proposal-subblock", "split"].includes(node.class)), []);
  assert.ok(model.disposition.every((row) => row.disposition === "collapsed_into_atomic_source_node" || row.disposition === "deleted_unratified"));
});

test("source-canonical BR0 gates every product without globally joining independent products", () => {
  const authority = loadAuthority();
  const products = authority.nodes.filter((node) => node.release_gating === "product");
  assert.equal(products.length, 9);
  const br0 = authority.nodes.find((node) => node.id === "BR0");
  assert.deepEqual(br0.predecessors, []);
  assert.deepEqual(br0.external_requirements, ["maintainer_rev11_repair_freeze_lift", "maintainer_successor_genesis"]);
  for (const product of products) {
    assert.ok(ancestorSet(authority.nodes, product.id).has("BR0"), `${product.id} is downstream of BR0`);
    const mutated = cloneNodes(authority.nodes);
    const target = mutated.find((node) => node.id === product.id);
    target.predecessors = target.predecessors.filter((id) => !ancestorSet(authority.nodes, id).has("BR0") && id !== "BR0");
    assert.match(validateGraphModel(mutated, { skipCharters: true }).join("\n"), new RegExp(`${product.id}: product release gate is not downstream of BR0`));
  }
  const independent = products.filter((node) => node.id !== "CLI3" && node.id !== "CLI5");
  for (let left = 0; left < independent.length; left += 1) for (let right = left + 1; right < independent.length; right += 1) {
    assert.equal(ancestorSet(authority.nodes, independent[left].id).has(independent[right].id), false);
    assert.equal(ancestorSet(authority.nodes, independent[right].id).has(independent[left].id), false);
  }
});

test("two independent output roots are byte-for-byte deterministic", () => {
  const authority = loadAuthority();
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rev11 deterministic roots "));
  try {
    const first = path.join(scratch, "first output");
    const second = path.join(scratch, "second output");
    writeGenerated(authority, first);
    writeGenerated(authority, second);
    assert.deepEqual(fileInventory(first), fileInventory(second));
    assert.equal(fileInventory(first).length, 4);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

test("every node and charter explicitly publish per-role effort minima and defaults", () => {
  const authority = loadAuthority();
  for (const node of authority.nodes) {
    const charter = fs.readFileSync(path.join(authority.packageRoot, node.charter), "utf8");
    for (const role of EFFORT_ROLES) {
      const minimum = node[`${role}_effort_min`];
      const configuredDefault = node[`${role}_effort_default`];
      assert.ok(EFFORTS.includes(minimum), `${node.id} ${role} minimum is explicit`);
      assert.ok(EFFORTS.includes(configuredDefault), `${node.id} ${role} default is explicit`);
      assert.ok(EFFORTS.indexOf(configuredDefault) >= EFFORTS.indexOf(minimum), `${node.id} ${role} default does not lower its minimum`);
      assert.match(charter, new RegExp(`^${role}_effort_min=${minimum}$`, "m"));
      assert.match(charter, new RegExp(`^${role}_effort_default=${configuredDefault}$`, "m"));
    }
  }
});

test("charter proof policy preserves outcomes without imposing universal test quotas", () => {
  const authority = loadAuthority();
  for (const node of authority.nodes.filter((candidate) => candidate.review_profile !== "history")) {
    const charter = fs.readFileSync(path.join(authority.packageRoot, node.charter), "utf8");
    assert.match(charter, /Preflight evidence selection:/, `${node.id} delegates proof selection to preflight`);
    assert.match(charter, /Every proposed new test must name a plausible regression or contract boundary not already discriminated/, `${node.id} requires a non-duplicate boundary`);
    assert.doesNotMatch(charter, /sole-owner proof:\*\* add `|positive contract:\*\* add `|incremental equivalence:\*\* add `|bounded work:\*\* capture equivalent-work counters/, `${node.id} has no generated test quota`);
    assert.doesNotMatch(charter, /migration\/deletion counts, RED\/GREEN commands and outputs/, `${node.id} does not require inapplicable RED/GREEN evidence`);
    assert.match(charter, /Performance budget: when preflight identifies touched authority or a hot path/, `${node.id} makes performance evidence applicable by touched authority`);
    assert.doesNotMatch(charter, /after warmup, retained bytes may not increase across 100 identical requests/, `${node.id} does not require a universal soak`);
    assert.match(charter, /Bind the preflight evidence selection and terse rationale/, `${node.id} persists proportionate evidence`);
  }

  // Charter prose is the public dispatch contract. Prove the validator rejects
  // the displaced quota even when every section and acceptance ID remains.
  const node = structuredClone(authority.nodes.find((candidate) => candidate.id === "D1"));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rev11-charter-test-economy-"));
  try {
    fs.mkdirSync(path.join(scratch, "catalogs"), { recursive: true });
    fs.copyFileSync(
      path.join(authority.packageRoot, "catalogs/review-profiles.toml"),
      path.join(scratch, "catalogs/review-profiles.toml"),
    );
    const file = path.join(scratch, node.charter);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const current = fs.readFileSync(path.join(authority.packageRoot, node.charter), "utf8");
    const mutations = [
      current.replace(
        /^## Acceptance IDs and discriminating proof\n[\s\S]*?(?=^## )/m,
        `## Acceptance IDs and discriminating proof\n\n- **D1-AC1 — sole-owner proof:** add \`d1_rejects_displaced_authority\`; planting any deleted route must make the targeted gate fail.\n- **D1-AC2 — positive contract:** add \`d1_publishes_exact_flowslice\`; assert exact identities, provenance, completeness, and deterministic ordering.\n- **D1-AC3 — incremental equivalence:** add \`d1_incremental_equals_fresh\`; cancellation/stale/partial outcomes must be refused from warm publication.\n- **D1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces.\n\n`,
      ),
      current.replace(
        /- Performance budget:.*$/m,
        "- Performance budget: equivalent-work counters may increase by 0; after warmup, retained bytes may not increase across 100 identical requests.",
      ),
    ];
    for (const mutation of mutations) {
      fs.writeFileSync(file, mutation);
      assert.match(validateCharters([node], scratch).join("\n"), /universal test quota|preflight evidence selection|proportionate applicability/i);
    }
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

test("pre-trusted-local r3-r6 review bytes are digest-bound audit-only and never acceptance eligible", () => {
  const authority = loadAuthority(); const manifest = JSON.parse(fs.readFileSync(path.join(authority.packageRoot, "authority/state/historical-review-audit.json"), "utf8"));
  assert.equal(manifest.acceptance_eligible, false); assert.equal(manifest.disposition, "AUDIT_ONLY_PRE_TRUSTED_LOCAL"); assert.ok(manifest.files.length >= 22);
  for (const row of manifest.files) assert.equal(crypto.createHash("sha256").update(fs.readFileSync(path.join(authority.packageRoot, row.path))).digest("hex"), row.sha256, row.path);
  const history = JSON.parse(fs.readFileSync(path.join(authority.packageRoot, "authority/state/preactivation-orc0-history.json"), "utf8"));
  assert.equal(history.acceptance_eligible, false); assert.equal(history.disposition, "REJECTED_AUDIT_ONLY"); assert.equal(history.minimum_successor_round_ordinal, 2);
});

test("review profiles and public orchestration policy encode risk scaling and stop boundaries", () => {
  const authority = loadAuthority();
  const catalog = readToml(path.join(authority.packageRoot, "catalogs/review-profiles.toml"));
  const byId = new Map(catalog.profile.map((profile) => [profile.id, profile]));
  assert.deepEqual([byId.get("simple-1").risk_band, byId.get("simple-1").reviewers, byId.get("simple-1").lenses], ["low", 1, ["adversarial"]]);
  assert.deepEqual([byId.get("semantic-3").risk_band, byId.get("semantic-3").reviewers, byId.get("semantic-3").lenses], ["medium", 2, ["adversarial", "conformance"]]);
  for (const id of ["architecture-3", "public-3", "concurrency-3"]) {
    const profile = byId.get(id);
    assert.equal(profile.risk_band, "high"); assert.equal(profile.reviewers, 3);
    assert.equal(profile.lenses[0], "adversarial"); assert.equal(profile.lenses[1], "conformance");
  }
  const skill = fs.readFileSync(path.resolve(authority.packageRoot, "../../../../.claude/skills/multi-agent-orchestration/SKILL.md"), "utf8");
  assert.match(skill, /do not automatically select or launch another train/i);
  assert.match(skill, /frozen.*worktree.*unchanged/i);
  assert.match(skill, /train manager.*parent/i);
  assert.match(skill, /Pre-admission.*independently landable outcomes/is);
  assert.match(skill, /Generated or mechanical file count is not the criterion/i);
  assert.match(skill, /must not turn them into one frozen review unit/i);
  assert.doesNotMatch(skill, /five-way scope-admission policy/i);
  assert.doesNotMatch(skill, /mutation-recipe/i);
});

test("pre-Rev11 orchestration skills are byte-exact audit backups outside active discovery", () => {
  const authority = loadAuthority();
  const repository = childProcess.execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: authority.packageRoot, encoding: "utf8" }).trim();
  const activeRoot = path.join(repository, ".claude/skills");
  const backupRoot = path.join(repository, ".claude/skills-backup/pre-rev11-orchestration");
  assert.equal(fs.existsSync(path.join(activeRoot, "mom-cto-orchestration")), false);
  assert.equal(fs.existsSync(path.join(activeRoot, "agent-prompts")), false);
  assert.equal(fs.statSync(path.join(activeRoot, "multi-agent-orchestration")).isDirectory(), true);
  assert.equal(path.relative(activeRoot, backupRoot).startsWith(".."), true);
  assert.match(fs.readFileSync(path.join(backupRoot, "README.md"), "utf8"), /historical, non-active/i);

  const assertArchivedTree = (commit, sourcePrefix, backupPrefix) => {
    const rows = childProcess.execFileSync("git", ["ls-tree", "-r", commit, "--", sourcePrefix], { cwd: repository, encoding: "utf8" }).trim().split("\n").filter(Boolean).map((line) => {
      const match = /^(\d+) blob ([0-9a-f]+)\t(.+)$/.exec(line); assert.ok(match, line); return { oid: match[2], source: match[3] };
    });
    assert.ok(rows.length > 0, `${sourcePrefix} has archived files`);
    const archivedPaths = rows.map((row) => row.source.replace(sourcePrefix, backupPrefix));
    const currentOids = childProcess.execFileSync("git", ["hash-object", "--stdin-paths"], { cwd: repository, encoding: "utf8", input: `${archivedPaths.join("\n")}\n` }).trim().split("\n");
    assert.deepEqual(currentOids, rows.map((row) => row.oid), `${sourcePrefix} archive is byte-exact`);
  };
  assertArchivedTree("bfe09961869e03f5bf16b12d273ade857f45971d", ".claude/skills/mom-cto-orchestration", ".claude/skills-backup/pre-rev11-orchestration/mom-cto-orchestration");
  assertArchivedTree("bfe09961869e03f5bf16b12d273ade857f45971d", ".claude/skills/agent-prompts", ".claude/skills-backup/pre-rev11-orchestration/agent-prompts");
  assertArchivedTree("7f75a42eebb59ae09aff636326f4bb5e5b9c95d0", ".claude/skills/multi-agent-orchestration", ".claude/skills-backup/pre-rev11-orchestration/multi-agent-orchestration");

  const routedText = ["AGENTS.md", "CLAUDE.md"].map((file) => fs.readFileSync(path.join(repository, file), "utf8")).join("\n");
  assert.doesNotMatch(routedText, /(?:\/|skills\/)(?:mom-cto-orchestration|agent-prompts)(?:\/|`)/);
});

test("rev11 has one promoted live authority root and an excluded historical backup", () => {
  const authority = loadAuthority();
  const rev11Root = path.resolve(authority.packageRoot);
  assert.equal(path.basename(rev11Root), "rev11");
  assert.equal(fs.existsSync(path.join(rev11Root, "unified")), false);
  assert.equal(fs.statSync(path.join(rev11Root, "backup")).isDirectory(), true);
  assert.equal(fs.statSync(path.join(rev11Root, "authority")).isDirectory(), true);
  assert.match(fs.readFileSync(path.join(rev11Root, "backup/NOTICE.md"), "utf8"), /non-authoritative, read-only history/i);
  const liveSources = readToml(path.join(rev11Root, "provenance/live-source-lock.toml")).source;
  assert.equal(liveSources.length, 53);
  assert.equal(liveSources.find((row) => row.ref === "live:docs/arch/refactor/rev11/charters/J1.md")?.commit, "6a6c3c1a83709f7a58918e5b4e3d1eedcbd3ddac");
  assert.ok(liveSources.filter((row) => row.ref !== "live:docs/arch/refactor/rev11/charters/J1.md").every((row) => row.commit === "bfe09961869e03f5bf16b12d273ade857f45971d"));
  const repository = childProcess.execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: rev11Root, encoding: "utf8" }).trim();
  const preCutover = "bfe09961869e03f5bf16b12d273ade857f45971d";
  const rows = childProcess.execFileSync("git", ["ls-tree", "-r", preCutover, "--", "docs/arch/refactor/rev11"], { cwd: repository, encoding: "utf8" }).trim().split("\n").filter(Boolean).map((line) => {
    const match = /^(\d+) blob ([0-9a-f]+)\t(.+)$/.exec(line); assert.ok(match, line); return { oid: match[2], source: match[3] };
  }).filter((row) => !row.source.startsWith("docs/arch/refactor/rev11/unified/"));
  assert.ok(rows.length >= 650);
  const backupPaths = rows.map((row) => row.source.replace("docs/arch/refactor/rev11/", "docs/arch/refactor/rev11/backup/"));
  const currentOids = childProcess.execFileSync("git", ["hash-object", "--stdin-paths"], { cwd: repository, encoding: "utf8", input: `${backupPaths.join("\n")}\n` }).trim().split("\n");
  assert.deepEqual(currentOids, rows.map((row) => row.oid), "every pre-cutover legacy byte is retained under backup");

  const forbidden = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === "backup" || (directory.endsWith("sources") && entry.name === "review-history-migration")) continue;
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(file);
      else if (file.endsWith("tools/build-collapse-map.mjs") || file.endsWith("tools/policy.test.mjs") || file.endsWith("provenance/collapsed-node-map.toml")) continue;
      else if (fs.readFileSync(file).includes("docs/arch/refactor/rev11/unified")) forbidden.push(path.relative(rev11Root, file));
    }
  };
  walk(rev11Root);
  assert.deepEqual(forbidden, [], `operational /unified references remain: ${forbidden.join(", ")}`);
});
