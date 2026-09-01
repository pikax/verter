#!/usr/bin/env node
// Negative controls for the canonical gate's wasm JavaScript-boundary lane.
//
// The lane exists because `#[wasm_bindgen_test]` cases can be compiled forever without ever being
// executed: they are `#[cfg(target_arch = "wasm32")]`, so the host-target archive cannot contain them and
// no host test run can observe them. A lane that reports success while executing nothing would reproduce
// that defect one layer up, so every scenario here drives a REAL decision function from
// `gate-internals.mjs` and proves it FAILS in a named direction — a missing runner, a missing target, a
// runner/library ABI skew, an empty scope, an empty inventory, a failing case, a truncated transcript, and
// an executed-vs-declared count that does not reconcile.
//
// Pure and hermetic: no cargo, no wasm toolchain, no process spawning. The fixtures stand in for the
// probes, so the same discriminations run identically on a machine with no wasm target installed.

import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import {
  WASM_LANE_ID,
  WASM_LANE_TARGET,
  WASM_LANE_PREREQUISITE_MARKER,
  checkWasmLanePrerequisites,
  countWasmBindgenTestAttributesInDir,
  discoverWasmBoundaryPackages,
  deriveWasmBindgenPin,
  parseWasmBindgenRunnerVersion,
  decideWasmLaneRunnerPin,
  decideWasmTargetInstalled,
  buildWasmLaneTestArgs,
  parseWasmLaneHarnessSummary,
  decideWasmLaneCaseParity,
  decideWasmLanePackageCaseParity,
  evaluateWasmLanePackageRun,
  reduceGateLaneReceipts,
  deriveGateLaneLayout,
  extractWasmLaneFailedNames,
} from "./gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(SCRIPT_DIR, "..");
const GATE = join(SCRIPT_DIR, "gate.mjs");

const TEMP_DIRS = [];
function tempTree() {
  const dir = mkdtempSync(join(tmpdir(), "verter-wasm-lane-"));
  TEMP_DIRS.push(dir);
  return dir;
}
process.on("exit", () => {
  for (const dir of TEMP_DIRS) {
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      /* best effort */
    }
  }
});

// A metadata document shaped exactly like `cargo metadata --format-version 1 --no-deps`, parameterised so
// each scenario perturbs ONE field.
function metadataFixture({
  name = "fixture_wasm",
  manifestDir = "/repo/crates/fixture_wasm",
  bindgenReq = "^0.2.122",
  testDep = true,
  targets = null,
} = {}) {
  return {
    packages: [
      {
        name: "unrelated_host_crate",
        manifest_path: "/repo/crates/unrelated/Cargo.toml",
        dependencies: [{ name: "serde", kind: null, req: "^1" }],
        targets: [{ kind: ["lib"], src_path: "/repo/crates/unrelated/src/lib.rs", test: true }],
      },
      {
        name,
        manifest_path: join(manifestDir, "Cargo.toml"),
        dependencies: [
          ...(bindgenReq === null ? [] : [{ name: "wasm-bindgen", kind: null, req: bindgenReq }]),
          ...(testDep ? [{ name: "wasm-bindgen-test", kind: "dev", req: "^0.3.72" }] : []),
        ],
        targets: targets || [
          { kind: ["cdylib", "rlib"], src_path: join(manifestDir, "src", "lib.rs"), test: true },
          { kind: ["test"], src_path: join(manifestDir, "tests", "main.rs"), test: true },
          // A bench target cargo does NOT run under `cargo test` — its directory must not become a scan
          // root, or the declared inventory would exceed what the lane can ever execute.
          { kind: ["bench"], src_path: join(manifestDir, "benches", "bench.rs"), test: false },
        ],
      },
    ],
  };
}

const okCapture = (stdout) => ({ ok: true, stdout, detail: "" });

// The default probe set: a healthy toolchain. Each scenario replaces exactly one answer.
function probes({
  metadata = metadataFixture(),
  libdir = "/installed/wasm32-unknown-unknown/lib",
  runnerVersionLine = "wasm-bindgen-test-runner 0.2.122",
  metadataFails = null,
} = {}) {
  return (command, args) => {
    if (args[0] === "metadata") {
      return metadataFails || okCapture(JSON.stringify(metadata));
    }
    if (args[0] === "--print") return okCapture(`${libdir}\n`);
    if (args[0] === "--version") return okCapture(`${runnerVersionLine}\n`);
    throw new Error(`unexpected probe: ${command} ${args.join(" ")}`);
  };
}

const HEALTHY = {
  repoRoot: "/repo",
  env: { PATH: "/usr/bin" },
  capture: probes(),
  resolveRunner: () => "/tools/bin/wasm-bindgen-test-runner",
  // Per SCAN ROOT, exactly as the production check calls it: the `src` root declares the cases, the
  // `tests` root declares none. Summed, the fixture tree declares 4.
  countCases: (root) => (root.endsWith("src") ? 4 : 0),
};

// `checkWasmLanePrerequisites` consults `decideWasmTargetInstalled` on the real filesystem, so the healthy
// fixture points at a real directory holding a real-looking core rlib.
function installedTargetLibdir() {
  const dir = tempTree();
  writeFileSync(join(dir, "libcore-0123456789abcdef.rlib"), "");
  return dir;
}

function healthy(overrides = {}) {
  const libdir = overrides.libdir || installedTargetLibdir();
  const { metadata, runnerVersionLine, metadataFails, ...rest } = overrides;
  delete rest.libdir;
  return {
    ...HEALTHY,
    capture: probes({
      libdir,
      ...(metadata === undefined ? {} : { metadata }),
      ...(runnerVersionLine === undefined ? {} : { runnerVersionLine }),
      ...(metadataFails === undefined ? {} : { metadataFails }),
    }),
    ...rest,
  };
}

function assertPrerequisiteFailure(result, ...mustMention) {
  assert.equal(result.ok, false, "the prerequisite check must FAIL, not degrade to a skip");
  const text = result.lines.join("\n");
  assert.ok(
    text.startsWith(WASM_LANE_PREREQUISITE_MARKER),
    `the failure must be marked with ${WASM_LANE_PREREQUISITE_MARKER}, got: ${text}`,
  );
  for (const needle of mustMention) {
    assert.ok(text.includes(needle), `failure text must name \`${needle}\`, got: ${text}`);
  }
}

// ---------------------------------------------------------------------------------------------------
// POSITIVE CONTROL. Without it a check that refuses everything would read as proof.
// ---------------------------------------------------------------------------------------------------
test("a healthy toolchain satisfies the prerequisite and pins the runner from the tree", () => {
  const result = checkWasmLanePrerequisites(healthy());
  assert.equal(result.ok, true);
  assert.deepEqual(
    result.packages.map((pkg) => pkg.name),
    ["fixture_wasm"],
    "only packages that dev-depend on wasm-bindgen-test are in scope",
  );
  assert.equal(result.expectedVersion, "0.2.122", "the pin is READ from the tree, never hardcoded");
  assert.equal(result.discoveredCases, 4);
  assert.deepEqual(
    result.packages[0].sourceRoots,
    ["/repo/crates/fixture_wasm/src", "/repo/crates/fixture_wasm/tests"],
    "scan roots come from cargo's own `test = true` targets — the bench directory is not one",
  );
});

// Native separators: production joins scan roots with `sep`, so a fixture written with literal
// forward slashes would make the prune a no-op on Windows and fail these assertions there.
const FIXTURE_PKG_DIR = join("/repo", "crates", "fixture_wasm");
const FIXTURE_SRC = join(FIXTURE_PKG_DIR, "src");
const FIXTURE_SRC_GEN = join(FIXTURE_PKG_DIR, "src-gen");

test("a scan root nested inside another is not counted twice", () => {
  // The roots are deduped by exact string, but the scanner walks them RECURSIVELY. A package with targets
  // at `src/lib.rs` and `src/bin/tool.rs` yields both `src` and `src/bin`, and every attribute under the
  // nested one is then counted twice — inflating the declared inventory against a correct executed count,
  // so the lane fails loudly on a healthy tree.
  const nested = checkWasmLanePrerequisites(
    healthy({
      metadata: metadataFixture({
        targets: [
          { kind: ["lib"], src_path: join(FIXTURE_SRC, "lib.rs"), test: true },
          { kind: ["bin"], src_path: join(FIXTURE_SRC, "bin", "tool.rs"), test: true },
        ],
      }),
    }),
  );
  assert.equal(nested.ok, true);
  assert.deepEqual(
    nested.packages[0].sourceRoots,
    [FIXTURE_SRC],
    "the nested root is pruned — the surviving root already covers it recursively",
  );

  // A sibling that merely shares a name PREFIX is a different directory and must survive.
  const siblings = checkWasmLanePrerequisites(
    healthy({
      metadata: metadataFixture({
        targets: [
          { kind: ["lib"], src_path: join(FIXTURE_SRC, "lib.rs"), test: true },
          { kind: ["test"], src_path: join(FIXTURE_SRC_GEN, "main.rs"), test: true },
        ],
      }),
    }),
  );
  assert.deepEqual(siblings.packages[0].sourceRoots, [FIXTURE_SRC, FIXTURE_SRC_GEN]);
});

test("the pin follows the tree rather than a constant", () => {
  const bumped = checkWasmLanePrerequisites(
    healthy({
      metadata: metadataFixture({ bindgenReq: "^0.2.200" }),
      runnerVersionLine: "wasm-bindgen-test-runner 0.2.200",
    }),
  );
  assert.equal(bumped.ok, true);
  assert.equal(bumped.expectedVersion, "0.2.200");
});

// ---------------------------------------------------------------------------------------------------
// NEGATIVE CONTROLS — each must FAIL, and must name the exact missing prerequisite.
// ---------------------------------------------------------------------------------------------------
test("a missing runner FAILS loudly and names the tool plus the tree-derived install command", () => {
  const result = checkWasmLanePrerequisites(healthy({ resolveRunner: () => null }));
  assertPrerequisiteFailure(
    result,
    "wasm-bindgen-test-runner",
    "cargo install wasm-bindgen-cli --version 0.2.122",
  );
});

test("a missing wasm target FAILS loudly and names `rustup target add`", () => {
  const missing = join(tempTree(), "not-installed");
  const result = checkWasmLanePrerequisites(healthy({ libdir: missing }));
  assertPrerequisiteFailure(result, WASM_LANE_TARGET, `rustup target add ${WASM_LANE_TARGET}`);
});

test("a recognised-but-uninstalled target directory is not mistaken for an installed one", () => {
  const empty = tempTree();
  assert.equal(
    decideWasmTargetInstalled(empty),
    false,
    "`rustc --print target-libdir` prints a path for any recognised triple; the path alone proves nothing",
  );
  assert.equal(decideWasmTargetInstalled(installedTargetLibdir()), true);
  assert.equal(decideWasmTargetInstalled(""), false);
});

test("a runner whose version differs from the tree's wasm-bindgen FAILS as an ABI skew", () => {
  const result = checkWasmLanePrerequisites(
    healthy({ runnerVersionLine: "wasm-bindgen-test-runner 0.2.100" }),
  );
  assertPrerequisiteFailure(result, "0.2.100", "0.2.122", "ONE ABI");
});

test("an unparseable runner version banner FAILS rather than being assumed compatible", () => {
  const result = checkWasmLanePrerequisites(
    healthy({ runnerVersionLine: "some other tool 9.9.9" }),
  );
  assertPrerequisiteFailure(result, "no recognisable version line");
  assert.equal(parseWasmBindgenRunnerVersion("some other tool 9.9.9"), null);
  assert.equal(parseWasmBindgenRunnerVersion("wasm-bindgen-test-runner 0.2.122"), "0.2.122");
  assert.equal(
    decideWasmLaneRunnerPin({ runnerVersion: "0.2.122", expectedVersion: "0.2.122" }),
    null,
  );
});

test("an empty lane scope FAILS instead of reporting a vacuous green lane", () => {
  const result = checkWasmLanePrerequisites(
    healthy({ metadata: metadataFixture({ testDep: false }) }),
  );
  assertPrerequisiteFailure(result, "would execute NOTHING");
});

test("zero discovered cases FAILS", () => {
  const result = checkWasmLanePrerequisites(healthy({ countCases: () => 0 }));
  assertPrerequisiteFailure(result, "ZERO `#[wasm_bindgen_test]` attributes");
});

test("a scope member with no wasm-bindgen requirement FAILS rather than running unpinned", () => {
  const result = checkWasmLanePrerequisites(
    healthy({ metadata: metadataFixture({ bindgenReq: null }) }),
  );
  assertPrerequisiteFailure(result, "nothing to pin the test runner's version against");
});

test("a wasm-bindgen requirement that is a range cannot pin one runner binary", () => {
  const ranged = deriveWasmBindgenPin([{ name: "p", bindgenReqs: [">=0.2, <0.3"] }]);
  assert.equal(ranged.version, null);
  assert.match(ranged.error, /not a single exact version/);
  const conflicting = deriveWasmBindgenPin([
    { name: "a", bindgenReqs: ["^0.2.122"] },
    { name: "b", bindgenReqs: ["^0.2.100"] },
  ]);
  assert.equal(conflicting.version, null);
  assert.match(conflicting.error, /different `wasm-bindgen` versions/);
  assert.equal(deriveWasmBindgenPin([{ name: "a", bindgenReqs: ["=0.2.122"] }]).version, "0.2.122");
  assert.equal(deriveWasmBindgenPin([{ name: "a", bindgenReqs: ["0.2.122"] }]).version, "0.2.122");
});

test("an unusable `cargo metadata` FAILS rather than silently emptying the scope", () => {
  const failed = checkWasmLanePrerequisites(
    healthy({ metadataFails: { ok: false, stdout: "", detail: "cargo not found" } }),
  );
  assertPrerequisiteFailure(failed, "cargo not found");
  const garbage = checkWasmLanePrerequisites(
    healthy({ metadataFails: { ok: true, stdout: "<not json>", detail: "" } }),
  );
  assertPrerequisiteFailure(garbage, "not parseable JSON");
  assert.match(discoverWasmBoundaryPackages({}).error, /no `packages` array/);
});

// ---------------------------------------------------------------------------------------------------
// DISCOVERY IS TREE-DERIVED, NOT A FILENAME LIST.
// ---------------------------------------------------------------------------------------------------
test("a case in a file the lane has never seen is counted without editing any list", () => {
  const root = tempTree();
  mkdirSync(join(root, "nested", "deeper"), { recursive: true });
  writeFileSync(
    join(root, "existing.rs"),
    '#[wasm_bindgen_test]\nfn a() {}\n#[cfg(target_arch = "wasm32")]\n#[wasm_bindgen_test::wasm_bindgen_test]\nfn b() {}\n',
  );
  assert.equal(
    countWasmBindgenTestAttributesInDir(root),
    2,
    "both spellings of the attribute count",
  );

  writeFileSync(
    join(root, "nested", "deeper", "brand_new_file.rs"),
    "#[wasm_bindgen_test(unsupported_cfg)]\nasync fn c() {}\n",
  );
  assert.equal(
    countWasmBindgenTestAttributesInDir(root),
    3,
    "a NEW file at a NEW depth enters the inventory with no edit here",
  );

  writeFileSync(join(root, "decoy.rs"), "// #[wasm_bindgen_test]\n#[test]\nfn host_only() {}\n");
  assert.equal(
    countWasmBindgenTestAttributesInDir(root),
    3,
    "a plain #[test] and a commented-out attribute are not wasm boundary cases",
  );
  assert.equal(countWasmBindgenTestAttributesInDir(join(root, "absent")), 0);
});

test("a newly added package enters the scope from cargo metadata alone", () => {
  const base = metadataFixture();
  const withSecond = {
    packages: [
      ...base.packages,
      {
        name: "another_wasm_crate",
        manifest_path: "/repo/crates/another/Cargo.toml",
        dependencies: [
          { name: "wasm-bindgen", kind: null, req: "^0.2.122" },
          { name: "wasm-bindgen-test", kind: "dev", req: "^0.3.72" },
        ],
        targets: [{ kind: ["lib"], src_path: "/repo/crates/another/src/lib.rs", test: true }],
      },
    ],
  };
  const discovered = discoverWasmBoundaryPackages(withSecond);
  assert.equal(discovered.error, null);
  assert.deepEqual(
    discovered.packages.map((pkg) => pkg.name),
    ["another_wasm_crate", "fixture_wasm"],
  );
});

test("the real repository tree yields a non-empty inventory through the production scanner", () => {
  // The repository's own boundary sources, scanned with the production regex through the production
  // walker. A change that made the scan vacuous — the failure mode that would silently disarm the lane —
  // reddens here without needing a wasm toolchain installed.
  const real = countWasmBindgenTestAttributesInDir(join(REPO_ROOT, "crates", "verter_wasm", "src"));
  assert.ok(real > 0, "the tracked wasm boundary sources must declare at least one case");
});

// ---------------------------------------------------------------------------------------------------
// TRANSCRIPT PARSING AND THE EXECUTED-VS-DECLARED VERDICT.
// ---------------------------------------------------------------------------------------------------
const GREEN_TRANSCRIPT =
  "    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.04s\n" +
  "     Running unittests src/lib.rs (target/wasm32-unknown-unknown/debug/deps/verter_wasm-d0f.wasm)\n" +
  "running 4 tests\n" +
  "test host_compile_request_tests::js_boundary::the_other_frameworks_option_on_a_js_payload_is_refused ... ok\n" +
  "test host_compile_request_tests::js_boundary::an_unknown_key_on_a_js_payload_is_refused ... ok\n" +
  "test host_compile_request_tests::js_boundary::a_valid_js_payload_reaches_the_canonical_request ... ok\n" +
  "test tests::public_api_js_serialization_uses_explicit_null_fields ... ok\n" +
  "\n" +
  "test result: ok. 4 passed; 0 failed; 0 ignored; 0 filtered out; finished in 0.02s\n" +
  "\n" +
  "     Running tests/main.rs (target/wasm32-unknown-unknown/debug/deps/main-104.wasm)\n" +
  "no tests to run!\n";

test("a complete green transcript reconciles with the declared inventory", () => {
  const summary = parseWasmLaneHarnessSummary(GREEN_TRANSCRIPT);
  assert.equal(summary.selected, 4);
  assert.equal(summary.passed, 4);
  assert.equal(summary.failed, 0);
  assert.equal(summary.ok, true);
  assert.equal(summary.complete, true);
  assert.equal(
    summary.emptyBinaries,
    1,
    "a binary with no cases is observed, not silently dropped",
  );
  assert.equal(
    decideWasmLaneCaseParity(summary.selected, 4, summary.passed + summary.failed),
    null,
  );
});

test("cases the harness SELECTED but never executed are a failure, not full coverage", () => {
  // The defect this whole lane exists to remove, one layer up. `#[ignore]` on a boundary case costs two
  // tokens and no gate edit: the harness prints `running N tests` BEFORE applying its ignore filter, so
  // SELECTED still reconciles with the source scan (which counts the `#[wasm_bindgen_test]` attribute
  // either way, `#[ignore]` being a separate line) while the case runs no assertion at all. Comparing
  // only SELECTED would report a complete, green, entirely vacuous lane.
  const allIgnored = GREEN_TRANSCRIPT.replace(
    "test result: ok. 4 passed; 0 failed; 0 ignored;",
    "test result: ok. 0 passed; 0 failed; 4 ignored;",
  ).replace(/^test (.+) \.\.\. ok$/gm, "test $1 ... ignored");
  const summary = parseWasmLaneHarnessSummary(allIgnored);
  assert.equal(summary.selected, 4, "the announcement is unchanged by the ignore filter");
  assert.equal(summary.ignored, 4);
  assert.equal(summary.ok, true, "the harness itself calls an all-ignored run `ok`");
  assert.equal(
    summary.complete,
    true,
    "and it CLOSED its announcement — the transcript is complete",
  );

  const verdict = decideWasmLaneCaseParity(4, 4, summary.passed + summary.failed);
  assert.ok(verdict, "0 executed against 4 declared must never reconcile");
  assert.match(verdict.message, /\b0 of them EXECUTED; 4 were skipped/);
  assert.match(verdict.message, /never `#\[ignore\]` it/);

  // One ignored case out of four is the same failure — the boundary is per case, not "did anything run".
  const partial = decideWasmLaneCaseParity(4, 4, 3);
  assert.ok(partial, "a single skipped boundary case is still an unproven refusal");
  assert.match(partial.message, /1 were skipped/);

  // A count the check cannot read is fail-closed, and says so rather than pretending to compare.
  const missing = decideWasmLaneCaseParity(4, 4, undefined);
  assert.ok(missing, "an absent executed count is a failure, never an assumed pass");
  assert.match(missing.message, /no executed-case count/);
});

test("the per-package verdict catches drift a global sum cancels out", () => {
  // The lane's scope comes from `cargo metadata`, so a second package enters it with no gate edit. Summing
  // first hides exactly the disagreement worth catching: one package short by five and another over by
  // five reconcile perfectly in the total while neither package does.
  assert.equal(decideWasmLaneCaseParity(10, 10, 10), null, "the totals reconcile...");
  const short = decideWasmLanePackageCaseParity({
    packageName: "alpha",
    selectedCount: 0,
    discoveredCount: 5,
    executedCount: 0,
  });
  assert.ok(short, "...while `alpha` ran none of the five cases it declares");
  assert.match(short.message, /`alpha`/);
  const over = decideWasmLanePackageCaseParity({
    packageName: "beta",
    selectedCount: 10,
    discoveredCount: 5,
    executedCount: 10,
  });
  assert.ok(over, "...and `beta` ran five the scan never saw");
  assert.match(over.message, /`beta`/);

  // A lane package whose sources declare no cases is legitimate — "the lane proves nothing" is the
  // whole-run check's question, not this one's. Running cases the scan cannot see is not.
  assert.equal(
    decideWasmLanePackageCaseParity({
      packageName: "quiet",
      selectedCount: 0,
      discoveredCount: 0,
      executedCount: 0,
    }),
    null,
  );
  const unseen = decideWasmLanePackageCaseParity({
    packageName: "quiet",
    selectedCount: 3,
    discoveredCount: 0,
    executedCount: 3,
  });
  assert.ok(unseen, "a package that runs cases its scan roots do not contain is untrusted");
  assert.match(unseen.message, /found NO/);
});

test("one package's transcript reduces to the receipt fields the lane records", () => {
  const green = evaluateWasmLanePackageRun({
    packageName: "verter_wasm",
    expected: 4,
    text: GREEN_TRANSCRIPT,
    exitCode: 0,
  });
  assert.deepEqual(green.failures, []);
  assert.equal(green.parseable, true);
  assert.equal(green.complete, true);
  assert.equal(green.parityMismatch, false);

  // A wasm32 COMPILE BREAK — the most likely real failure of this lane — announces nothing at all. It must
  // be named as what it is; reporting it as an inventory mismatch points at the wrong cause entirely.
  const compileBreak = evaluateWasmLanePackageRun({
    packageName: "verter_wasm",
    expected: 4,
    text: "error[E0433]: failed to resolve: use of undeclared crate\nerror: could not compile\n",
    exitCode: 101,
  });
  assert.ok(
    compileBreak.failures.some((f) => /cargo exited 101/.test(f.name)),
    "the compile diagnostic must survive as a named failure",
  );
  assert.equal(compileBreak.complete, false);
  assert.equal(compileBreak.parityMismatch, true);
  assert.ok(compileBreak.failures.every((f) => f.surface === "wasm:verter_wasm"));

  // Ignored cases reach the receipt through the same per-package route, not only the totals.
  const ignored = evaluateWasmLanePackageRun({
    packageName: "verter_wasm",
    expected: 4,
    text: GREEN_TRANSCRIPT.replace(
      "test result: ok. 4 passed; 0 failed; 0 ignored;",
      "test result: ok. 0 passed; 0 failed; 4 ignored;",
    ),
    exitCode: 0,
  });
  assert.equal(ignored.parityMismatch, true);
  assert.ok(ignored.failures.some((f) => /\b0 of them EXECUTED; \d+ were skipped/.test(f.name)));

  // A package whose sources declare nothing legitimately announces nothing and stays clean.
  const quiet = evaluateWasmLanePackageRun({
    packageName: "quiet",
    expected: 0,
    text: "     Running tests/main.rs (target/wasm32-unknown-unknown/debug/deps/main-104.wasm)\nno tests to run!\n",
    exitCode: 0,
  });
  assert.deepEqual(quiet.failures, []);
  assert.equal(quiet.complete, true);
});

test("a failing boundary case is named and reddens the lane", () => {
  // The wasm harness spells a failing case `... FAIL`, NOT libtest's `... FAILED`. Reading it with the
  // libtest extractor names ZERO cases on a genuinely red run, and the lane then reports "no terminal
  // result" for a run that produced one and failed in it. The spelling here is transcribed from a real
  // red run of the boundary probes, not assumed.
  const red = GREEN_TRANSCRIPT.replace(
    "test host_compile_request_tests::js_boundary::an_unknown_key_on_a_js_payload_is_refused ... ok",
    "test host_compile_request_tests::js_boundary::an_unknown_key_on_a_js_payload_is_refused ... FAIL",
  ).replace("test result: ok. 4 passed; 0 failed;", "test result: FAILED. 3 passed; 1 failed;");
  const summary = parseWasmLaneHarnessSummary(red);
  assert.equal(summary.failed, 1);
  assert.equal(summary.ok, false, "a FAILED harness result is never `ok`");
  assert.equal(
    summary.complete,
    true,
    "a red run still CLOSED its announcement — failing is not the same receipt as unfinished",
  );
  assert.deepEqual(extractWasmLaneFailedNames(red), [
    "host_compile_request_tests::js_boundary::an_unknown_key_on_a_js_payload_is_refused",
  ]);
  assert.deepEqual(
    extractWasmLaneFailedNames("test some::case ... FAILED\ntest other::case ... ok\n"),
    ["some::case"],
    "libtest's spelling is accepted too, so a harness that adopts it keeps naming cases",
  );
  assert.deepEqual(extractWasmLaneFailedNames(GREEN_TRANSCRIPT), []);
});

test("a transcript that announced work and never closed it is not a pass", () => {
  const truncated = GREEN_TRANSCRIPT.slice(0, GREEN_TRANSCRIPT.indexOf("test result:"));
  const summary = parseWasmLaneHarnessSummary(truncated);
  assert.equal(summary.announced, 1);
  assert.equal(summary.resultBlocks, 0);
  assert.equal(summary.complete, false, "an absent terminal result must never read as complete");

  const empty = parseWasmLaneHarnessSummary("");
  assert.equal(empty.complete, false);
  assert.equal(empty.selected, 0);
});

test("selecting fewer cases than the tree declares is a setup FAILURE, not a pass", () => {
  const short = decideWasmLaneCaseParity(3, 4, 3);
  assert.ok(short, "3 selected against 4 declared must not reconcile");
  assert.match(short.message, /selected 3 case\(s\)/);
  assert.match(short.message, /found 4/);

  const superset = decideWasmLaneCaseParity(5, 4, 5);
  assert.ok(superset, "a superset is equally untrusted — the scan missed a compiled source");

  const vacuous = decideWasmLaneCaseParity(0, 0, 0);
  assert.ok(vacuous, "zero declared cases can never be a pass");
  assert.match(vacuous.message, /ZERO/);
});

// ---------------------------------------------------------------------------------------------------
// THE VERDICT REDUCER CANNOT REACH PASS WITHOUT THE LANE.
// ---------------------------------------------------------------------------------------------------
const completeSurface = {
  hardFailure: false,
  failures: [],
  toleratedOccurred: false,
  coverage: { parseable: true, complete: true },
};
const completeWasm = {
  laneId: WASM_LANE_ID,
  hardFailure: false,
  failures: [],
  coverage: { parseable: true, complete: true },
  parity: { complete: true, matches: true, discoveredCases: 4, selectedCases: 4 },
};

test("the verdict reducer requires a complete wasm receipt in every direction", () => {
  const green = reduceGateLaneReceipts({
    surface: completeSurface,
    shipped: null,
    wasm: completeWasm,
    shippedCfgLaneEnabled: false,
  });
  assert.equal(green.verdict, "PASS", "the positive control must actually PASS");

  const rows = [
    ["no wasm receipt at all", null],
    [
      "a lane that never produced a parseable receipt",
      { ...completeWasm, coverage: { parseable: false, complete: false } },
    ],
    [
      "a lane whose harness never closed its run",
      { ...completeWasm, coverage: { parseable: true, complete: false } },
    ],
    [
      "a lane whose executed count does not reconcile",
      {
        ...completeWasm,
        parity: { complete: true, matches: false, discoveredCases: 4, selectedCases: 0 },
      },
    ],
    [
      "a lane that never reached its parity decision",
      {
        ...completeWasm,
        parity: { complete: false, matches: false, discoveredCases: 4, selectedCases: 0 },
      },
    ],
  ];
  for (const [label, wasm] of rows) {
    const decision = reduceGateLaneReceipts({
      surface: completeSurface,
      shipped: null,
      wasm,
      shippedCfgLaneEnabled: false,
    });
    assert.equal(decision.verdict, "FAIL", `${label} must FAIL`);
    assert.equal(decision.coverageComplete, false, `${label} must not read as complete coverage`);
    assert.ok(
      decision.failures.some((row) => row.surface === "gate/incomplete"),
      `${label} must be reported as incomplete required coverage`,
    );
  }
});

test("a failing wasm case is attributed to the lane and defeats a tolerated-only PASS", () => {
  const decision = reduceGateLaneReceipts({
    surface: { ...completeSurface, toleratedOccurred: true },
    shipped: null,
    wasm: {
      ...completeWasm,
      hardFailure: true,
      failures: [{ surface: "wasm:verter_wasm", name: "js_boundary::an_unknown_key_is_refused" }],
    },
    shippedCfgLaneEnabled: false,
  });
  assert.equal(decision.verdict, "FAIL");
  assert.deepEqual(
    decision.failures.map((row) => row.surface),
    [`${WASM_LANE_ID}/wasm:verter_wasm`],
    "a wasm failure is namespaced to its lane, never merged into Surface 1's",
  );
});

test("a lane infrastructure abort propagates its exit code, never a verdict", () => {
  const decision = reduceGateLaneReceipts({
    surface: completeSurface,
    shipped: null,
    wasm: { ...completeWasm, exitCode: 124 },
    shippedCfgLaneEnabled: false,
  });
  assert.equal(decision.verdict, null);
  assert.equal(decision.exitCode, 124);
  assert.equal(decision.coverageDisposition, "aborted");
});

// ---------------------------------------------------------------------------------------------------
// LAYOUT AND COMMAND CONSTRUCTION.
// ---------------------------------------------------------------------------------------------------
test("the lane owns a mutable root disjoint from every other lane", () => {
  const runnerTarget = join(tempTree(), "target");
  const layout = deriveGateLaneLayout(runnerTarget, join(runnerTarget, "gate-work"));
  const roots = [
    layout.surface1.targetDir,
    layout.surface1.extractDir,
    layout.shippedCfg.targetDir,
    layout.wasmJsBoundary.targetDir,
    layout.wasmJsBoundary.workDir,
    layout.wasmJsBoundary.outputFile,
  ];
  assert.equal(new Set(roots).size, roots.length);
  assert.equal(layout.wasmJsBoundary.laneId, WASM_LANE_ID);
  assert.ok(layout.wasmJsBoundary.targetDir.startsWith(runnerTarget));
});

test("the lane argv selects cargo's own testable targets and honours execution policy only", () => {
  const bare = buildWasmLaneTestArgs({ packageName: "verter_wasm" });
  assert.deepEqual(bare, ["test", "--target", WASM_LANE_TARGET, "-p", "verter_wasm", "--tests"]);
  const exhaustive = buildWasmLaneTestArgs({ packageName: "verter_wasm", exhaustive: true });
  assert.deepEqual(exhaustive, [...bare, "--no-fail-fast"]);
  assert.throws(() => buildWasmLaneTestArgs({}), TypeError);
});

// ---------------------------------------------------------------------------------------------------
// PRODUCTION WIRING. A bounded call-site check: the tested decisions must be the ones the real gate runs,
// on every real invocation, with no path filter and no enable flag.
// ---------------------------------------------------------------------------------------------------
test("the production gate wires the lane unconditionally into the canonical run", () => {
  const gateSource = readFileSync(GATE, "utf8");
  const runGateStart = gateSource.indexOf("async function runGate(opts, ctx)");
  const runGateEnd = gateSource.indexOf("\n}\n\nmain().catch", runGateStart);
  assert.ok(runGateStart >= 0 && runGateEnd > runGateStart);
  const runGateBody = gateSource.slice(runGateStart, runGateEnd);

  const preflightAt = runGateBody.indexOf("runWasmLanePrerequisitePreflight(ctx)");
  const archiveAt = runGateBody.indexOf("await archiveAndList(ctx)");
  const laneAt = runGateBody.indexOf("await runWasmJsBoundaryLane(opts, ctx,");
  const surfaceAt = runGateBody.indexOf("runSurface1Lane(opts, ctx,");
  const reduceAt = runGateBody.indexOf("reduceGateLaneReceipts(receipts)");

  assert.ok(preflightAt >= 0, "the prerequisite preflight must run inside runGate");
  assert.ok(
    preflightAt > archiveAt && preflightAt < laneAt,
    "prerequisites are established in the Cargo phase, immediately before the one lane that needs them — " +
      "the pre-archive preflights are deliberately node-only and must not acquire a toolchain dependency",
  );
  assert.ok(laneAt > archiveAt, "the lane runs after the front archive/list phase");
  assert.ok(
    laneAt < surfaceAt,
    "the lane's receipt exists before Surface 1's fail-fast can end the run",
  );
  assert.ok(reduceAt > laneAt, "one fixed-order reducer still owns the final verdict");
  assert.equal(
    (runGateBody.match(/runWasmJsBoundaryLane\(/g) || []).length,
    1,
    "exactly one lane invocation",
  );
  assert.equal(
    (runGateBody.match(/reduceGateLaneReceipts\(/g) || []).length,
    1,
    "the lane must not introduce a second verdict authority",
  );
  assert.equal(
    (runGateBody.match(/replayGateLaneTranscript\(/g) || []).length,
    1,
    "one canonical transcript replay",
  );
  assert.ok(
    runGateBody.includes("receipts.wasm = wasmReceipt"),
    "the lane receipt must reach the reducer",
  );
  assert.ok(
    gateSource.includes("const wasmCargoEnv =") &&
      gateSource.includes("laneLayout.wasmJsBoundary.targetDir"),
    "the lane builds on its own cargo target root",
  );
  // No enable flag, and no path filter: the whole point of the lane is that it cannot be quietly skipped.
  assert.ok(
    !/WASM_LANE_ENABLED|wasmLaneEnabled/.test(gateSource),
    "the lane must not acquire an enable flag",
  );
  assert.ok(
    !/wasm[A-Za-z]*Changed|changedFiles/.test(runGateBody),
    "the lane must not be path-filtered inside the gate",
  );
});

test("every workflow runner pin equals the wasm-bindgen version the gate derives from the tree", () => {
  // The gate REFUSES a runner whose version differs from the tree's `wasm-bindgen` dependency, so a
  // workflow pin left behind by a dependency bump does not ship a silently-skewed lane — it reddens CI.
  // Catching the drift here instead makes that a local failure with an obvious fix.
  const manifest = readFileSync(join(REPO_ROOT, "crates", "verter_wasm", "Cargo.toml"), "utf8");
  const declared = /^wasm-bindgen\s*=\s*"([^"]+)"/m.exec(manifest);
  assert.ok(declared, "crates/verter_wasm/Cargo.toml must declare a wasm-bindgen version");
  const expected = deriveWasmBindgenPin([{ name: "verter_wasm", bindgenReqs: [declared[1]] }]);
  assert.equal(expected.error, null, expected.error || "");

  for (const workflow of ["ci.yml", "release.yml"]) {
    const source = readFileSync(join(REPO_ROOT, ".github", "workflows", workflow), "utf8");
    const pins = [...source.matchAll(/wasm-bindgen-cli@([0-9][^\s"']*)/g)].map((m) => m[1]);
    assert.ok(pins.length > 0, `${workflow} must pin wasm-bindgen-cli`);
    for (const pin of pins) {
      assert.equal(
        pin,
        expected.version,
        `${workflow} pins wasm-bindgen-cli@${pin} but the tree declares ${expected.version}`,
      );
    }
  }

  // Each workflow must actually RUN the lane, and provision what it needs. Which
  // entry point runs it is not the invariant — `release.yml` reaches it through
  // the canonical gate, while `ci.yml` builds one shared nextest archive and
  // therefore drives the lane through its standalone entry. The invariant is
  // that a workflow does not merely install the target and the runner and then
  // never execute the only run that can reach a `#[wasm_bindgen_test]` case.
  const LANE_ENTRIES = ["node scripts/gate.mjs --exhaustive", "scripts/wasm-js-boundary-lane.mjs"];
  for (const workflow of ["ci.yml", "release.yml"]) {
    const source = readFileSync(join(REPO_ROOT, ".github", "workflows", workflow), "utf8");
    assert.ok(
      LANE_ENTRIES.some((entry) => source.includes(entry)),
      `${workflow} must run the wasm JS-boundary lane through one of: ${LANE_ENTRIES.join(", ")}`,
    );
    assert.ok(source.includes(WASM_LANE_TARGET), `${workflow} must provision ${WASM_LANE_TARGET}`);
  }
});
