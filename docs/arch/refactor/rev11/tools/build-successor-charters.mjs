#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { NODE_FIELDS, PACKAGE_ROOT, loadAuthority, readToml } from "./lib.mjs";

const check = process.argv.includes("--check");
const authority = loadAuthority(PACKAGE_ROOT);
const reviewProfiles = new Map(
  readToml(path.join(PACKAGE_ROOT, "catalogs/review-profiles.toml")).profile.map((row) => [
    row.id,
    row,
  ]),
);
const conflictDomains = new Map(
  readToml(path.join(PACKAGE_ROOT, "catalogs/conflict-domains.toml")).domain.map((row) => [
    row.id,
    row,
  ]),
);
const successor = authority.nodes.filter((node) => /^(?:NCK|NCF-|LSO|EPR)/u.test(node.id));

function sourceCharter(node) {
  return path.join(PACKAGE_ROOT, "sources/successor-dag-charter-pack", node.charter);
}

function section(text, heading) {
  return (
    new RegExp(`^## ${heading}\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))`, "mu")
      .exec(text)?.[1]
      ?.trim() || ""
  );
}

function nested(heading, body) {
  if (!body) return "";
  return `### ${heading}\n\n${body.replace(/^### /gmu, "#### ")}`;
}

function metadata(node) {
  const fields = NODE_FIELDS.map((key) => {
    const value = node[key];
    return `${key}=${Array.isArray(value) ? value.join(",") : value === undefined ? "" : String(value)}`;
  });
  return `<!-- unified-charter-v2\n${fields.join("\n")}\n-->`;
}

function packPathRoots(node) {
  const text = fs.readFileSync(sourceCharter(node), "utf8");
  return [...section(text, "Expected production surfaces").matchAll(/`([^`]+)`/gu)]
    .map((match) => match[1].split("::", 1)[0])
    .filter((value) =>
      /^(?:\.claude|crates|docs|editors|extensions|packages|scripts|test-corpora|tools)(?:\/|$)/u.test(
        value,
      ),
    );
}

function surfaces(node) {
  const roots = [...new Set(packPathRoots(node))];
  if (roots.length) return roots;
  const fallback = [
    ...new Set(node.conflict_domains.flatMap((id) => conflictDomains.get(id)?.path_roots || [])),
  ];
  if (!fallback.length)
    throw new Error(
      `${node.id}: exact pack charter and acquired conflict domains have no machine-readable production path root`,
    );
  return fallback;
}

function predecessorSection(node, byId) {
  const predecessors = node.predecessors.length
    ? node.predecessors.map(
        (id) =>
          `- **${id}:** exact current receipt ID and digest for “${byId.get(id).name}”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.`,
      )
    : ["- **Direct DAG predecessors:** none."];
  const external = node.external_requirements.length
    ? node.external_requirements.map(
        (id) =>
          `- **External custody ${id}:** require the exact immutable authorization slot at dispatch and a finalized-candidate-bound authorization before evidence or acceptance.`,
      )
    : [
        "- **External custody:** no node-specific external authorization beyond the package activation boundary.",
      ];
  return [...predecessors, ...external].join("\n");
}

function reviewSection(node) {
  const profile = reviewProfiles.get(node.review_profile);
  if (!profile) throw new Error(`${node.id}: unknown review profile ${node.review_profile}`);
  const lenses = profile.lenses.map((lens) => `\`${lens}\``).join(", ");
  return `Apply \`${profile.id}\`: ${profile.reviewers} fresh distinct harness task${profile.reviewers === 1 ? "" : "s"} covering exactly ${lenses}. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete ${profile.reviewers}/${profile.reviewers} current-round profile to contain independent clean PASS reports on the exact candidate tree, plus \`${profile.confirmation_policy}\` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.`;
}

function dispatchSection(node) {
  const profile = reviewProfiles.get(node.review_profile);
  return `The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and \`codex/<node>\` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; ${profile.reviewers} fresh distinct harness review task${profile.reviewers === 1 ? "" : "s"} for exactly ${profile.lenses.map((lens) => `\`${lens}\``).join(", ")}, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.`;
}

function render(node, byId) {
  const source = fs.readFileSync(sourceCharter(node), "utf8");
  const outcome = section(source, "Independently acceptable outcome");
  const production = section(source, "Expected production surfaces");
  const productionChanges = section(source, "Expected production changes");
  const apis = section(source, "Named APIs and data boundaries");
  const architecturalBoundary = section(source, "Architectural boundary");
  const principles = section(source, "Binding architecture") || section(source, "Architecture");
  const subblocks = section(source, "Internal subblocks");
  const laws = section(source, "Data, identity, invalidation, and publication laws");
  const migration = section(source, "Migration and cutover");
  const deletions = section(source, "Deletions");
  const forbidden = section(source, "Forbidden designs");
  const sourceAcceptance =
    section(source, "Acceptance IDs and discriminating proof") || section(source, "Acceptance IDs");
  const performance = section(source, "Performance and bounded work");
  const abort = section(source, "Mandatory rescope and abort conditions");
  const verification = section(source, "Targeted verification") || section(source, "Verification");
  const consumers = section(source, "Consumers and unlocks");
  const surfaceList = surfaces(node);
  const originalDetails = [
    nested("Architectural boundary", architecturalBoundary),
    nested("Binding architecture", principles),
    nested("Expected production changes", productionChanges),
    nested("Internal subblocks", subblocks),
    nested("Identity, invalidation, and publication", laws),
    nested("Migration and cutover", migration),
    nested("Consumers and unlocks", consumers),
  ]
    .filter(Boolean)
    .join("\n\n");
  const sourceProof = sourceAcceptance
    ? `\n\n### Pack-specific proof obligations\n\n${sourceAcceptance.replace(/^### /gmu, "#### ")}`
    : "";
  const sourceVerification = verification
    ? `\n\n### Pack-specific verification inventory\n\n${verification.replace(/^### /gmu, "#### ")}`
    : "";
  return `${metadata(node)}

# ${node.id} — ${node.name}

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

${outcome}

## Concrete surfaces and APIs

- Production surfaces: ${surfaceList.map((value) => `\`${value}\``).join(", ")}.
- Pack production inventory:
${production || (productionChanges ? "  - The exact pack production changes are preserved under Source-specific scope below." : "  - no production mutation; this is a constitution/convergence authority block.")}
- Named API/data boundaries:
${apis || "  - exact schema and receipt boundaries declared by this charter."}

## Exact predecessor contracts

${predecessorSection(node, byId)}

## Source-specific scope

${originalDetails}

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **${node.id}-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **${node.id}-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **${node.id}-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **${node.id}-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: \`crates/verter_session/tests/cases\`, \`crates/verter_protocol/tests\`, \`packages/typescript-plugin/src\`, and the exact generated vertical fixture selected by this node.
${sourceProof}

## Deletions and forbidden designs

${deletions || "- Delete or structurally reject the displaced authority named by the source charter after same-candidate replacement proof."}

${forbidden || "- Delete or structurally reject every duplicate semantic, publication, mapping, provider, or consumer route owned by this node."}

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: ${node.max_production_loc} production LOC, ${node.max_production_files} production files, ${node.max_related_packages} related crates/packages.
- Mandatory rescope above ${node.rescope_loc} production LOC, ${node.rescope_files} files, ${node.rescope_unrelated_packages} unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

${performance}

## Abort conditions

${abort || "- Stop before mutation if the exact sole owner, predecessor contract, or evidence boundary is false."}

## Targeted verification

1. \`node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict\`
2. Run every final command in the bound \`${node.gate_profile}\` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.${sourceVerification}

## Review and lower-severity findings

${reviewSection(node)}

## Dispatch-time immutable bindings

${dispatchSection(node)}

## Citations

${node.source_refs.map((ref) => `- \`${ref}\``).join("\n")}

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated \`applicable_nodes\` ledger and embedded verbatim in cold packets.
`;
}

const byId = new Map(authority.nodes.map((node) => [node.id, node]));
const outputs = new Map(
  successor.map((node) => {
    const file = path.join(PACKAGE_ROOT, node.charter);
    let output = render(node, byId);
    if (fs.existsSync(file)) {
      const current = fs.readFileSync(file, "utf8");
      const currentClauses = current.split(/^## Transferred source requirement atoms$/mu)[1];
      if (currentClauses !== undefined) {
        output = `${output.split(/^## Transferred source requirement atoms$/mu)[0].trimEnd()}\n\n## Transferred source requirement atoms${currentClauses}`;
      }
    }
    return [file, output];
  }),
);
const stale = [...outputs]
  .filter(([file, text]) => !fs.existsSync(file) || fs.readFileSync(file, "utf8") !== text)
  .map(([file]) => path.relative(PACKAGE_ROOT, file));
if (check) {
  if (stale.length) {
    console.error(`STALE successor charters: ${stale.join(", ")}`);
    process.exit(1);
  }
  console.log(`build-successor-charters: PASS (${outputs.size} exact charters)`);
} else {
  for (const [file, text] of outputs) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, text);
  }
  console.log(`build-successor-charters: wrote ${outputs.size} exact charters`);
}
