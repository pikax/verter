#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { LANE_GATES } from "./ci-impact.mjs";
import { CONTRACT_GROUPS, compileContractPlan, runCompileContracts } from "./compile-contracts.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

function yamlJob(source, name) {
  const start = source.indexOf(`\n  ${name}:`);
  assert.notEqual(start, -1, `workflow must define the ${name} job`);
  const next = source.slice(start + 1).search(/\n  [a-z0-9][a-z0-9-]*:\r?\n/);
  return next === -1 ? source.slice(start) : source.slice(start, start + 1 + next);
}

test("compile-contract cache belongs to the required standalone runner job", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const rustJob = yamlJob(workflow, "rust-test");
  const compilerJob = yamlJob(workflow, "compiler-contracts");
  const successJob = yamlJob(workflow, "ci-success");

  assert.match(compilerJob, /needs:\s*detect-changes/);
  // Gated on the compile-contract owners' dependency closure, not on any
  // Rust change: an LSP-only or tooling-crate change does not re-run them.
  assert.match(compilerJob, /needs\.detect-changes\.outputs\.compiler_contracts\s*==\s*'true'/);
  assert.match(compilerJob, /target\/compiler-contracts/);
  assert.match(compilerJob, /node scripts\/compile-contracts\.mjs --group/);
  assert.match(compilerJob, /generated_svelte_artifacts_match_their_authoritative_inputs/);
  // Test targets activate the dev-dependency self-edge's test-support feature;
  // they cannot replace the production feature-off build.
  assert.match(compilerJob, /cargo build -p verter_compiler/);
  assert.match(compilerJob, /cargo test -p verter_compiler --lib/);
  assert.doesNotMatch(compilerJob, /cargo nextest|install-action@nextest/);
  assert.doesNotMatch(compilerJob, /max-threads|test-threads|--jobs|-j\s*\d/);
  assert.doesNotMatch(rustJob, /target\/tests\/trybuild/);
  assert.match(successJob, /^\s*- compiler-contracts\s*$/m);
});

test("the matrix partitions all owners and feature variants without feature unification", () => {
  const all = compileContractPlan();
  const split = Object.keys(CONTRACT_GROUPS).flatMap((group) => compileContractPlan(group));
  assert.deepEqual(split, all);
  assert.equal(new Set(all.map(({ name }) => name)).size, 15);
  const owners = spawnSync(
    process.execPath,
    [join(SCRIPT_DIR, "compile-contracts.mjs"), "--list-owners"],
    {
      encoding: "utf8",
    },
  );
  assert.equal(owners.status, 0, owners.stderr);
  for (const owner of owners.stdout.trim().split(/\r?\n/)) {
    assert.deepEqual(all.find(({ name }) => name === owner)?.args, [
      "run",
      "--locked",
      "-p",
      "verter_compile_contracts",
      "--no-default-features",
      "--features",
      owner,
    ]);
  }
  for (const [name, pkg] of Object.entries({
    "compiler-bench": "verter_compile_contracts_bench",
    "session-feature-variants": "verter_compile_contracts_session_variants",
    "session-soak": "verter_compile_contracts_session_soak",
  })) {
    assert.deepEqual(all.find((task) => task.name === name)?.args, ["run", "--locked", "-p", pkg]);
  }
  assert.deepEqual(all.find(({ name }) => name === "session-observe")?.args, [
    "run",
    "--locked",
    "--manifest-path",
    "crates/verter_compile_contracts_session_observe/Cargo.toml",
  ]);
  assert.throws(() => compileContractPlan("typo"), /unknown compile-contract group/);
});

test("each group fetches its locks first and shares one absolute target for all its commands", () => {
  for (const group of Object.keys(CONTRACT_GROUPS)) {
    const calls = [];
    const status = runCompileContracts({
      group,
      env: { CARGO_TARGET_DIR: "target/custom-contracts" },
      log() {},
      run(command, args, options) {
        calls.push({ command, args, options });
        return { status: 0 };
      },
    });
    assert.equal(status, 0);
    assert.deepEqual(calls[0].args, ["fetch", "--locked"]);
    const fetchCount = group === "session-observe" ? 2 : 1;
    if (fetchCount === 2) {
      assert.deepEqual(calls[1].args, [
        "fetch",
        "--locked",
        "--manifest-path",
        "crates/verter_compile_contracts_session_observe/Cargo.toml",
      ]);
    }
    assert.deepEqual(
      calls.slice(fetchCount).map(({ args }) => args),
      compileContractPlan(group).map(({ args }) => args),
    );
    for (const call of calls) {
      assert.equal(call.command, "cargo");
      assert.equal(call.options.cwd, REPO_ROOT);
      assert.equal(
        call.options.env.CARGO_TARGET_DIR,
        resolve(REPO_ROOT, "target/custom-contracts"),
      );
    }
  }
});

test("a failed prerequisite stops the group; contract failures still run the remaining configurations", () => {
  for (const failure of [
    { status: 17 },
    { status: null, signal: "SIGTERM" },
    { error: new Error("spawn failed") },
  ]) {
    const calls = [];
    const run = (_command, args) => {
      calls.push(args);
      return calls.length === 1 ? failure : { status: 0 };
    };
    assert.equal(runCompileContracts({ group: "session", run, log() {} }), 1);
    assert.equal(calls.length, 1);
  }
  const calls = [];
  assert.equal(
    runCompileContracts({
      group: "compiler",
      log() {},
      run(_command, args) {
        calls.push(args);
        return { status: calls.length === 2 ? 1 : 0 };
      },
    }),
    1,
  );
  assert.equal(calls.length, 4, "bench and soak still execute after the default contract fails");
});

test("compile-contract cache uses immutable restore and save without target pruning", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const job = yamlJob(workflow, "compiler-contracts");
  const actions = [...job.matchAll(/uses: actions\/cache\/(restore|save)@([a-f0-9]{40})\b/g)];

  assert.deepEqual(
    actions.map((match) => match[1]),
    ["restore", "save"],
  );
  assert.equal(actions[0][2], actions[1][2], "restore and save use the same reviewed dependency");
  assert.doesNotMatch(job, /Swatinem\/rust-cache|continue-on-error/);
  assert.match(job, /github\.ref == 'refs\/heads\/main'.*cache-hit != 'true'/);
  assert.match(job, /steps\.compiler-contract-cache\.outputs\.cache-primary-key/);
  assert.equal((job.match(/^\s+target\/compiler-contracts\s*$/gm) ?? []).length, 2);
  assert.ok(
    job.indexOf("uses: actions/cache/save@") > job.indexOf("cargo test -p verter_compiler --lib"),
    "cache saving follows every required compiler check",
  );
});

test("PR and nightly matrices run every group with isolated caches and an active compiler cache", () => {
  for (const file of ["ci.yml", "nightly.yml"]) {
    const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", file), "utf8");
    const job = yamlJob(workflow, "compiler-contracts");
    const matrix = job
      .match(/group: \[([^\]]+)\]/)?.[1]
      .split(",")
      .map((value) => value.trim());
    assert.deepEqual(matrix, Object.keys(CONTRACT_GROUPS), file);
    assert.match(job, /fail-fast: false/);
    // setup-node otherwise auto-detects packageManager and invokes pnpm even
    // for Rust-only groups, where pnpm/action-setup deliberately does not run.
    const nodeSetup = job.match(
      /uses: actions\/setup-node@v5\r?\n([\s\S]*?)(?=\r?\n      - )/,
    )?.[1];
    assert.ok(nodeSetup, file);
    assert.match(nodeSetup, /package-manager-cache: false/, file);
    assert.match(
      nodeSetup,
      /cache: \$\{\{ matrix\.group == 'compiler' && 'pnpm' \|\| '' \}\}/,
      file,
    );
    assert.match(job, /cargo build -p verter_compiler/);
    assert.match(job, /RUSTC_WRAPPER: sccache/);
    assert.match(job, /CARGO_INCREMENTAL: "0"/);
    assert.match(job, /CARGO_PROFILE_DEV_DEBUG: "0"/);
    assert.match(job, /CARGO_PROFILE_TEST_DEBUG: "0"/);
    assert.match(job, /SCCACHE_BASEDIRS: \$\{\{ github\.workspace \}\}/);
    assert.match(job, /SCCACHE_GHA_VERSION: "verter-compiler-contracts-v2"/);
    assert.match(job, /key:.*matrix\.group.*rust-toolchain\.toml.*Cargo\.lock.*github\.sha/);
    assert.match(
      job,
      /if: matrix\.group == 'compiler'[\s\S]*generated_svelte_artifacts_match_their_authoritative_inputs/,
    );
    assert.match(job, /if: matrix\.group == 'compiler'[\s\S]*cargo test -p verter_compiler --lib/);
    assert.match(job, /if: always\(\)[\s\S]*actions\/upload-artifact/);
    assert.match(job, /shell: bash\s+run: node scripts\/compile-contracts\.mjs --group/);
    if (file === "nightly.yml") {
      assert.match(workflow, /schedule:[\s\S]*cron:/);
      assert.doesNotMatch(job, /detect-changes|outputs\.compiler_contracts/);
      assert.match(job, /permissions:\s*contents: read/);
    }
  }
});

// @ai-generated - Guards workflow prerequisites for generated Svelte conformance artifacts.
test("Svelte conformance runs for golden-generator changes", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const conformanceJob = yamlJob(workflow, "svelte-conformance");

  // The gate is composed in detect-changes: the `svelte_oracle` path filter
  // (golden generator, corpus, vendored oracle sources) OR the conformance
  // crate's dependency closure. Both halves are asserted where they live.
  assert.match(
    conformanceJob,
    /if:\s*needs\.detect-changes\.outputs\.svelte_conformance\s*==\s*'true'/,
  );
  assert.deepEqual(LANE_GATES.svelte_conformance, {
    filters: ["svelte_oracle"],
    impact: ["svelte_conformance"],
  });
  assert.match(conformanceJob, /gen-svelte-goldens\.mjs --conformance --check/);
});

// @ai-generated - Guards clean-checkout package entrypoints used by the JavaScript tests.
// The JavaScript suite is CI's js-build-test lane, which a release pull request
// runs and a tag's release reuses; release.yml runs no JavaScript tests itself.
test("CI builds TypeScript package entrypoints before JavaScript tests", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const testJob = yamlJob(workflow, "js-build-test");
  const build = testJob.indexOf("pnpm run build:ts");
  const tests = testJob.indexOf("name: Run tests");

  assert.notEqual(build, -1, "the JavaScript lane must build untracked package dist entrypoints");
  assert.notEqual(tests, -1, "the JavaScript lane must execute the JavaScript test suite");
  assert.ok(
    build < tests,
    "TypeScript package entrypoints must exist before JavaScript tests start",
  );
  const release = readFileSync(join(REPO_ROOT, ".github", "workflows", "release.yml"), "utf8");
  assert.doesNotMatch(
    release,
    /pnpm test\b/,
    "release.yml reuses CI's JavaScript suite instead of repeating it",
  );
});
