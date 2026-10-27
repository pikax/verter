#!/usr/bin/env node

// Tests for perturbation.mjs and the hermetic candidate list. Run: node --test scripts/perturbation.test.mjs

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { allowListedVariables, buildMetadata, junitTestCases } from "./ci-junit-metadata.mjs";
import {
  exclusionReason,
  hermeticTestIds,
  readHermeticListDocument,
  validateHermeticList,
} from "./lib/hermetic-list.mjs";
import {
  compareReruns,
  executionEnvironment,
  readConfig,
  rerunArtifacts,
  runRerun,
  validateConfig,
} from "./perturbation.mjs";

function withDir(prefix, body) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  try {
    return body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const LIST = {
  schemaVersion: 2,
  version: 4,
  approved: false,
  approvedBy: null,
  approvedOn: null,
  derivedFrom: { run: "https://example.test/runs/1", reruns: 3, generatedAt: "2026-10-01" },
  candidates: ["a m::one", "a m::two"],
  exclusions: [{ reason: "build-time value", patterns: ["^b(::\\S+)? "] }],
};

test("the committed list is well formed and excludes verter_lsp's build date", () => {
  const list = readHermeticListDocument();
  const hermetic = hermeticTestIds(list);
  if (list.approved) {
    assert.deepEqual([...hermetic], list.candidates);
  } else {
    assert.equal(hermetic.size, 0);
  }
  for (const id of [
    "verter_lsp main_tests::logs_the_version",
    "verter_lsp::bin/verter-lsp main_tests::logs_the_version",
    "verter_lsp::main cases::stdio_launch_smoke::starts",
  ]) {
    assert.match(exclusionReason(list, id) ?? "", /VERTER_BUILD_DATE/, id);
  }
  assert.match(exclusionReason(list, "verter_bench::main cases::x") ?? "", /Build-time value/);
  assert.equal(exclusionReason(list, "verter_lsp_shim cases::x"), null);
  assert.equal(exclusionReason(list, "verter_span tests::offsets"), null);
});

test("the committed list excludes tests that read the clock for a budget, in nested modules too", () => {
  const list = readHermeticListDocument();
  for (const id of [
    // Samples the clock until a deadline and asserts how many samples it took.
    "verter_session::main cases::g_audit::audit_request_registration_survives_worker_tls_churn::active_request_registration_survives_worker_tls_install_drop_churn",
    // recv_timeout bounds.
    "verter_session::main cases::g_misc0::sampler_thread_joined_at_host_drop::sampler_shuts_down_during_an_active_sample",
    // wait_timeout_while and recv_timeout bounds.
    "verter_session::main cases::g_misc2::scheduler_audit_queue_dwell_under_load::at_least_one_concurrent_request_observes_non_zero_queue_dwell_ms",
    "verter_scheduler scheduler::reset_tests::a_completion_wake_can_reset_its_own_driver_without_leaving_it_running",
    // A module nested in an excluded module is excluded with it.
    "verter_scheduler scheduler::reset_tests::nested::a_test",
  ]) {
    assert.match(exclusionReason(list, id) ?? "", /^Timing budget:/, id);
  }
});

test("only an approved list makes a test hermetic, and never an excluded one", () => {
  assert.deepEqual([...hermeticTestIds(LIST)], []);
  const approved = { ...LIST, approved: true, approvedBy: "operator", approvedOn: "2026-10-02" };
  assert.deepEqual(validateHermeticList(approved), []);
  assert.deepEqual([...hermeticTestIds(approved)], ["a m::one", "a m::two"]);
});

test("a malformed list is refused", () => {
  const mutations = [
    (l) => (l.schemaVersion = 1),
    (l) => (l.version = 0),
    (l) => (l.approved = true),
    (l) => Object.assign(l, { approved: true, approvedBy: "operator", approvedOn: "soon" }),
    (l) => (l.approvedBy = "operator"),
    (l) => (l.candidates = ["a m::two", "a m::one"]),
    (l) => (l.candidates = ["a m::one", "a m::one"]),
    (l) => (l.candidates = ["no-space"]),
    (l) => l.candidates.push("b m::excluded"),
    (l) => (l.derivedFrom = null),
    (l) => (l.exclusions[0].reason = ""),
    (l) => (l.exclusions[0].patterns = []),
    (l) => (l.exclusions[0].patterns = ["("]),
    (l) => l.exclusions.push({ reason: "again", patterns: ["^b(::\\S+)? "] }),
  ];
  for (const mutate of mutations) {
    const list = structuredClone(LIST);
    mutate(list);
    assert.notDeepEqual(validateHermeticList(list), [], String(mutate));
  }
});

test("junit testcases carry their outcome", () => {
  const xml = `<testsuites><testsuite name="a">
<testcase name="p" classname="a"/>
<testcase name="o" classname="a"><system-out>&lt;failure</system-out></testcase>
<testcase name="f" classname="a"><failure message="x"/></testcase>
<testcase name="e" classname="a"><error message="x"/></testcase>
<testcase name="r" classname="a"><flakyFailure message="x"/></testcase>
<testcase name="s" classname="a"><skipped/></testcase>
</testsuite></testsuites>`;
  assert.deepEqual(
    junitTestCases(xml).map((c) => `${c.id}=${c.outcome}`),
    ["a p=passed", "a o=passed", "a f=failed", "a e=error", "a r=flaky", "a s=skipped"],
  );
});

test("the committed configuration is usable and a malformed one is refused", () => {
  const config = readConfig();
  assert.ok(config.reruns >= 2);
  const mutations = [
    (c) => (c.reruns = 1),
    (c) => (c.timeZones = c.timeZones.slice(0, 1)),
    (c) => (c.timeZones[1] = c.timeZones[0]),
    (c) => (c.clockOffsetDays[1] = c.clockOffsetDays[0]),
    (c) => (c.clockOffsetDays[0] = 0),
    (c) => (c.runnerSuppliedVariables = ["GITHUB_TOKEN"]),
  ];
  for (const mutate of mutations) {
    const doc = structuredClone(config);
    mutate(doc);
    assert.notDeepEqual(validateConfig(doc), [], String(mutate));
  }
});

test("the execution environment is the allow-list plus the per-job directories", () => {
  const env = executionEnvironment({
    env: { PATH: "/bin", CI: "true", RUSTFLAGS: "-D warnings", GITHUB_TOKEN: "secret", HOME: "/h" },
    runnerVariables: { CI: "perturbed-2-x" },
    home: "/job/home",
    tmp: "/job/tmp",
    timeZone: "Etc/GMT+12",
  });
  assert.equal(env.GITHUB_TOKEN, undefined);
  assert.equal(env.CI, "perturbed-2-x");
  assert.equal(env.RUSTFLAGS, "-D warnings");
  assert.equal(env.HOME, "/job/home");
  assert.equal(env.TMPDIR, "/job/tmp");
  assert.equal(env.TZ, "Etc/GMT+12");
  assert.equal(env.PATH, "/bin");
});

test("a rerun runs the command under the cleared environment and records each variation", () => {
  withDir("perturb-run-", (dir) => {
    const seen = join(dir, "seen.json");
    const options = {
      rerun: "2",
      "work-dir": join(dir, "work"),
      record: join(dir, "record.json"),
      "env-snapshot": join(dir, "env.json"),
    };
    const write = `require("fs").writeFileSync(process.argv[1], JSON.stringify(process.env)); process.exit(Number(process.argv[2]))`;
    const env = { ...process.env, GITHUB_TOKEN: "secret", NEXTEST_PROFILE: "ci" };
    assert.equal(runRerun(options, [process.execPath, "-e", write, seen, "100"], env), 0);
    const record = JSON.parse(readFileSync(options.record, "utf8"));
    const childEnv = JSON.parse(readFileSync(seen, "utf8"));
    assert.equal(childEnv.GITHUB_TOKEN, undefined);
    assert.equal(childEnv.HOME, record.variation.home);
    assert.equal(childEnv.TMPDIR, record.variation.temporaryDirectory);
    assert.equal(childEnv.TZ, readConfig().timeZones[1]);
    assert.match(childEnv.CI, /^perturbed-2-/);
    assert.equal(record.variation.runnerSuppliedVariables.CI.rerun, childEnv.CI);
    assert.equal(record.exitCode, 100);
    assert.equal(record.rerun, 2);
    assert.equal(record.variation.clock.libfaketime.applied, false);
    assert.equal(typeof record.networkIsolation.applied, "boolean");
    assert.ok(record.variation.host.hostname);
    const snapshot = JSON.parse(readFileSync(options["env-snapshot"], "utf8"));
    assert.equal(snapshot.variables.CI, childEnv.CI);
    assert.equal(snapshot.variables.NEXTEST_PROFILE, "ci");
    assert.equal(runRerun(options, [process.execPath, "-e", write, seen, "3"], env), 3);
  });
});

const IDS = ["a m::steady", "a m::flips", "a m::broken", "a m::gone", "b m::built"];

function junitFor(rerun) {
  const outcome = {
    "a m::steady": "pass",
    "a m::flips": rerun === 2 ? "fail" : "pass",
    "a m::broken": "fail",
    "a m::gone": rerun === 3 ? null : "pass",
    "b m::built": "pass",
  };
  const cases = IDS.filter((id) => outcome[id] !== null).map((id) => {
    const [classname, name] = id.split(" ");
    return outcome[id] === "fail"
      ? `<testcase name="${name}" classname="${classname}"><failure/></testcase>`
      : `<testcase name="${name}" classname="${classname}"/>`;
  });
  return `<testsuites><testsuite name="s">${cases.join("")}</testsuite></testsuites>`;
}

const REAL_START = Date.parse("2026-10-27T02:30:00Z");

/** A clock record whose read-back is `observedShiftDays` off the real clock. */
function shiftedClock(
  timeZone,
  offsetDays,
  { applied = true, observedShiftDays = offsetDays } = {},
) {
  return {
    timeZone,
    offsetDays,
    libfaketime: applied ? { applied: true } : { applied: false, reason: "no library" },
    realStart: new Date(REAL_START).toISOString(),
    observedStart: new Date(REAL_START + observedShiftDays * 86400000).toISOString(),
  };
}

function recordFor(rerun, overrides = {}) {
  return {
    schemaVersion: 1,
    rerun,
    reruns: 3,
    privileges: { uid: 1001, euid: 1001, unprivileged: true, elevation: "none" },
    osImage: { distribution: "ubuntu", release: "24.04", kernel: "6.8.0" },
    baseFingerprint: { os: "linux 6.8.0", architecture: "x64", variables: { CI: "true" } },
    variation: {
      temporaryDirectory: `/t/rerun-${rerun}/tmp`,
      runnerSuppliedVariables: { CI: { runner: "true", rerun: `perturbed-${rerun}` } },
      host: { hostname: `host-${rerun}` },
      clock: shiftedClock(`Z${rerun}`, rerun),
    },
    exitCode: 100,
    ...overrides,
  };
}

/** Writes three reruns' artifacts the way the workflow downloads them. */
function writeReruns(dir, recordOverrides = {}) {
  for (let rerun = 1; rerun <= 3; rerun += 1) {
    const names = rerunArtifacts(rerun);
    const evidence = join(dir, names.evidence);
    mkdirSync(evidence, { recursive: true });
    mkdirSync(join(dir, names.record), { recursive: true });
    const junit = join(evidence, "junit.xml");
    writeFileSync(junit, junitFor(rerun));
    const snapshot = join(dir, `env-${rerun}.json`);
    writeFileSync(
      snapshot,
      JSON.stringify({ variables: allowListedVariables({ NEXTEST_PROFILE: "ci" }) }),
    );
    const meta = buildMetadata(
      {
        junit,
        profile: "ci",
        "env-snapshot": snapshot,
        "argv-json": JSON.stringify(["nextest", "run", "-E", "all()"]),
        tools: "node",
      },
      {
        GITHUB_EVENT_NAME: "schedule",
        RUNNER_OS: "Linux",
        GITHUB_JOB: "rerun",
        GITHUB_WORKFLOW: "Perturbation",
      },
    );
    meta.tools = { ...meta.tools, rustc: "r", cargo: "c", "cargo-nextest": "n", pnpm: "p" };
    writeFileSync(join(evidence, "junit-metadata.json"), JSON.stringify(meta));
    writeFileSync(
      join(dir, names.record, "perturbation-record.json"),
      JSON.stringify(recordFor(rerun, recordOverrides[rerun])),
    );
  }
}

const CONFIG = { reruns: 3 };

test("a candidate passed unchanged in every rerun and no exclusion names it", () => {
  withDir("perturb-cmp-", (dir) => {
    writeReruns(dir);
    const { comparison, proposed } = compareReruns({
      input: dir,
      run: "https://example.test/runs/9",
      config: CONFIG,
      list: LIST,
      generatedAt: "2026-10-27",
    });
    assert.deepEqual(comparison.problems, []);
    assert.equal(comparison.comparable, true);
    const byId = Object.fromEntries(comparison.tests.map((t) => [t.id, t]));
    assert.deepEqual(byId["a m::steady"].outcomes, ["passed", "passed", "passed"]);
    assert.equal(byId["a m::steady"].candidate, true);
    assert.deepEqual(byId["a m::flips"].outcomes, ["passed", "failed", "passed"]);
    assert.match(byId["a m::flips"].reason, /differs/);
    assert.match(byId["a m::broken"].reason, /failed in every rerun/);
    assert.deepEqual(byId["a m::gone"].outcomes, ["passed", "passed", "absent"]);
    assert.equal(byId["a m::gone"].candidate, false);
    assert.match(byId["b m::built"].reason, /excluded: build-time value/);
    assert.deepEqual(byId["a m::steady"].hermetic, [false, false, false]);
    assert.deepEqual(proposed.candidates, ["a m::steady"]);
    assert.equal(proposed.version, LIST.version + 1);
    assert.equal(proposed.approved, false);
    assert.equal(proposed.derivedFrom.run, "https://example.test/runs/9");
    assert.deepEqual(validateHermeticList(proposed), []);
    assert.equal(comparison.rerunRecords[0].junitMetadata.runConfiguration.nextestProfile, "ci");
  });
});

test("reruns that do not vary every dimension, or differ in fingerprint, propose nothing", () => {
  const cases = [
    [{ 2: { variation: { ...recordFor(2).variation, host: { hostname: "host-1" } } } }, /host/],
    [{ 3: { baseFingerprint: { os: "linux 6.9.0", architecture: "x64" } } }, /fingerprints/],
    [{ 1: { privileges: { uid: 0, euid: 0, unprivileged: false } } }, /unprivileged/],
    [{ 1: { osImage: { distribution: "ubuntu", release: "24.04" } } }, /kernel/],
    [{ 2: { exitCode: 101 } }, /did not complete/],
    [
      {
        2: {
          variation: {
            ...recordFor(2).variation,
            clock: shiftedClock("Z1", 1),
          },
        },
      },
      /wall clock offset[^]*time zone/,
    ],
  ];
  // Reruns whose wall clock was never shifted, or shifted by other than the
  // configured offset, varied no clock or date, however distinct their time zones.
  const unshifted = (rerun, options) => ({
    variation: { ...recordFor(rerun).variation, clock: shiftedClock(`Z${rerun}`, rerun, options) },
  });
  cases.push(
    [
      {
        1: unshifted(1, { applied: false, observedShiftDays: 0 }),
        2: unshifted(2, { applied: false, observedShiftDays: 0 }),
        3: unshifted(3, { applied: false, observedShiftDays: 0 }),
      },
      /rerun 1: the wall clock was not shifted \(no library\)/,
    ],
    [
      { 2: unshifted(2, { applied: false, observedShiftDays: 0 }) },
      /rerun 2: the wall clock was not shifted/,
    ],
    [
      { 3: unshifted(3, { observedShiftDays: 0 }) },
      /rerun 3: the wall clock read under the suite was 0s off, not 3d/,
    ],
    [
      {
        1: {
          variation: {
            ...recordFor(1).variation,
            clock: { ...shiftedClock("Z1", 1), observedStart: null },
          },
        },
      },
      /rerun 1: the shifted wall clock was not read back/,
    ],
  );
  for (const [overrides, expected] of cases) {
    withDir("perturb-cmp-", (dir) => {
      writeReruns(dir, overrides);
      const { comparison, proposed } = compareReruns({
        input: dir,
        run: null,
        config: CONFIG,
        list: LIST,
      });
      assert.equal(comparison.comparable, false);
      assert.match(comparison.problems.join("\n"), expected);
      assert.deepEqual(proposed.candidates, []);
      assert.equal(proposed.derivedFrom, null);
      assert.ok(comparison.tests.every((t) => !t.candidate));
    });
  }
  withDir("perturb-cmp-", (dir) => {
    writeReruns(dir);
    rmSync(join(dir, rerunArtifacts(3).record), { recursive: true });
    const { comparison } = compareReruns({ input: dir, run: null, config: CONFIG, list: LIST });
    assert.match(comparison.problems.join("\n"), /rerun 3: no record/);
  });
});
