/** @ai-generated - Successor-DAG amendment and legacy-architecture cutover guards. */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { validateSuccessorSourcePack } from "./build-successor-source-pack-lock.mjs";
import * as lib from "./lib.mjs";

const STATIC_IDS = [
  "NCK0",
  "NCK1",
  "NCK2",
  "NCK3",
  "NCK4",
  "NCK5",
  "NCK6",
  "NCK7",
  "NCK8",
  "LSO0",
  "LSO1",
  "LSO2",
  "LSO3",
  "LSO4",
  "LSO5",
  "LSO6",
  "LSO7",
  "LSO8",
  "LSO9",
  "LSO10",
  "EPR0",
  "EPR1",
  "EPR2",
  "EPR3",
  "EPR4",
  "EPR5",
  "EPR6",
];

function productionSurfaces(text) {
  const line = /^- Production surfaces: (.+)$/mu.exec(text)?.[1] || "";
  return [...line.matchAll(/`([^`]+)`/gu)].map((match) => match[1]);
}

function relativeFiles(root) {
  const files = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) files.push(path.relative(root, absolute).split(path.sep).join("/"));
    }
  };
  walk(root);
  return files.sort();
}

test("successor topology is registered as three atomic product trains plus generated checker convergence", () => {
  const authority = lib.loadAuthority();
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const familyIds = authority.nodes
    .map((node) => node.id)
    .filter((id) => id.startsWith("NCF-"))
    .sort();

  assert.equal(authority.nodes.length, 268);
  assert.ok(STATIC_IDS.every((id) => byId.has(id)));
  assert.equal(familyIds.length, 30);
  assert.ok(byId.has("NCKF0"));
  assert.deepEqual([...byId.get("NCKF0").predecessors].sort(), familyIds);
  assert.ok(byId.get("NCK8").predecessors.includes("NCKF0"));
  assert.deepEqual(byId.get("NCK8").external_requirements, []);

  assert.deepEqual(byId.get("NCK7").conditional_predecessors, [
    "CLI2:when-opened",
    "CLI4:when-opened",
  ]);
  assert.deepEqual(byId.get("LSO5").predecessors, ["LSO4", "LRA0"]);
  assert.deepEqual(byId.get("LSO8").predecessors, ["LSO1", "LSO5", "LSO6", "LRA0", "ENCL0"]);
  assert.equal(byId.get("EPR2").optional, true);
  assert.equal(byId.get("EPR3").optional, true);
  assert.deepEqual(byId.get("EPR4").conditional_predecessors, [
    "EPR2:when-opened",
    "EPR3:when-opened",
  ]);
  assert.deepEqual(byId.get("NCK2").conflict_domains, [
    "semantic_authority",
    "semantic_cache_store",
    "public_protocol",
    "diagnostic_action_service",
  ]);
  assert.deepEqual(byId.get("NCK4").conflict_domains, [
    "semantic_authority",
    "vertical_manifest",
    "performance_evidence",
    "successor_generator_tooling",
  ]);
  assert.deepEqual(byId.get("NCK7").conflict_domains, [
    "diagnostic_action_service",
    "public_protocol",
    "lsp_publication",
    "cli_application",
    "capability_catalog",
  ]);
  assert.deepEqual(byId.get("LSO3").conflict_domains, [
    "semantic_authority",
    "mapping_geometry",
    "lsp_publication",
    "performance_evidence",
  ]);
  assert.deepEqual(byId.get("LSO6").conflict_domains, [
    "provider_lifecycle",
    "mapping_geometry",
    "public_protocol",
    "semantic_authority",
  ]);
  assert.deepEqual(byId.get("LSO7").conflict_domains, [
    "provider_lifecycle",
    "public_protocol",
    "lsp_publication",
    "semantic_authority",
  ]);
  assert.deepEqual(byId.get("EPR3").conflict_domains, [
    "provider_lifecycle",
    "program_authority",
    "source_lineage",
    "cli_application",
  ]);
});

test("executable supply-chain nodes use the closed high-risk security review profile", () => {
  const authority = lib.loadAuthority();
  const profiles = new Map(
    lib
      .readToml(path.join(authority.packageRoot, "catalogs/review-profiles.toml"))
      .profile.map((row) => [row.id, row]),
  );
  assert.deepEqual(profiles.get("security-3"), {
    id: "security-3",
    reviewers: 3,
    independent: true,
    lenses: ["adversarial", "conformance", "supply-chain-platform"],
    dispatch: "fresh-distinct-harness-task",
    provider_policy: "provider-neutral",
    minimum_effort: "high",
    risk_band: "high",
    confirmation_policy: "independent-full",
  });
});

test("existing owners carry the successor contract amendments instead of successor-private copies", () => {
  const authority = lib.loadAuthority();
  const expected = new Map([
    ["B2", "recoverable parser diagnostic"],
    ["PAR0", "RecoverySnapshot"],
    ["TCM3", "correction overlay"],
    ["TCM4", "mapper snapshot"],
    ["IDX0", "negative complete result"],
    ["LRA0", "authority state is separate"],
    ["PUB0", "SemanticOperationOutcome"],
    ["VIM0", "native_checker_slice"],
    ["VIM1", "language_service_operation"],
    ["PER0", "engine source adapters"],
    ["H2", "ProviderEpoch"],
    ["H3", "mixed-epoch"],
    ["COX0", "WorkspaceOnly"],
    ["CLI2", "DiagnosticService"],
    ["CLI4", "engine acquisition"],
  ]);
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  for (const [id, phrase] of expected) {
    const charter = fs.readFileSync(path.join(authority.packageRoot, byId.get(id).charter), "utf8");
    assert.match(charter, new RegExp(phrase, "i"), `${id} owns its successor amendment`);
  }
});

test("successor charters preserve exact pack-owned mutation roots and generated-family obligations", () => {
  const authority = lib.loadAuthority();
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const charter = (id) =>
    fs.readFileSync(path.join(authority.packageRoot, byId.get(id).charter), "utf8");

  assert.deepEqual(productionSurfaces(charter("NCK2")), [
    "crates/verter_session/src/semantic_query",
    "crates/verter_semantic",
    "crates/verter_diagnostics",
    "crates/verter_protocol",
    "crates/verter_session/tests",
  ]);

  const generated = charter("NCF-CO-OVER");
  const domains = new Map(
    lib
      .readToml(path.join(authority.packageRoot, "catalogs/conflict-domains.toml"))
      .domain.map((row) => [row.id, row]),
  );
  const generatedNode = byId.get("NCF-CO-OVER");
  const expectedGeneratedSurfaces = [
    ...new Set(generatedNode.conflict_domains.flatMap((id) => domains.get(id).path_roots)),
  ];
  assert.deepEqual(productionSurfaces(generated), expectedGeneratedSurfaces);
  assert.ok(!productionSurfaces(generated).includes("tools"));
  assert.match(generated, /^### Architectural boundary$/mu);
  assert.match(generated, /^#### Required fact and proof inputs$/mu);
  assert.match(generated, /^#### Required oracle obligations$/mu);
  assert.match(generated, /^### Expected production changes$/mu);
  assert.match(
    generated,
    /Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice\./u,
  );
  assert.doesNotMatch(generated, /no production mutation/u);
});

test("only the exact source-surface conflict-domain reconciliations are accepted", () => {
  const authority = lib.loadAuthority();
  const baseline = lib.validateAuthority(authority, {
    checkGenerated: false,
    checkAmendments: false,
  });
  assert.equal(baseline.filter((error) => error.includes("reconciled conflict_domains")).length, 0);

  const nck2 = authority.nodes.find((node) => node.id === "NCK2");
  nck2.conflict_domains.push("performance_evidence");
  const mutated = lib.validateAuthority(authority, {
    checkGenerated: false,
    checkAmendments: false,
  });
  assert.ok(
    mutated.includes(
      "NCK2: live reconciled conflict_domains differ from the exact source-pack module",
    ),
  );
});

test("source-pack initial state remains an exact operational field", () => {
  const authority = lib.loadAuthority();
  authority.nodes.find((node) => node.id === "NCK0").initial_state = "SUPERSEDED";
  const mutated = lib.validateAuthority(authority, {
    checkGenerated: false,
    checkAmendments: false,
  });
  assert.ok(mutated.includes("NCK0: live initial_state differs from exact source-pack module"));
});

test("retained successor source pack has the complete immutable input inventory", () => {
  const authority = lib.loadAuthority();
  const packRoot = path.join(authority.packageRoot, "sources/successor-dag-charter-pack");
  const files = relativeFiles(packRoot);
  assert.equal(files.length, 92);
  for (const required of [
    "authority/root-module-registration.example.toml",
    "catalogs/external-requirements.additions.toml",
    "catalogs/review-profile.security-3.example.toml",
    "generated/CHARTER-INDEX.md",
    "sources/legacy-arch-reconciliation.md",
    "tools/validate_package.py",
  ])
    assert.ok(files.includes(required), `source pack is missing ${required}`);
  assert.deepEqual(validateSuccessorSourcePack(authority.packageRoot), []);

  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "verter-successor-source-pack-"));
  try {
    fs.mkdirSync(path.join(scratch, "sources"), { recursive: true });
    fs.mkdirSync(path.join(scratch, "provenance"), { recursive: true });
    fs.cpSync(packRoot, path.join(scratch, "sources/successor-dag-charter-pack"), {
      recursive: true,
    });
    fs.copyFileSync(
      path.join(authority.packageRoot, "provenance/successor-source-pack-lock.toml"),
      path.join(scratch, "provenance/successor-source-pack-lock.toml"),
    );
    fs.appendFileSync(
      path.join(scratch, "sources/successor-dag-charter-pack/README.md"),
      "\nmutated\n",
    );
    assert.ok(
      validateSuccessorSourcePack(scratch).some((error) =>
        error.includes("path/byte/digest inventory mismatch"),
      ),
    );
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

test("legacy architecture disposition is exact, complete, deletion-bound, and reference-clean", () => {
  const authority = lib.loadAuthority();
  assert.equal(typeof lib.validateLegacyArchitectureDisposition, "function");
  assert.deepEqual(lib.validateLegacyArchitectureDisposition(authority), []);

  const disposition = lib.readToml(
    path.join(authority.packageRoot, "catalogs/legacy-arch-disposition.toml"),
  );
  assert.equal(disposition.source_commit, "0698f521c2b8acb6521cc7f038afe4031fd94c41");
  assert.equal(disposition.source_tree, "5b04f40d54628ac300661825b56442070dabe4e2");
  const historical = disposition.entry.filter((row) => row.disposition === "historical_evidence");
  const transferred = disposition.entry.filter(
    (row) => row.disposition === "transferred_exact_source",
  );
  assert.equal(historical.length, 326);
  assert.ok(
    historical.every(
      (row) =>
        row.path.startsWith("docs/arch/architecture-lock/ledger/") &&
        row.evidence_class === "immutable_audit_ledger",
    ),
  );
  assert.equal(transferred.length, 92);
  assert.ok(
    transferred.every(
      (row) =>
        row.targets.length && row.replacement_sources.length && row.requirement_atoms.length === 1,
    ),
  );
  assert.deepEqual(
    transferred.find((row) =>
      row.path.endsWith("real-provider-harness-template-position-locators.md"),
    ).targets,
    ["LSO2", "LSO9"],
  );
});

test("a newly introduced current legacy path is a fail-closed cleanup error", () => {
  assert.deepEqual(lib.legacyArchitectureCurrentPathErrors(["docs/arch/post-orc0-note.md"]), [
    "uncatalogued or undeleted current legacy architecture path(s): docs/arch/post-orc0-note.md",
  ]);
  assert.deepEqual(
    lib.legacyArchitectureCurrentPathErrors(["docs/arch/refactor/rev11/sources/current.md"]),
    [],
  );
});

test("exact retained legacy source inventory rejects symlinks", () => {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "verter-legacy-transfer-inventory-"));
  try {
    fs.writeFileSync(path.join(scratch, "retained.md"), "retained\n");
    fs.symlinkSync(path.join(scratch, "retained.md"), path.join(scratch, "extra-link"));
    const inventory = lib.exactRegularFileInventory(
      scratch,
      "exact retained legacy source inventory",
    );
    assert.deepEqual(inventory.files, ["retained.md"]);
    assert.deepEqual(inventory.errors, [
      "exact retained legacy source inventory contains unsupported filesystem entry: extra-link",
    ]);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});
