#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "..");
// Single-extension authority (B-70): the ONLY live `extensions/*` trees are
// the editor extensions below. Every retired authority must be gone BOTH
// physically and from git tracking — the live VS Code extension is
// `packages/vue-vscode` and the live TypeScript plugin is
// `packages/typescript-plugin`; nothing under `extensions/` may shadow them.
const LIVE_EXTENSIONS = ["extensions/lapce", "extensions/zed"];
const RETIRED_EXTENSIONS = [
  "extensions/vscode",
  "extensions/typescript-plugin",
  "extensions/vue-vscode",
];
const PRODUCTION_VSIX_TARGETS = new Set([
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-x64",
]);
const COHORT_FIELDS = new Set([
  "vue_parser_version",
  "svelte_parser_version",
  "grammar_fingerprint_schema_version",
  "framework_artifact_schema_version",
  "carrier_source_space_schema_version",
  "carrier_source_map_schema_version",
  "session_current_parser_version",
  "carrier_cache_serialization_version",
]);

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function readJson(root, path) {
  return JSON.parse(readFileSync(join(root, path), "utf8"));
}

function sortedObject(value) {
  return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)));
}

function equalSets(left, right) {
  return left.size === right.size && [...left].every((value) => right.has(value));
}

function trackedFiles(root) {
  return execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: root,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .map((path) => path.replaceAll("\\", "/"))
    .filter((path) => existsSync(join(root, path)));
}

export function verifyNoLegacyExtensionAuthority(root = DEFAULT_ROOT, files = trackedFiles(root)) {
  // PHYSICAL arm: no retired extension tree may exist on disk.
  for (const retired of RETIRED_EXTENSIONS) {
    invariant(!existsSync(join(root, retired)), `${retired} still exists`);
  }
  // TRACKED arm (retired): no retired extension tree may have tracked files.
  for (const retired of RETIRED_EXTENSIONS) {
    invariant(
      !files.some((path) => path === retired || path.startsWith(`${retired}/`)),
      `${retired} still has tracked files`,
    );
  }
  // TRACKED arm (allowlist): every tracked path under extensions/ must belong
  // to a live extension.
  for (const path of files.filter((candidate) => candidate.startsWith("extensions/"))) {
    invariant(
      LIVE_EXTENSIONS.some((live) => path.startsWith(`${live}/`)),
      `tracked extensions residue outside the live allowlist: ${path}`,
    );
  }

  for (const path of files.filter((candidate) => candidate.endsWith("package.json"))) {
    const source = readFileSync(join(root, path), "utf8").replaceAll("\\", "/");
    for (const retired of RETIRED_EXTENSIONS) {
      invariant(!source.includes(retired), `${path} references ${retired}`);
    }
  }

  for (const path of [
    "packages/playground/scripts/generate-vue-language.ts",
    "packages/playground/src/editor/vueLanguage.ts",
  ]) {
    const source = readFileSync(join(root, path), "utf8");
    invariant(
      source.includes("packages/vue-vscode"),
      `${path} lacks the current grammar authority`,
    );
    for (const retired of RETIRED_EXTENSIONS) {
      invariant(!source.includes(retired), `${path} references ${retired}`);
    }
  }
}

export function verifyScannerFreeBoundary(root = DEFAULT_ROOT, files = trackedFiles(root)) {
  for (const retired of [
    "crates/verter_lsp/src/documents/sfc_scanner.rs",
    "crates/verter_parser/src/cursor/script_detector.rs",
    "packages/vue-vscode/src/css/styleBlockScanner.ts",
  ]) {
    invariant(!files.includes(retired), `${retired} was reintroduced`);
  }
  for (const fixture of [
    "crates/verter_session/tests/cases/compile-fail/scanners_replacement_raw_parser_public.rs",
    "crates/verter_session/tests/cases/compile-fail/scanners_replacement_script_detector_public.rs",
  ]) {
    invariant(existsSync(join(root, fixture)), `${fixture} compile boundary is missing`);
  }
}

export function verifyLedger(root = DEFAULT_ROOT) {
  const ledger = readJson(root, "docs/arch/scanners-replacement-capability-ledger.json");
  const rows = ledger.rows;
  invariant(Array.isArray(rows) && rows.length > 0, "capability ledger has no rows");
  const rowKeys = new Set(ledger.row_schema);
  const identities = new Set();
  const byCapability = {};
  const byDisposition = {};
  let productionRows = 0;

  for (const row of rows) {
    invariant(
      equalSets(new Set(Object.keys(row)), rowKeys),
      `non-canonical ledger row ${row.path}`,
    );
    const identity = `${row.path}\0${row.symbol}`;
    invariant(!identities.has(identity), `duplicate ledger row ${row.path}::${row.symbol}`);
    identities.add(identity);
    invariant(
      ["migrate", "delete", "allowed_nested", "allowed_standalone", "test_only"].includes(
        row.disposition,
      ),
      `invalid disposition ${row.disposition}`,
    );
    for (const field of ["acceptance_id", "test", "architecture_guard"]) {
      invariant(
        typeof row[field] === "string" && row[field].length > 0,
        `${identity} lacks ${field}`,
      );
    }
    byCapability[row.capability_class] = (byCapability[row.capability_class] ?? 0) + 1;
    byDisposition[row.disposition] = (byDisposition[row.disposition] ?? 0) + 1;
    if (row.runtime_role === "production_runtime") {
      invariant(
        row.disposition !== "test_only",
        `${identity} misclassifies production as test-only`,
      );
      productionRows += 1;
    }
  }

  invariant(ledger.statistics.rows_total === rows.length, "ledger rows_total is stale");
  invariant(
    JSON.stringify(sortedObject(ledger.statistics.by_capability_class)) ===
      JSON.stringify(sortedObject(byCapability)),
    "ledger by_capability_class is stale",
  );
  invariant(
    JSON.stringify(sortedObject(ledger.statistics.by_disposition)) ===
      JSON.stringify(sortedObject(byDisposition)),
    "ledger by_disposition is stale",
  );
  // RETRACTED self-attestation: the former `independently_discovered_candidates
  // === rows.length` / `classified_ledger_rows === rows.length` checks asserted
  // the ledger's own row count as the discovered count — a receipt of nothing
  // (CLAUDE.md "Verification Must Prove Execution"). The B-52/B-91 evidence is
  // the EXTERNAL input-bound discovery receipt; this verifier checks only the
  // RECORD's shape (fresh run + retraction + honest reopened status).
  const freshRun = ledger.discovery?.fresh_run;
  invariant(
    typeof freshRun?.receipt === "string" && freshRun.receipt.includes("DISCOVERY-RECEIPT.md"),
    "discovery record must name its input-bound receipt",
  );
  invariant(
    /^[0-9a-f]{40}$/.test(freshRun?.fixed_tip ?? ""),
    "discovery record must pin the fixed tip it ran against",
  );
  for (const input of ["git_ls_files", "cargo_metadata", "pnpm_workspace_graph"]) {
    invariant(
      typeof freshRun?.inputs?.[input]?.sha256 === "string" &&
        freshRun.inputs[input].sha256.length > 0,
      `discovery record must carry an input hash for ${input}`,
    );
  }
  invariant(
    typeof ledger.discovery?.retraction === "string" &&
      ledger.discovery.retraction.includes("RETRACTED"),
    "the retraction of the prior self-attested closure must stay recorded",
  );
  invariant(
    typeof ledger.set_equality.b52_b91_status === "string" &&
      ledger.set_equality.b52_b91_status.includes("REOPENED"),
    "B-52/B-91 must remain explicitly reopened while any named residual is open",
  );
  invariant(ledger.set_equality.unclassified_runtime_rows === 0, "unclassified runtime row");
  invariant(ledger.set_equality.deferred_runtime_rows === 0, "deferred runtime row");
  invariant(ledger.consumer_matrix.length === productionRows, "consumer matrix is not total");

  const ledgerTargets = new Set(
    rows
      .filter((row) => row.acceptance_id === "B-78")
      .map((row) => row.symbol.replace(/^vsix_bundle_/, "").replaceAll("_", "-")),
  );
  const extension = readJson(root, "packages/vue-vscode/package.json");
  const packageTargets = new Set(
    Object.keys(extension.scripts)
      .filter((script) => script.startsWith("package:") && script !== "package:dev:universal")
      .map((script) => script.slice("package:".length)),
  );
  invariant(equalSets(ledgerTargets, PRODUCTION_VSIX_TARGETS), "B-78 ledger target set drifted");
  invariant(equalSets(packageTargets, PRODUCTION_VSIX_TARGETS), "B-78 package target set drifted");
}

export function verifySchemaAndGuardInventory(root = DEFAULT_ROOT) {
  const schema = readJson(root, "schemas/scanners-replacement-v1.schema.json");
  const cohort = new Set(Object.keys(schema.persisted_carrier_artifact_cohort?.fields ?? {}));
  invariant(
    equalSets(cohort, COHORT_FIELDS),
    "persisted carrier cohort is not the exact eight-field set",
  );

  const guards = new Map([
    [
      "crates/verter_session/src/carrier_artifact_cohort.rs",
      [
        "persisted_carrier_cohort_has_frozen_eight_word_shape",
        "every_cohort_field_uses_exact_equality",
      ],
    ],
    [
      "crates/verter_session/src/carrier_publication_store_tests.rs",
      ["registered_file_structure_is_the_envelope_owner"],
    ],
    [
      "crates/verter_lsp/src/documents/carrier_structure.rs",
      ["registered_projection_preserves_duplicate_attributes_and_sealed_refs"],
    ],
    [
      "crates/verter_protocol/src/component_meta.rs",
      ["schema8_full_body_uses_tag_26_and_roundtrips_supported_contract"],
    ],
  ]);
  for (const [path, names] of guards) {
    const source = readFileSync(join(root, path), "utf8");
    for (const name of names) invariant(source.includes(`fn ${name}`), `${path} lacks ${name}`);
  }
}

function run(root, command, args) {
  console.log(`\n[verify] ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: root, stdio: "inherit", env: process.env });
}

export function runFocusedRustGuards(root = DEFAULT_ROOT) {
  run(root, "cargo", [
    "test",
    "-p",
    "verter_session",
    "--test",
    "main",
    "cases::scanners_replacement::scanners_replacement",
  ]);
  run(root, "cargo", ["test", "-p", "verter_session", "--lib", "carrier_artifact_cohort::tests::"]);
  run(root, "cargo", [
    "test",
    "-p",
    "verter_session",
    "--lib",
    "registered_file_structure_is_the_envelope_owner",
  ]);
  run(root, "cargo", [
    "test",
    "-p",
    "verter_lsp",
    "--lib",
    "registered_projection_preserves_duplicate_attributes_and_sealed_refs",
  ]);
  run(root, "cargo", [
    "test",
    "-p",
    "verter_protocol",
    "--lib",
    "schema8_full_body_uses_tag_26_and_roundtrips_supported_contract",
  ]);
  run(root, "cargo", [
    "test",
    "-p",
    "verter_session",
    "--features",
    "compile-fail",
    "--test",
    "main",
    "is_not_public",
  ]);
}

export function verifyRepository(root = DEFAULT_ROOT, { runCargo = true } = {}) {
  const files = trackedFiles(root);
  verifyNoLegacyExtensionAuthority(root, files);
  verifyScannerFreeBoundary(root, files);
  verifyLedger(root);
  verifySchemaAndGuardInventory(root);
  if (runCargo) runFocusedRustGuards(root);
  console.log("\n[verify] scanners replacement non-broker checks passed");
}

function parseArgs(args) {
  if (args.length === 0) return;
  invariant(
    args.length === 2 && args[0] === "--profile" && args[1] === "b4",
    "only --profile b4 is supported",
  );
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    parseArgs(process.argv.slice(2));
    verifyRepository();
  } catch (error) {
    console.error(`[verify] FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
