#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const authority = loadAuthority(PACKAGE_ROOT);
const byId = new Map(authority.nodes.map((node) => [node.id, node]));
const outputs = new Map();
const reviewProfiles = new Map(
  readToml(path.join(PACKAGE_ROOT, "catalogs/review-profiles.toml")).profile.map((profile) => [
    profile.id,
    profile,
  ]),
);

function predecessorSection(node) {
  const receipts = node.predecessors.length
    ? node.predecessors.map(
        (id) =>
          `- **${id}:** exact current receipt ID and digest for “${byId.get(id).name}”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.`,
      )
    : [
        "- **Direct DAG predecessors:** none. This is a source-canonical entry; its external requirements remain mandatory and are not predecessor substitutes.",
      ];
  const external = node.external_requirements.length
    ? node.external_requirements.map(
        (requirement) =>
          `- **External custody ${requirement}:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.`,
      )
    : [
        "- **External custody:** no node-specific external authorization beyond the package activation boundary.",
      ];
  return `## Exact predecessor contracts\n\n${[...receipts, ...external].join("\n")}`;
}

function reviewSection(node) {
  const profile = reviewProfiles.get(node.review_profile);
  if (!profile) throw new Error(`${node.id}: unknown review profile ${node.review_profile}`);
  const lenses = profile.lenses.map((lens) => `\`${lens}\``).join(", ");
  return `## Review and lower-severity findings\n\nApply \`${profile.id}\`: ${profile.reviewers} fresh distinct harness task${profile.reviewers === 1 ? "" : "s"} covering exactly ${lenses}. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete ${profile.reviewers}/${profile.reviewers} current-round profile to contain independent clean PASS reports on the exact candidate tree, plus \`${profile.confirmation_policy}\` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.`;
}

function dispatchSection(node) {
  const profile = reviewProfiles.get(node.review_profile);
  return `## Dispatch-time immutable bindings\n\nThe packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and \`codex/<node>\` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; ${profile.reviewers} fresh distinct harness review task${profile.reviewers === 1 ? "" : "s"} for exactly ${profile.lenses.map((lens) => `\`${lens}\``).join(", ")}, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.`;
}

function section(text, heading) {
  return (
    new RegExp(`^## ${heading}\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))`, "m")
      .exec(text)?.[1]
      ?.trim() || ""
  );
}

function replaceSection(text, heading, body) {
  return text.replace(
    new RegExp(`^## ${heading}\\n[\\s\\S]*?(?=^## )`, "m"),
    `## ${heading}\n\n${body}\n\n`,
  );
}

function proofSection(node, text) {
  let testHomes = /^- Test homes:.*$/m.exec(
    section(text, "Acceptance IDs and discriminating proof"),
  )?.[0];
  if (!testHomes && /^(?:GH|FB|REL)\d+$/u.test(node.id)) {
    testHomes =
      "- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.";
  }
  if (!testHomes) throw new Error(`${node.id}: cannot preserve test-home boundary`);
  return `Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **${node.id}-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **${node.id}-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **${node.id}-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **${node.id}-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
${testHomes}`;
}

function budgetSection(node, text) {
  const current = section(text, "Budgets and mandatory rescope").split("\n");
  const preserved = current.filter((line) => !line.startsWith("- Performance budget:"));
  if (preserved.length !== current.length - 1)
    throw new Error(`${node.id}: expected one performance budget`);
  preserved.push(
    "- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.",
  );
  return preserved.join("\n");
}

function verificationSection(node, text) {
  const current = section(text, "Targeted verification");
  const targeted = /^1\.\s+`[^`\n]+`$/m.exec(current)?.[0];
  if (!targeted) throw new Error(`${node.id}: targeted verification command is missing`);
  const governance = /^(?:GH|FB|REL)\d+$/u.test(node.id)
    ? "\n2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`"
    : "";
  const next = governance ? 3 : 2;
  return `${targeted}${governance}
${next}. Run every final command in the bound \`${node.gate_profile}\` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
${next + 1}. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.`;
}

for (const node of authority.nodes) {
  const file = path.join(PACKAGE_ROOT, node.charter);
  let text = fs.readFileSync(file, "utf8");
  if (node.review_profile !== "history" && !/^(?:NCK|NCF-|LSO|EPR)/u.test(node.id)) {
    text = text.replace(
      /^## Exact predecessor contracts\n[\s\S]*?(?=^## )/m,
      `${predecessorSection(node)}\n\n`,
    );
    text = text.replace(
      /^## Review and lower-severity findings\n[\s\S]*?(?=^## )/m,
      `${reviewSection(node)}\n\n`,
    );
    text = text.replace(
      /^## Dispatch-time immutable bindings\n[\s\S]*?(?=^## )/m,
      `${dispatchSection(node)}\n\n`,
    );
    text = text.replace(
      /^- Mutation boundary: only the exact files, symbols, routes, and migration rows assigned to `[A-Z][A-Z0-9-]*::[a-z][a-z0-9_]+`; sibling ownership is excluded\.$/m,
      "- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.",
    );
    text = text.replace(
      /^- \*\*Leaf boundary:\*\* `[A-Z][A-Z0-9-]*::[a-z][a-z0-9_]+` is the exclusive acceptance subset for “[^”]+”; it owns no sibling API, migration population, corpus, or deletion unit\.$/m,
      "- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.",
    );
    text = replaceSection(
      text,
      "Acceptance IDs and discriminating proof",
      proofSection(node, text),
    );
    text = replaceSection(text, "Budgets and mandatory rescope", budgetSection(node, text));
    text = replaceSection(text, "Targeted verification", verificationSection(node, text));
    text = text.replace(
      "required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions)",
      "required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, preflight proof selection and rationale, applicable behavioral TDD RED/GREEN evidence, selected existing/type/compiler/static/gate/inspection/benchmark evidence, gate receipt digests, review report digests, residual findings, and abort/rescope decisions)",
    );
  }
  outputs.set(file, text);
}

const stale = [...outputs]
  .filter(([file, text]) => fs.readFileSync(file, "utf8") !== text)
  .map(([file]) => path.relative(PACKAGE_ROOT, file));
if (check) {
  if (stale.length) {
    console.error(`STALE operational charters: ${stale.join(", ")}`);
    process.exit(1);
  }
  console.log(`build-operational-charters: PASS (${outputs.size} exact charters)`);
} else {
  for (const [file, text] of outputs) fs.writeFileSync(file, text);
  console.log(`build-operational-charters: wrote ${outputs.size} exact charters`);
}
