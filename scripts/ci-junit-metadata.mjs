#!/usr/bin/env node

// Metadata document (schema v1) written beside each nextest `ci`-profile
// junit.xml. It names the tested tree and its parents, the toolchain, the
// environment fingerprint, the run configuration and each test's hermetic flag
// so the JUnit report is more than an anonymous observation.
//
//   node scripts/ci-junit-metadata.mjs snapshot-env --out <env.json>
//        run by the step that executes nextest, in that step's shell, so the
//        recorded variables are the ones the tests ran under
//   node scripts/ci-junit-metadata.mjs write --junit <junit.xml> --out <metadata.json>
//        --env-snapshot <env.json> --profile <name>
//        (--argv-json <cargo argv json> | --command <cmd> [--features <list>] [--filter <expr>])
//        [--partition <spec>] [--matrix-json <json>] [--tools rustc,cargo,...]
//   node scripts/ci-junit-metadata.mjs check --junit <junit.xml> --metadata <metadata.json>
//
// `check` exits non-zero when a required field is missing or the test list does
// not match the junit.xml, so a CI step running it fails the job.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { arch, platform, release } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const SCHEMA_VERSION = 1;

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const HERMETIC_LIST = join(REPO_ROOT, "scripts", "manifests", "ci-hermetic-tests.json");

// Only these variables enter the fingerprint: names are always recorded, a value
// only when set. Anything not named here (tokens, secrets) is never read.
export const ENV_ALLOWLIST = [
  "CI",
  "CARGO_INCREMENTAL",
  "CARGO_LLVM_COV",
  "CARGO_TARGET_DIR",
  "CARGO_TERM_COLOR",
  "NEXTEST_PROFILE",
  "LLVM_PROFILE_FILE",
  "LLVM_PROFILE_FILE_NAME",
  "RUSTDOCFLAGS",
  "RUSTFLAGS",
  "RUSTUP_TOOLCHAIN",
  "RUST_BACKTRACE",
  "SCCACHE_GHA_ENABLED",
  "SCCACHE_GHA_VERSION",
  "ImageOS",
  "ImageVersion",
];

/** Tools every nextest job runs, so every document must carry their versions. */
export const REQUIRED_TOOLS = ["rustc", "cargo", "cargo-nextest", "node", "pnpm"];

const TOOL_COMMANDS = {
  rustc: ["rustc", ["--version", "--verbose"]],
  cargo: ["cargo", ["--version"]],
  "cargo-nextest": ["cargo", ["nextest", "--version"]],
  node: ["node", ["--version"]],
  pnpm: ["pnpm", ["--version"]],
};

function run(file, args, cwd = REPO_ROOT) {
  return execFileSync(file, args, { cwd, encoding: "utf8" }).trim();
}

/** The allow-listed variables of `env`: every name, its value when set, else null. */
export function allowListedVariables(env) {
  return Object.fromEntries(ENV_ALLOWLIST.map((name) => [name, env[name] ?? null]));
}

function readEnvSnapshot(path) {
  const doc = JSON.parse(readFileSync(path, "utf8"));
  const variables = doc?.variables;
  const names = variables && typeof variables === "object" ? Object.keys(variables) : [];
  if (names.length !== ENV_ALLOWLIST.length || !ENV_ALLOWLIST.every((n) => names.includes(n))) {
    throw new Error(`${path}: expected {variables: {${ENV_ALLOWLIST.join(", ")}}}`);
  }
  return variables;
}

/** Command, cargo features and filter as the run passed them, read from its cargo argument vector. */
export function describeArgv(argv) {
  if (!Array.isArray(argv) || argv.length === 0 || !argv.every((a) => typeof a === "string")) {
    throw new Error("--argv-json must be a non-empty JSON array of strings");
  }
  const valueOf = (flag) => {
    const at = argv.indexOf(flag);
    return at >= 0 && at + 1 < argv.length ? argv[at + 1] : undefined;
  };
  return {
    command: ["cargo", ...argv].join(" "),
    features: valueOf("--features"),
    filter: valueOf("-E"),
    partition: valueOf("--partition"),
  };
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const options = {};
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i];
    if (!key?.startsWith("--") || i + 1 >= rest.length) {
      throw new Error(`malformed argument near ${JSON.stringify(key)}`);
    }
    options[key.slice(2)] = rest[i + 1];
  }
  return { command, options };
}

function xmlUnescape(text) {
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n)))
    .replace(/&#x([0-9a-fA-F]+);/g, (_, n) => String.fromCodePoint(parseInt(n, 16)))
    .replace(/&amp;/g, "&");
}

/** Test ids (`<binary id> <test name>`) of every testcase in a nextest junit.xml. */
export function junitTestIds(xml) {
  const ids = [];
  for (const tag of xml.matchAll(/<testcase\b([^>]*)>/g)) {
    const attr = (name) => {
      const m = new RegExp(`\\b${name}="([^"]*)"`).exec(tag[1]);
      return m ? xmlUnescape(m[1]) : "";
    };
    ids.push(`${attr("classname")} ${attr("name")}`);
  }
  return ids;
}

function readHermeticList() {
  const doc = JSON.parse(readFileSync(HERMETIC_LIST, "utf8"));
  if (doc.schemaVersion !== SCHEMA_VERSION || !Array.isArray(doc.hermetic)) {
    throw new Error(`${HERMETIC_LIST}: expected {schemaVersion: 1, hermetic: [...]}`);
  }
  return new Set(doc.hermetic);
}

/** The checked-out commit's tree and parents, read from the raw commit object so a shallow clone still reports them. */
export function readCheckout(cwd = REPO_ROOT) {
  const commit = run("git", ["rev-parse", "HEAD"], cwd);
  const raw = run("git", ["cat-file", "-p", "HEAD"], cwd);
  const parents = raw
    .split("\n\n", 1)[0]
    .split("\n")
    .filter((line) => line.startsWith("parent "))
    .map((line) => line.slice("parent ".length));
  return { commit, tree: run("git", ["rev-parse", "HEAD^{tree}"], cwd), parents };
}

function readToolVersions(names) {
  const versions = {};
  for (const name of names) {
    const spec = TOOL_COMMANDS[name];
    if (!spec) throw new Error(`unknown tool ${JSON.stringify(name)}`);
    versions[name] = run(spec[0], spec[1]);
  }
  return versions;
}

function readToolchainPins() {
  const rustToolchain = readFileSync(join(REPO_ROOT, "rust-toolchain.toml"), "utf8");
  const pkg = JSON.parse(readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  return {
    rust: /^\s*channel\s*=\s*"([^"]+)"/m.exec(rustToolchain)?.[1] ?? null,
    node: readFileSync(join(REPO_ROOT, ".nvmrc"), "utf8").trim(),
    pnpm: typeof pkg.packageManager === "string" ? pkg.packageManager.split("+")[0] : null,
  };
}

export function buildMetadata(options, env = process.env) {
  const junit = readFileSync(options.junit, "utf8");
  const hermetic = readHermeticList();
  const workflowRef = env.GITHUB_WORKFLOW_REF ?? "";
  const toolNames = (options.tools ?? REQUIRED_TOOLS.join(",")).split(",").filter(Boolean);
  const argv = options["argv-json"] ? describeArgv(JSON.parse(options["argv-json"])) : null;
  const command = argv?.command ?? options.command ?? "";
  const filter = argv ? argv.filter : options.filter;
  const features = argv ? argv.features : options.features;
  return {
    schemaVersion: SCHEMA_VERSION,
    checkout: { ...readCheckout(), event: env.GITHUB_EVENT_NAME ?? "none" },
    tools: readToolVersions(toolNames),
    environment: {
      os: `${platform()} ${release()}`,
      runnerOs: env.RUNNER_OS ?? "none",
      architecture: arch(),
      runnerImageDigest: env.RUNNER_IMAGE_DIGEST || "none",
      toolchainPins: readToolchainPins(),
      variables: readEnvSnapshot(options["env-snapshot"]),
    },
    runConfiguration: {
      workflowPath: workflowRef.split("@", 1)[0].split("/").slice(2).join("/") || "none",
      workflow: env.GITHUB_WORKFLOW ?? "none",
      job: env.GITHUB_JOB ?? "none",
      matrix: options["matrix-json"] ? JSON.parse(options["matrix-json"]) : {},
      command,
      cargoFeatures: features ?? "none",
      nextestProfile: options.profile ?? "",
      testFilter: filter ?? "none",
      partition: options.partition ?? argv?.partition ?? "none",
    },
    tests: junitTestIds(junit).map((id) => ({ id, hermetic: hermetic.has(id) })),
  };
}

const nonEmpty = (v) => typeof v === "string" && v.trim() !== "";

/** Every violation of the schema; empty when the document is complete and matches the junit.xml. */
export function checkMetadata(meta, junitXml) {
  const problems = [];
  const need = (path, ok) => ok || problems.push(`missing or empty ${path}`);
  need("schemaVersion", meta?.schemaVersion === SCHEMA_VERSION);
  const c = meta?.checkout ?? {};
  need("checkout.commit", /^[0-9a-f]{40,64}$/.test(c.commit ?? ""));
  need("checkout.tree", /^[0-9a-f]{40,64}$/.test(c.tree ?? ""));
  need("checkout.event", nonEmpty(c.event));
  need(
    "checkout.parents",
    Array.isArray(c.parents) &&
      c.parents.length > 0 &&
      c.parents.every((p) => /^[0-9a-f]{40,64}$/.test(p)),
  );
  if (c.event === "pull_request") {
    need("checkout.parents (a pull_request merge commit has exactly two)", c.parents?.length === 2);
  }
  const tools = meta?.tools ?? {};
  for (const name of REQUIRED_TOOLS) need(`tools.${name}`, nonEmpty(tools[name]));
  const e = meta?.environment ?? {};
  for (const key of ["os", "runnerOs", "architecture", "runnerImageDigest"]) {
    need(`environment.${key}`, nonEmpty(e[key]));
  }
  for (const key of ["rust", "node", "pnpm"])
    need(`environment.toolchainPins.${key}`, nonEmpty(e.toolchainPins?.[key]));
  for (const name of ENV_ALLOWLIST) {
    need(`environment.variables.${name}`, e.variables && Object.hasOwn(e.variables, name));
  }
  const r = meta?.runConfiguration ?? {};
  for (const key of [
    "workflowPath",
    "workflow",
    "job",
    "command",
    "cargoFeatures",
    "nextestProfile",
    "testFilter",
    "partition",
  ]) {
    need(`runConfiguration.${key}`, nonEmpty(r[key]));
  }
  need("runConfiguration.matrix", r.matrix !== null && typeof r.matrix === "object");
  need("runConfiguration.nextestProfile is 'ci'", r.nextestProfile === "ci");
  need(
    "environment.variables.NEXTEST_PROFILE equals runConfiguration.nextestProfile",
    e.variables?.NEXTEST_PROFILE === r.nextestProfile,
  );
  need(
    "runConfiguration.testFilter (the command passes -E, so a filter is required)",
    !/(^|\s)-E(\s|$)/.test(r.command ?? "") || (nonEmpty(r.testFilter) && r.testFilter !== "none"),
  );
  const tests = meta?.tests;
  if (
    !Array.isArray(tests) ||
    !tests.every((t) => nonEmpty(t?.id) && typeof t.hermetic === "boolean")
  ) {
    problems.push("tests: every entry needs an id and a boolean hermetic flag");
  } else {
    const expected = junitTestIds(junitXml);
    if (expected.length === 0) problems.push("junit.xml holds no testcase");
    if (expected.length !== tests.length || expected.some((id, i) => id !== tests[i].id)) {
      problems.push("tests do not match the junit.xml testcases");
    }
    const approved = readHermeticList();
    if (tests.some((t) => t.hermetic !== approved.has(t.id))) {
      problems.push("a hermetic flag disagrees with the operator-approved hermetic list");
    }
  }
  return problems;
}

function main() {
  const { command, options } = parseArgs(process.argv.slice(2));
  if (command === "snapshot-env") {
    if (!options.out) throw new Error("snapshot-env requires --out");
    writeFileSync(
      options.out,
      `${JSON.stringify({ variables: allowListedVariables(process.env) }, null, 2)}\n`,
    );
    return 0;
  }
  if (command === "write") {
    for (const key of ["junit", "out", "profile", "env-snapshot"]) {
      if (!options[key]) throw new Error(`write requires --${key}`);
    }
    if (!options["argv-json"] && !options.command) {
      throw new Error("write requires --argv-json or --command");
    }
    writeFileSync(options.out, `${JSON.stringify(buildMetadata(options), null, 2)}\n`);
    return 0;
  }
  if (command === "check") {
    const problems = checkMetadata(
      JSON.parse(readFileSync(options.metadata, "utf8")),
      readFileSync(options.junit, "utf8"),
    );
    for (const problem of problems) process.stderr.write(`::error::JUnit metadata: ${problem}\n`);
    return problems.length === 0 ? 0 : 1;
  }
  throw new Error("usage: ci-junit-metadata.mjs snapshot-env|write|check ...");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    process.exitCode = main();
  } catch (error) {
    process.stderr.write(`::error::JUnit metadata: ${error.message}\n`);
    process.exitCode = 1;
  }
}
