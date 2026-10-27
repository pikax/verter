#!/usr/bin/env node

// Perturbation reruns of the nextest suites and the hermetic candidate list they
// propose (`.github/workflows/perturbation.yml`).
//
// Each rerun runs the same suites in an unprivileged job, with the environment
// cleared to the allow-list, HOME and TMPDIR in fresh per-job directories,
// best-effort network isolation beyond loopback, and its own temporary
// directory, runner-supplied variable values, host, and clock and time zone.
// `compare` lines the reruns up per test: a test is a candidate only when it
// passed in every rerun and no exclusion of the candidate list names it.
//
//   node scripts/perturbation.mjs plan
//        prints the rerun indexes of scripts/manifests/perturbation.json as a JSON array
//   node scripts/perturbation.mjs run --rerun <i> --work-dir <dir> --record <record.json>
//        --env-snapshot <env.json> [--faketime-lib <libfaketime.so.1>] -- <command...>
//        runs the command as rerun <i> and records what it varied
//   node scripts/perturbation.mjs compare --input <dir> --out <comparison.json>
//        --proposed <candidates.json> [--run <workflow run url>]
//        writes the per-test comparison and the proposed (unapproved) candidate list;
//        exits 1 when the reruns are not comparable
//   node scripts/perturbation.mjs check-list [--list <path>]

import { spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { arch, homedir, hostname, platform, release, version } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  ENV_ALLOWLIST,
  allowListedVariables,
  checkMetadata,
  junitTestCases,
  readToolchainPins,
} from "./ci-junit-metadata.mjs";
import {
  HERMETIC_LIST_PATH,
  HERMETIC_LIST_SCHEMA_VERSION,
  exclusionPatterns,
  exclusionReason,
  readHermeticListDocument,
  validateHermeticList,
} from "./lib/hermetic-list.mjs";

export const RECORD_SCHEMA_VERSION = 1;
export const COMPARISON_SCHEMA_VERSION = 1;

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CONFIG_PATH = join(REPO_ROOT, "scripts", "manifests", "perturbation.json");

/** nextest's exit code when tests ran and some failed: a rerun outcome, not a harness failure. */
const NEXTEST_TESTS_FAILED = 100;

/** A seconds tolerance for reading back a shifted clock. */
const CLOCK_TOLERANCE_SECONDS = 600;

// Runs the command in a new user and network namespace with only loopback up,
// then drops back to the caller's own uid and gid in a nested user namespace, so
// the tests run without capabilities and without any interface but `lo`.
const NETNS_SCRIPT =
  'u=$1; g=$2; shift 2; ip link set lo up && exec unshare --user --map-user="$u" --map-group="$g" -- "$@"';

/** Artifact directory names one rerun uploads, as `compare` reads them. */
export function rerunArtifacts(rerun) {
  return {
    evidence: `perturbation-rerun-${rerun}-nextest-evidence`,
    record: `perturbation-rerun-${rerun}-record`,
  };
}

const nonEmpty = (v) => typeof v === "string" && v.trim() !== "";
const distinct = (values) => new Set(values).size === values.length;

/** Every problem with the perturbation configuration; empty when usable. */
export function validateConfig(doc) {
  const problems = [];
  if (doc?.schemaVersion !== 1) problems.push("schemaVersion must be 1");
  const n = doc?.reruns;
  if (!Number.isInteger(n) || n < 2) problems.push("reruns must be an integer of at least 2");
  const zones = doc?.timeZones;
  if (!Array.isArray(zones) || zones.length < n || !zones.every(nonEmpty)) {
    problems.push("timeZones must name one time zone per rerun");
  } else if (!distinct(zones.slice(0, n))) {
    problems.push("each rerun needs its own time zone");
  }
  const offsets = doc?.clockOffsetDays;
  if (
    !Array.isArray(offsets) ||
    offsets.length < n ||
    !offsets.every((d) => Number.isInteger(d) && d !== 0)
  ) {
    problems.push("clockOffsetDays must give one non-zero whole-day offset per rerun");
  } else if (!distinct(offsets.slice(0, n))) {
    problems.push("each rerun needs its own clock offset");
  }
  const vars = doc?.runnerSuppliedVariables;
  if (!Array.isArray(vars) || vars.length === 0 || !vars.every((v) => ENV_ALLOWLIST.includes(v))) {
    problems.push("runnerSuppliedVariables must be allow-listed variable names");
  }
  return problems;
}

export function readConfig(path = CONFIG_PATH) {
  const doc = JSON.parse(readFileSync(path, "utf8"));
  const problems = validateConfig(doc);
  if (problems.length > 0) throw new Error(`${path}: ${problems.join("; ")}`);
  return doc;
}

/** `/etc/os-release` as a map; both files are world-readable. */
function readOsRelease() {
  for (const path of ["/etc/os-release", "/usr/lib/os-release"]) {
    if (!existsSync(path)) continue;
    const fields = {};
    for (const line of readFileSync(path, "utf8").split("\n")) {
      const m = /^([A-Z0-9_]+)=(.*)$/.exec(line.trim());
      if (m) fields[m[1]] = m[2].replace(/^(["'])(.*)\1$/, "$2");
    }
    return fields;
  }
  return {};
}

/** The runner's OS image identity: distribution, release and kernel, read without privileges. */
export function readOsImageIdentity(env = process.env) {
  const osRelease = readOsRelease();
  return {
    distribution: osRelease.ID ?? null,
    distributionName: osRelease.NAME ?? null,
    release: osRelease.VERSION_ID ?? null,
    kernel: release(),
    kernelVersion: version(),
    imageOS: env.ImageOS ?? null,
    imageVersion: env.ImageVersion ?? null,
  };
}

function readMachineId() {
  for (const path of ["/etc/machine-id", "/var/lib/dbus/machine-id"]) {
    if (existsSync(path)) return readFileSync(path, "utf8").trim() || null;
  }
  return null;
}

/** The value rerun `rerun` gives each runner-supplied variable. */
export function perturbedRunnerVariables(names, rerun, token) {
  return Object.fromEntries(names.map((name) => [name, `perturbed-${rerun}-${token}`]));
}

/**
 * The environment the command runs under: the allow-listed variables (the
 * runner-supplied ones replaced by the rerun's values), the PATH and toolchain
 * homes it needs to find cargo, and the per-job HOME, TMPDIR and time zone.
 */
export function executionEnvironment({ env, runnerVariables, home, tmp, timeZone }) {
  const exec = {};
  for (const [name, value] of Object.entries(allowListedVariables(env))) {
    if (value !== null) exec[name] = value;
  }
  Object.assign(exec, runnerVariables);
  exec.PATH = env.PATH ?? "/usr/bin:/bin";
  exec.CARGO_HOME = env.CARGO_HOME || join(homedir(), ".cargo");
  exec.RUSTUP_HOME = env.RUSTUP_HOME || join(homedir(), ".rustup");
  exec.HOME = home;
  exec.TMPDIR = tmp;
  exec.TZ = timeZone;
  return exec;
}

function epochSecondsUnder(env, wrapper = []) {
  const argv = [...wrapper, "date", "-u", "+%s"];
  const out = spawnSync(argv[0], argv.slice(1), { env, encoding: "utf8" });
  const seconds = Number.parseInt(out.stdout?.trim() ?? "", 10);
  return out.status === 0 && Number.isFinite(seconds) ? seconds : null;
}

/** Shifts the wall clock by `offsetDays` through libfaketime, leaving the monotonic clock real. */
function applyClockShift(exec, offsetDays, library) {
  if (!library) return { applied: false, reason: "no libfaketime library was provided" };
  if (!existsSync(library)) return { applied: false, reason: `${library} does not exist` };
  const shifted = {
    ...exec,
    LD_PRELOAD: library,
    FAKETIME: `${offsetDays > 0 ? "+" : ""}${offsetDays}d`,
    FAKETIME_DONT_FAKE_MONOTONIC: "1",
  };
  const real = Math.floor(Date.now() / 1000);
  const observed = epochSecondsUnder(shifted);
  if (observed === null) return { applied: false, reason: "the shifted clock could not be read" };
  const drift = Math.abs(observed - real - offsetDays * 86400);
  if (drift > CLOCK_TOLERANCE_SECONDS) {
    return {
      applied: false,
      reason: `the clock read ${observed - real}s away, not ${offsetDays}d`,
    };
  }
  Object.assign(exec, shifted);
  return { applied: true, library };
}

/** Probes the network namespace wrapper; on success returns the argv prefix that applies it. */
function probeNetworkIsolation(exec) {
  if (platform() !== "linux") {
    return { applied: false, reason: `no unprivileged network namespace on ${platform()}` };
  }
  const wrapper = [
    "unshare",
    "--user",
    "--map-root-user",
    "--net",
    "--",
    "sh",
    "-c",
    NETNS_SCRIPT,
    "perturbation-netns",
    String(process.getuid()),
    String(process.getgid()),
  ];
  const probe = [...wrapper, "sh", "-c", "id -u; cat /proc/net/dev"];
  const out = spawnSync(probe[0], probe.slice(1), { env: exec, encoding: "utf8" });
  if (out.status !== 0) {
    const reason = (out.stderr || out.error?.message || `exit ${out.status}`).trim();
    return { applied: false, reason: `unshare failed: ${reason}` };
  }
  const [uid, ...dev] = out.stdout.trim().split("\n");
  const interfaces = dev
    .slice(2)
    .map((line) => line.split(":", 1)[0].trim())
    .filter(Boolean);
  if (uid !== String(process.getuid())) {
    return { applied: false, reason: `the namespace ran as uid ${uid}, not the job's own uid` };
  }
  if (interfaces.join(",") !== "lo") {
    return { applied: false, reason: `the namespace still sees ${interfaces.join(", ")}` };
  }
  return {
    applied: true,
    method: "unprivileged user and network namespace (unshare), loopback only",
    interfaces,
    wrapper,
  };
}

export function runRerun(options, command, env = process.env) {
  const config = readConfig(options.config);
  const rerun = Number.parseInt(options.rerun, 10);
  if (!Number.isInteger(rerun) || rerun < 1 || rerun > config.reruns) {
    throw new Error(`--rerun must be 1..${config.reruns}`);
  }
  if (command.length === 0) throw new Error("run needs a command after --");
  const token = randomBytes(6).toString("hex");
  const jobDir = join(options["work-dir"], `rerun-${rerun}-${token}`);
  const home = join(jobDir, "home");
  const tmp = join(jobDir, "tmp");
  mkdirSync(home, { recursive: true });
  mkdirSync(tmp, { recursive: true });

  const timeZone = config.timeZones[rerun - 1];
  const offsetDays = config.clockOffsetDays[rerun - 1];
  const runnerVariables = perturbedRunnerVariables(config.runnerSuppliedVariables, rerun, token);
  const exec = executionEnvironment({ env, runnerVariables, home, tmp, timeZone });
  const libfaketime = applyClockShift(exec, offsetDays, options["faketime-lib"]);
  const network = probeNetworkIsolation(exec);
  const wrapper = network.applied ? network.wrapper : [];
  delete network.wrapper;

  const uid = process.getuid?.() ?? null;
  const euid = process.geteuid?.() ?? null;
  const realStart = Math.floor(Date.now() / 1000);
  const observedStart = epochSecondsUnder(exec, wrapper);
  const record = {
    schemaVersion: RECORD_SCHEMA_VERSION,
    rerun,
    reruns: config.reruns,
    privileges: { uid, euid, unprivileged: uid !== 0 && euid !== 0, elevation: "none" },
    osImage: readOsImageIdentity(env),
    baseFingerprint: {
      os: `${platform()} ${release()}`,
      osImage: readOsImageIdentity(env),
      architecture: arch(),
      toolchainPins: readToolchainPins(),
      variables: allowListedVariables(env),
    },
    variation: {
      temporaryDirectory: tmp,
      home,
      runnerSuppliedVariables: Object.fromEntries(
        Object.entries(runnerVariables).map(([name, value]) => [
          name,
          { runner: env[name] ?? null, rerun: value },
        ]),
      ),
      host: { hostname: hostname(), machineId: readMachineId() },
      clock: {
        timeZone,
        offsetDays,
        method: libfaketime.applied ? "libfaketime wall-clock offset and TZ" : "TZ only",
        libfaketime,
        realStart: new Date(realStart * 1000).toISOString(),
        observedStart: observedStart === null ? null : new Date(observedStart * 1000).toISOString(),
      },
    },
    networkIsolation: network,
    executionEnvironment: exec,
    command,
    startedAt: new Date().toISOString(),
    endedAt: null,
    exitCode: null,
    signal: null,
  };
  const writeRecord = () => writeFileSync(options.record, `${JSON.stringify(record, null, 2)}\n`);
  writeRecord();
  writeFileSync(
    options["env-snapshot"],
    `${JSON.stringify({ variables: allowListedVariables(exec) }, null, 2)}\n`,
  );

  const argv = [...wrapper, ...command];
  const child = spawnSync(argv[0], argv.slice(1), { env: exec, stdio: "inherit", cwd: REPO_ROOT });
  record.endedAt = new Date().toISOString();
  record.exitCode = child.status;
  record.signal = child.signal;
  writeRecord();
  if (child.error) throw child.error;
  if (child.status === 0 || child.status === NEXTEST_TESTS_FAILED) return 0;
  return child.status ?? 1;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stable(value[k])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

/** Keys whose values differ between two fingerprints, as dotted paths. */
function differingPaths(a, b, prefix = "") {
  if (stable(a) === stable(b)) return [];
  const objects = a && b && typeof a === "object" && typeof b === "object";
  if (!objects || Array.isArray(a) || Array.isArray(b)) return [prefix || "(root)"];
  const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])].sort();
  return keys.flatMap((k) => differingPaths(a[k], b[k], prefix ? `${prefix}.${k}` : k));
}

function loadRerun(input, rerun, problems) {
  const names = rerunArtifacts(rerun);
  const paths = {
    junit: join(input, names.evidence, "junit.xml"),
    metadata: join(input, names.evidence, "junit-metadata.json"),
    record: join(input, names.record, "perturbation-record.json"),
  };
  const missing = Object.entries(paths).filter(([, p]) => !existsSync(p));
  if (missing.length > 0) {
    problems.push(`rerun ${rerun}: no ${missing.map(([k]) => k).join(", ")} was uploaded`);
    return null;
  }
  const junit = readFileSync(paths.junit, "utf8");
  const metadata = readJson(paths.metadata);
  const record = readJson(paths.record);
  for (const problem of checkMetadata(metadata, junit)) {
    problems.push(`rerun ${rerun}: JUnit metadata: ${problem}`);
  }
  if (record.schemaVersion !== RECORD_SCHEMA_VERSION || record.rerun !== rerun) {
    problems.push(`rerun ${rerun}: the perturbation record is not rerun ${rerun}'s`);
  }
  if (record.privileges?.unprivileged !== true) {
    problems.push(`rerun ${rerun}: the job did not run unprivileged`);
  }
  for (const key of ["distribution", "release", "kernel"]) {
    if (!nonEmpty(record.osImage?.[key])) {
      problems.push(`rerun ${rerun}: the OS image identity lacks ${key}`);
    }
  }
  if (record.exitCode !== 0 && record.exitCode !== NEXTEST_TESTS_FAILED) {
    problems.push(`rerun ${rerun}: the suite did not complete (exit ${record.exitCode})`);
  }
  return { rerun, record, metadata, cases: junitTestCases(junit) };
}

function clockKey(record) {
  const clock = record.variation?.clock ?? {};
  return `${clock.timeZone}|${clock.libfaketime?.applied ? clock.offsetDays : 0}`;
}

/** Checks that the reruns tested one tree and fingerprint and each varied every dimension. */
function checkComparable(loaded, problems) {
  const [first, ...rest] = loaded;
  for (const other of rest) {
    const diff = differingPaths(first.record.baseFingerprint, other.record.baseFingerprint);
    if (diff.length > 0) {
      problems.push(
        `reruns ${first.rerun} and ${other.rerun} have different environment fingerprints (${diff.join(", ")})`,
      );
    }
    if (first.metadata.checkout?.tree !== other.metadata.checkout?.tree) {
      problems.push(`reruns ${first.rerun} and ${other.rerun} tested different trees`);
    }
    if (
      stable(first.metadata.runConfiguration?.argv) !==
      stable(other.metadata.runConfiguration?.argv)
    ) {
      problems.push(`reruns ${first.rerun} and ${other.rerun} ran different commands`);
    }
  }
  const dimensions = {
    "temporary directory": (r) => r.variation?.temporaryDirectory,
    "runner-supplied variable values": (r) =>
      stable(
        Object.fromEntries(
          Object.entries(r.variation?.runnerSuppliedVariables ?? {}).map(([k, v]) => [k, v.rerun]),
        ),
      ),
    host: (r) => r.variation?.host?.hostname,
    "clock and time zone": clockKey,
  };
  for (const [name, read] of Object.entries(dimensions)) {
    const values = loaded.map((l) => read(l.record));
    if (!values.every((v) => v !== undefined && v !== null) || !distinct(values)) {
      problems.push(`the reruns did not each vary the ${name}`);
    }
  }
}

function defaultRunUrl(env) {
  const { GITHUB_SERVER_URL: server, GITHUB_REPOSITORY: repo, GITHUB_RUN_ID: id } = env;
  return server && repo && id ? `${server}/${repo}/actions/runs/${id}` : null;
}

/** The per-test comparison and the proposed candidate list for the reruns under `input`. */
export function compareReruns({
  input,
  run,
  config,
  list,
  generatedAt = new Date().toISOString(),
}) {
  const problems = [];
  const loaded = [];
  for (let rerun = 1; rerun <= config.reruns; rerun += 1) {
    const one = loadRerun(input, rerun, problems);
    if (one) loaded.push(one);
  }
  if (loaded.length === config.reruns) checkComparable(loaded, problems);
  const comparable = problems.length === 0;

  const outcomesById = new Map();
  for (const one of loaded) {
    const hermetic = new Map(one.metadata.tests?.map((t) => [t.id, t.hermetic]) ?? []);
    for (const { id, outcome } of one.cases) {
      if (!outcomesById.has(id)) outcomesById.set(id, { outcomes: {}, hermetic: {} });
      const entry = outcomesById.get(id);
      entry.outcomes[one.rerun] = entry.outcomes[one.rerun] ? "duplicate" : outcome;
      entry.hermetic[one.rerun] = hermetic.get(id) ?? null;
    }
  }
  const tests = [...outcomesById.keys()].sort().map((id) => {
    const entry = outcomesById.get(id);
    const outcomes = [];
    const hermetic = [];
    for (let rerun = 1; rerun <= config.reruns; rerun += 1) {
      outcomes.push(entry.outcomes[rerun] ?? "absent");
      hermetic.push(entry.hermetic[rerun] ?? null);
    }
    const unchanged = outcomes.every((o) => o === outcomes[0]) && outcomes[0] !== "absent";
    const excluded = exclusionReason(list, id);
    let reason = null;
    if (!comparable) reason = "the reruns are not comparable";
    else if (!unchanged) reason = "the outcome differs across reruns";
    else if (outcomes[0] !== "passed") reason = `${outcomes[0]} in every rerun`;
    else if (excluded) reason = `excluded: ${excluded}`;
    return { id, outcomes, hermetic, unchanged, candidate: reason === null, reason };
  });

  const candidates = tests.filter((t) => t.candidate).map((t) => t.id);
  const staleExclusions = exclusionPatterns(list).filter(
    (pattern) => !tests.some((t) => new RegExp(pattern, "u").test(t.id)),
  );
  const proposed = {
    schemaVersion: HERMETIC_LIST_SCHEMA_VERSION,
    version: list.version + 1,
    approved: false,
    approvedBy: null,
    approvedOn: null,
    derivedFrom: comparable ? { run, reruns: config.reruns, generatedAt } : null,
    candidates,
    exclusions: list.exclusions,
  };
  const listProblems = validateHermeticList(proposed);
  if (listProblems.length > 0) {
    throw new Error(`the proposed candidate list is malformed: ${listProblems.join("; ")}`);
  }
  const comparison = {
    schemaVersion: COMPARISON_SCHEMA_VERSION,
    workflowRun: run,
    generatedAt,
    reruns: config.reruns,
    comparable,
    problems,
    tree: comparable ? loaded[0].metadata.checkout.tree : null,
    fingerprint: comparable ? loaded[0].record.baseFingerprint : null,
    rerunRecords: loaded.map((one) => {
      const { tests: _tests, ...junitMetadata } = one.metadata;
      return { ...one.record, junitMetadata };
    }),
    summary: {
      tests: tests.length,
      unchanged: tests.filter((t) => t.unchanged).length,
      excluded: tests.filter((t) => t.reason?.startsWith("excluded: ")).length,
      candidates: candidates.length,
    },
    staleExclusions,
    tests,
  };
  return { comparison, proposed };
}

function parseOptions(rest) {
  const options = {};
  let i = 0;
  for (; i < rest.length; i += 2) {
    if (rest[i] === "--") break;
    const key = rest[i];
    if (!key?.startsWith("--") || i + 1 >= rest.length) {
      throw new Error(`malformed argument near ${JSON.stringify(key)}`);
    }
    options[key.slice(2)] = rest[i + 1];
  }
  return { options, command: rest.slice(i + 1) };
}

function main() {
  const [subcommand, ...rest] = process.argv.slice(2);
  const { options, command } = parseOptions(rest);
  if (subcommand === "plan") {
    const config = readConfig(options.config);
    process.stdout.write(
      `${JSON.stringify(Array.from({ length: config.reruns }, (_, i) => i + 1))}\n`,
    );
    return 0;
  }
  if (subcommand === "run") {
    for (const key of ["rerun", "work-dir", "record", "env-snapshot"]) {
      if (!options[key]) throw new Error(`run requires --${key}`);
    }
    return runRerun(options, command);
  }
  if (subcommand === "compare") {
    for (const key of ["input", "out", "proposed"]) {
      if (!options[key]) throw new Error(`compare requires --${key}`);
    }
    const { comparison, proposed } = compareReruns({
      input: options.input,
      run: options.run ?? defaultRunUrl(process.env),
      config: readConfig(options.config),
      list: readHermeticListDocument(options.list),
    });
    writeFileSync(options.out, `${JSON.stringify(comparison, null, 2)}\n`);
    writeFileSync(options.proposed, `${JSON.stringify(proposed, null, 2)}\n`);
    for (const problem of comparison.problems) {
      process.stderr.write(`::error::perturbation: ${problem}\n`);
    }
    for (const pattern of comparison.staleExclusions) {
      process.stderr.write(`::warning::perturbation: exclusion ${pattern} names no test\n`);
    }
    const s = comparison.summary;
    process.stdout.write(
      `${s.tests} tests, ${s.unchanged} unchanged, ${s.excluded} excluded, ${s.candidates} candidates\n`,
    );
    return comparison.comparable ? 0 : 1;
  }
  if (subcommand === "check-list") {
    const path = options.list ?? HERMETIC_LIST_PATH;
    const problems = validateHermeticList(readJson(path));
    for (const problem of problems) process.stderr.write(`::error::${path}: ${problem}\n`);
    return problems.length === 0 ? 0 : 1;
  }
  throw new Error("usage: perturbation.mjs plan|run|compare|check-list ...");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    process.exitCode = main();
  } catch (error) {
    process.stderr.write(`::error::perturbation: ${error.message}\n`);
    process.exitCode = 1;
  }
}
