#!/usr/bin/env node

// Campaign-gate execution-proof orchestrator for the scanners-replacement
// cutover (plan §8.4 `b4` profile). Run manually by the
// orchestrator at plan gates — it is NOT a CI job. The durable enforcement is
// the in-suite structural rails (trybuild/privacy/seals plus the
// `cases::scanners_replacement` suite) that already run in normal CI; this
// script proves one clean-tree gate actually executed them.
//
// Execution-proof shape (CLAUDE.md "Verification Must Prove Execution"):
// every declared phase must run, select non-zero work, and record its child
// exit codes; a missing, failed, or zero-selection phase FAILS — never a
// silent skip. The run ends with one terminal input-bound summary (tip sha,
// profile, per-phase counts).

import { execFileSync, spawnSync } from "node:child_process";
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

// The canonical phase set of this profile. The terminal summary refuses to
// exist unless every one of these recorded non-zero selected work.
const B4_PHASES = [
  "inputs",
  "prerequisites",
  "extension-authority",
  "scanner-free-boundary",
  "ledger",
  "schema-cohort",
  "rust-guards",
];

// Focused structural/guard suites, run by EXACT name so a deleted or renamed
// guard surfaces as a zero-selection failure here (execution proof replaces
// the former source-grep guard inventory).
const B4_RUST_GUARD_COMMANDS = [
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_source_policy_gate",
      "--test",
      "main",
      "cases::scanners_replacement::scanners_replacement",
    ],
  ],
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_session",
      "--lib",
      "carrier_artifact_cohort::tests::persisted_carrier_cohort_has_frozen_eight_word_shape",
    ],
  ],
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_session",
      "--lib",
      "carrier_artifact_cohort::tests::every_cohort_field_uses_exact_equality",
    ],
  ],
  [
    "cargo",
    ["test", "-p", "verter_session", "--lib", "registered_file_structure_is_the_envelope_owner"],
  ],
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_lsp",
      "--lib",
      "registered_projection_preserves_duplicate_attributes_and_sealed_refs",
    ],
  ],
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_protocol",
      "--lib",
      "schema8_full_body_uses_tag_26_and_roundtrips_supported_contract",
    ],
  ],
  [
    "cargo",
    [
      "test",
      "-p",
      "verter_session",
      "--features",
      "compile-fail",
      "--test",
      "main",
      "is_not_public",
    ],
  ],
];

// Files later phases depend on. A missing prerequisite fails HERE with its
// name instead of surfacing as an unrelated read error inside a phase.
const B4_PREREQUISITES = [
  "scripts/manifests/scanners-replacement-capability-ledger.json",
  "schemas/scanners-replacement-v1.schema.json",
  "packages/vue-vscode/package.json",
  "packages/playground/scripts/generate-vue-language.ts",
  "packages/playground/src/editor/vueLanguage.ts",
];

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
  let selected = 0;
  // PHYSICAL arm: no retired extension tree may exist on disk.
  for (const retired of RETIRED_EXTENSIONS) {
    invariant(!existsSync(join(root, retired)), `${retired} still exists`);
    selected += 1;
  }
  // TRACKED arm (retired): no retired extension tree may have tracked files.
  for (const retired of RETIRED_EXTENSIONS) {
    invariant(
      !files.some((path) => path === retired || path.startsWith(`${retired}/`)),
      `${retired} still has tracked files`,
    );
    selected += 1;
  }
  // TRACKED arm (allowlist): every tracked path under extensions/ must belong
  // to a live extension.
  for (const path of files.filter((candidate) => candidate.startsWith("extensions/"))) {
    invariant(
      LIVE_EXTENSIONS.some((live) => path.startsWith(`${live}/`)),
      `tracked extensions residue outside the live allowlist: ${path}`,
    );
    selected += 1;
  }

  for (const path of files.filter((candidate) => candidate.endsWith("package.json"))) {
    const source = readFileSync(join(root, path), "utf8").replaceAll("\\", "/");
    for (const retired of RETIRED_EXTENSIONS) {
      invariant(!source.includes(retired), `${path} references ${retired}`);
    }
    selected += 1;
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
    selected += 1;
  }
  return selected;
}

export function verifyScannerFreeBoundary(root = DEFAULT_ROOT, files = trackedFiles(root)) {
  let selected = 0;
  for (const retired of [
    "crates/verter_lsp/src/documents/sfc_scanner.rs",
    "crates/verter_parser/src/cursor/script_detector.rs",
    "packages/vue-vscode/src/css/styleBlockScanner.ts",
  ]) {
    invariant(!files.includes(retired), `${retired} was reintroduced`);
    selected += 1;
  }
  for (const fixture of [
    "crates/verter_session/tests/cases/compile-fail/scanners_replacement_raw_parser_public.rs",
    "crates/verter_session/tests/cases/compile-fail/scanners_replacement_script_detector_public.rs",
  ]) {
    invariant(existsSync(join(root, fixture)), `${fixture} compile boundary is missing`);
    selected += 1;
  }
  return selected;
}

export function verifyLedger(root = DEFAULT_ROOT) {
  const ledger = readJson(root, "scripts/manifests/scanners-replacement-capability-ledger.json");
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
  const openResiduals = ledger.set_equality.open_residual_migrate_rows;
  invariant(
    Number.isInteger(openResiduals) && openResiduals >= 0,
    "open_residual_migrate_rows must be a recorded count",
  );
  const b52Status = ledger.set_equality.b52_b91_status;
  invariant(typeof b52Status === "string", "b52_b91_status must be recorded");
  if (openResiduals === 0) {
    invariant(
      b52Status.includes("CLOSED") &&
        !b52Status.includes("REOPENED") &&
        b52Status.includes("DISCOVERY-RECEIPT.md"),
      "an empty residual set closes B-52/B-91 citing the input-bound receipt",
    );
  } else {
    invariant(
      b52Status.includes("REOPENED"),
      "B-52/B-91 must remain explicitly reopened while any named residual is open",
    );
  }
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
  return rows.length;
}

export function verifySchemaCohort(root = DEFAULT_ROOT) {
  const schema = readJson(root, "schemas/scanners-replacement-v1.schema.json");
  const cohort = new Set(Object.keys(schema.persisted_carrier_artifact_cohort?.fields ?? {}));
  invariant(
    equalSets(cohort, COHORT_FIELDS),
    "persisted carrier cohort is not the exact eight-field set",
  );
  return cohort.size;
}

export function verifyPrerequisites(root = DEFAULT_ROOT) {
  let selected = 0;
  for (const path of B4_PREREQUISITES) {
    invariant(existsSync(join(root, path)), `prerequisite missing: ${path}`);
    selected += 1;
  }
  return selected;
}

// Real child runner: captures exit code and output (echoed for the operator)
// instead of inheriting stdio, so selection can be PROVEN from the child's own
// test summary rather than assumed from exit 0.
function realExec(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: process.env,
    maxBuffer: 64 * 1024 * 1024,
  });
  invariant(!result.error, `failed to launch ${command}: ${result.error?.message}`);
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  return { status: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

// libtest prints one "test result: ok. N passed; M failed; ..." line per binary
// and, critically, EXITS 0 even when a name filter matches nothing (N == 0). A
// stale `-p`/module-path filter therefore "passes" a naive `status === 0` check
// while having run NOTHING — the exact zero-selection-reads-as-pass shape this
// verifier exists to catch (CLAUDE.md "Verification Must Prove Execution").
// `summarizeTestRun` is the ONE place that turns raw stdout into a pass/fail
// verdict for a guard command, so every caller gets the same zero-selection
// floor instead of re-deriving (and potentially forgetting) it per call site.
function summarizeTestRun(stdout) {
  let passed = 0;
  let sawResultLine = false;
  for (const match of stdout.matchAll(/test result: \w+\. (\d+) passed;/g)) {
    sawResultLine = true;
    passed += Number(match[1]);
  }
  return { passed, sawResultLine };
}

export function runFocusedRustGuards(root = DEFAULT_ROOT, exec = realExec) {
  const receipts = [];
  for (const [command, args] of B4_RUST_GUARD_COMMANDS) {
    const shown = `${command} ${args.join(" ")}`;
    console.log(`\n[verify] ${shown}`);
    const { status, stdout } = exec(command, args, root);
    invariant(status === 0, `rust guard failed (exit ${status}): ${shown}`);
    const { passed, sawResultLine } = summarizeTestRun(stdout);
    invariant(
      sawResultLine,
      `rust guard produced no parseable "test result:" summary (cannot prove it ran): ${shown}`,
    );
    invariant(
      passed > 0,
      `rust guard ZERO-SELECTION: filter matched no tests, which libtest reports as exit 0 — ` +
        `this would silently read as a pass without this check: ${shown}`,
    );
    receipts.push({ command: shown, exit_code: status, tests_passed: passed });
  }
  return receipts;
}

// The terminal input-bound summary. It refuses to exist unless bound to a tip
// sha and complete over every canonical phase with non-zero selected work — a
// run that cannot produce it never reports success.
export function terminalSummary({ profile, tip, phases, rust_commands = [] }) {
  invariant(profile === "b4", `unknown profile ${profile}`);
  invariant(/^[0-9a-f]{40}$/.test(tip ?? ""), "terminal summary must bind the tip sha");
  for (const name of B4_PHASES) {
    const entry = phases[name];
    invariant(
      entry && Number.isInteger(entry.selected) && entry.selected > 0,
      `terminal summary missing phase ${name} (a silent skip or zero work)`,
    );
  }
  return {
    profile,
    tip,
    phases: Object.fromEntries(B4_PHASES.map((name) => [name, phases[name]])),
    rust_commands,
  };
}

export function runProfileB4(root = DEFAULT_ROOT, exec = realExec) {
  const phases = {};
  const record = (name, work) => {
    console.log(`\n[verify] phase: ${name}`);
    const selected = work();
    invariant(Number.isInteger(selected) && selected > 0, `phase ${name} ran zero work`);
    phases[name] = { selected };
  };

  const tip = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  const files = trackedFiles(root);
  let rustReceipts = [];

  record("inputs", () => {
    invariant(/^[0-9a-f]{40}$/.test(tip), `unusable tip sha: ${tip}`);
    invariant(files.length > 0, "tracked-file inventory is empty");
    // Inputs bound: tip sha + tracked inventory + the toolchain identities.
    const node = process.version;
    const cargo = exec("cargo", ["--version"], root);
    invariant(cargo.status === 0 && cargo.stdout.includes("cargo"), "cargo identity unavailable");
    console.log(`[verify] tip=${tip} files=${files.length} node=${node} ${cargo.stdout.trim()}`);
    return 2 + files.length;
  });
  record("prerequisites", () => verifyPrerequisites(root));
  record("extension-authority", () => verifyNoLegacyExtensionAuthority(root, files));
  record("scanner-free-boundary", () => verifyScannerFreeBoundary(root, files));
  record("ledger", () => verifyLedger(root));
  record("schema-cohort", () => verifySchemaCohort(root));
  record("rust-guards", () => {
    rustReceipts = runFocusedRustGuards(root, exec);
    return rustReceipts.reduce((total, receipt) => total + receipt.tests_passed, 0);
  });

  const summary = terminalSummary({ profile: "b4", tip, phases, rust_commands: rustReceipts });
  console.log(`\n[verify] SUMMARY ${JSON.stringify(summary)}`);
  console.log("[verify] scanners replacement b4 orchestration passed");
  return summary;
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
    runProfileB4();
  } catch (error) {
    console.error(`[verify] FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
