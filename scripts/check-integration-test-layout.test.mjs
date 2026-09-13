#!/usr/bin/env node

// Tests for check-integration-test-layout.mjs. Run: node --test scripts/check-integration-test-layout.test.mjs
//
// computeFailures and parseAllowlist are pure and tested against synthetic
// layouts covering the discriminations the guard exists for: a stray second
// top-level tests/*.rs, a stale allowlist entry, autotests = false hiding
// targets, tests/main.rs missing from metadata, two [[test]] blocks both
// pointing at tests/main.rs, and a hidden nested tests/<dir>/main.rs. One test
// additionally pins the COMMITTED allowlist to exactly the known standalone
// targets — the durable exact-set pin (adding/removing an exception is an
// architecture decision and must not slip in unnoticed).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { computeFailures, parseAllowlist } from "./check-integration-test-layout.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWLIST_PATH = join(repoRoot, "scripts", "integration-test-layout-allowlist.json");

const MAIN_SRC = "crates/demo/tests/main.rs";

// A conformant baseline: one package with exactly tests/main.rs.
function conformantLayout() {
  return {
    name: "demo",
    expectedMainSrcPosix: MAIN_SRC,
    mainRsExists: true,
    testTargets: [{ name: "main", src: MAIN_SRC }],
    immediateTestFiles: [MAIN_SRC],
    autoDiscoverableCandidates: [MAIN_SRC],
  };
}

const NO_ALLOW = [];

test("a conformant single tests/main.rs layout produces zero failures", () => {
  assert.deepEqual(computeFailures([conformantLayout()], NO_ALLOW), []);
});

test("a second top-level tests/*.rs is flagged as a non-allowlisted target and a stray immediate file", () => {
  const withStray = conformantLayout();
  withStray.testTargets = [
    { name: "main", src: MAIN_SRC },
    { name: "rogue", src: "crates/demo/tests/rogue.rs" },
  ];
  withStray.immediateTestFiles = [MAIN_SRC, "crates/demo/tests/rogue.rs"];
  withStray.autoDiscoverableCandidates = [MAIN_SRC, "crates/demo/tests/rogue.rs"];
  const failures = computeFailures([withStray], NO_ALLOW);
  assert.ok(
    failures.some((f) => f.message.includes("is not") && f.message.includes("allowlisted")),
    `a second top-level tests/*.rs MUST be flagged as a non-allowlisted target; got: ${JSON.stringify(failures)}`,
  );
  assert.ok(
    failures.some((f) => f.message.includes("stray immediate test file")),
    `a second top-level tests/*.rs MUST be flagged as a stray immediate file; got: ${JSON.stringify(failures)}`,
  );
});

test("an allowlist entry naming a non-existent target is flagged stale", () => {
  const staleAllow = [
    { package: "demo", target: "ghost", src_path: "crates/demo/tests/ghost.rs", reason: "x" },
  ];
  const failures = computeFailures([conformantLayout()], staleAllow);
  assert.ok(
    failures.some((f) => f.message.includes("STALE allowlist entry")),
    `an allowlist entry naming a non-existent target MUST be flagged stale; got: ${JSON.stringify(failures)}`,
  );
});

test("immediate tests/*.rs with zero metadata targets is flagged (autotests=false hiding tests)", () => {
  const autotestsOff = conformantLayout();
  autotestsOff.testTargets = [];
  const failures = computeFailures([autotestsOff], NO_ALLOW);
  assert.ok(
    failures.some((f) => f.message.includes("ZERO integration-test targets")),
    `tests/*.rs present with zero metadata targets MUST be flagged; got: ${JSON.stringify(failures)}`,
  );
});

test("tests/main.rs on disk but absent from metadata is flagged, while an exactly-allowlisted target stays exempt", () => {
  const canarySrc = "crates/demo/tests/allocator_canaries.rs";
  const missingMain = conformantLayout();
  missingMain.testTargets = [{ name: "allocator_canaries", src: canarySrc }];
  missingMain.immediateTestFiles = [canarySrc];
  // The auto-discoverable candidate (the allowlisted canary) HAS a matching
  // metadata target, so the only signal that fires here is missing-main.
  missingMain.autoDiscoverableCandidates = [canarySrc];
  const canaryAllow = [
    {
      package: "demo",
      target: "allocator_canaries",
      src_path: canarySrc,
      reason: "single counting #[global_allocator]",
    },
  ];
  const failures = computeFailures([missingMain], canaryAllow);
  assert.ok(
    failures.some((f) => f.message.includes("does NOT report a tests/main.rs")),
    `tests/main.rs on disk but absent from metadata MUST be flagged; got: ${JSON.stringify(failures)}`,
  );
  assert.ok(
    !failures.some((f) => f.message.includes("is not") && f.message.includes("allowlisted")),
    `an exactly-allowlisted target must be exempt from the 'not allowlisted' failure; got: ${JSON.stringify(failures)}`,
  );
});

test("two [[test]] blocks both pointing at tests/main.rs are flagged as a duplicate main", () => {
  const duplicateMain = conformantLayout();
  duplicateMain.testTargets = [
    { name: "main_a", src: MAIN_SRC },
    { name: "main_b", src: MAIN_SRC },
  ];
  const failures = computeFailures([duplicateMain], NO_ALLOW);
  assert.ok(
    failures.some(
      (f) =>
        f.message.includes("tests/main.rs integration-test targets") &&
        f.message.includes("exactly one"),
    ),
    `two [[test]] blocks both pointing at tests/main.rs MUST be flagged — a second ` +
      `binary still compiles even though both share the sanctioned src; got: ${JSON.stringify(failures)}`,
  );
  // The single-main baseline must NOT trip the duplicate-main check.
  assert.ok(
    !computeFailures([conformantLayout()], NO_ALLOW).some((f) =>
      f.message.includes("tests/main.rs integration-test targets"),
    ),
    "a single tests/main.rs target must not be flagged as a duplicate",
  );
});

test("a hidden nested tests/<dir>/main.rs is flagged even when another valid target exists", () => {
  const hiddenNestedMainSrc = "crates/demo/tests/rogue/main.rs";
  const hiddenNestedMain = conformantLayout();
  // metadata reports ONLY the sanctioned main target (the nested-main is hidden).
  hiddenNestedMain.testTargets = [{ name: "main", src: MAIN_SRC }];
  hiddenNestedMain.immediateTestFiles = [MAIN_SRC];
  // disk has the sanctioned main PLUS a nested rogue main — both are
  // cargo-auto-discoverable positions.
  hiddenNestedMain.autoDiscoverableCandidates = [MAIN_SRC, hiddenNestedMainSrc];
  const failures = computeFailures([hiddenNestedMain], NO_ALLOW);
  assert.ok(
    failures.some(
      (f) =>
        f.message.includes(hiddenNestedMainSrc) &&
        f.message.includes("cargo metadata reports no integration-test target"),
    ),
    `a hidden nested tests/<dir>/main.rs (autotests=false) MUST be flagged as a ` +
      `compiled-but-unreported binary even when another target exists; got: ${JSON.stringify(failures)}`,
  );
  // The conformant baseline (its sole candidate IS reported) must NOT trip the
  // auto-discoverable check.
  assert.ok(
    !computeFailures([conformantLayout()], NO_ALLOW).some((f) =>
      f.message.includes("cargo metadata reports no integration-test target"),
    ),
    "an auto-discoverable candidate that HAS a matching metadata target must not be flagged as hidden",
  );
});

test("parseAllowlist rejects a duplicate (package, target) key", () => {
  const raw = JSON.stringify({
    allow: [
      { package: "p", target: "t", src_path: "crates/p/tests/a.rs", reason: "x" },
      { package: "p", target: "t", src_path: "crates/p/tests/b.rs", reason: "y" },
    ],
  });
  assert.throws(() => parseAllowlist(raw), /duplicate allowlist \(package, target\) key/);
});

test("parseAllowlist rejects a fully-identical duplicate entry", () => {
  const entry = { package: "p", target: "t", src_path: "crates/p/tests/a.rs", reason: "x" };
  const raw = JSON.stringify({ allow: [entry, { ...entry }] });
  assert.throws(() => parseAllowlist(raw), /duplicate allowlist entry/);
});

test("parseAllowlist accepts a well-formed, duplicate-free allowlist", () => {
  const raw = JSON.stringify({
    allow: [
      { package: "p", target: "t1", src_path: "crates/p/tests/a.rs", reason: "x" },
      { package: "p", target: "t2", src_path: "crates/p/tests/b.rs", reason: "y" },
    ],
  });
  assert.equal(parseAllowlist(raw).length, 2, "two distinct entries must both load");
});

test("the committed allowlist is exactly the known standalone targets", () => {
  const entries = parseAllowlist(readFileSync(ALLOWLIST_PATH, "utf8"));
  const actual = entries.map((e) => `${e.package}::${e.target}::${e.src_path}`).sort();
  const expected = [
    "verter_compiler::allocator_canaries::crates/verter_compiler/tests/allocator_canaries.rs",
    "verter_lsp::lsp_audit_trace_out_env_var::crates/verter_lsp/tests/lsp_audit_trace_out_env_var.rs",
    "verter_session::allocator_canaries::crates/verter_session/tests/allocator_canaries.rs",
  ];
  assert.deepEqual(
    actual,
    expected,
    "the integration-test-layout allowlist drifted from the known standalone targets " +
      "(allocator_canaries x2 + lsp_audit_trace_out_env_var). Adding/removing an exception is " +
      "an architecture decision: update this pin AND " +
      "scripts/integration-test-layout-allowlist.json, and justify the standalone target.",
  );
});
