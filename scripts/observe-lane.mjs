#!/usr/bin/env node

/**
 * observe-lane.mjs — the instrumentation test lane.
 *
 * Audit capture, footprints, timing and the other optional observations
 * compile only under the default-off `semantic-observe` feature (see
 * docs/arch/semantic-observe.md). The canonical gate and the core CI shards
 * build without it, so the suites that read a captured record, a footprint or
 * a measurement are compiled out there and run here instead.
 *
 * The lane's package set is DERIVED from `cargo metadata`: every workspace
 * member that declares a `semantic-observe` feature is tested with that
 * feature switched on. A crate that gains the feature joins the lane without a
 * hand-maintained list. The test selection is the provider-free core filter,
 * the same one the core shards use, so real-provider tests stay in their own
 * lifecycle jobs.
 *
 * Foreign-crate compile fixtures type-check against metadata staged from the
 * exact build under test, so `run` first prepares that metadata
 * (`scripts/compile-fixture-artifacts.mjs prepare`) with the lane's own
 * package and feature selection — the same build the tests then run from.
 *
 * Usage:
 *   node scripts/observe-lane.mjs args   # print the nextest arguments
 *   node scripts/observe-lane.mjs run [extra nextest args...]
 */

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

import { buildProviderLaneFilterExpr } from "./provider-ci-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

export const OBSERVE_FEATURE = "semantic-observe";

/**
 * Workspace members, from a `cargo metadata --no-deps` document, that declare
 * the observation feature. Sorted so the argument list is stable.
 */
export function observeLanePackages(metadata) {
  const members = new Set(metadata.workspace_members ?? []);
  return metadata.packages
    .filter((pkg) => members.has(pkg.id))
    .filter((pkg) => Object.hasOwn(pkg.features ?? {}, OBSERVE_FEATURE))
    .map((pkg) => pkg.name)
    .sort();
}

/**
 * The package and feature selection that defines the lane's build. Fails
 * closed on an empty package set: a lane that selects nothing would report a
 * vacuous pass.
 */
export function buildObserveLaneBuildArgs(metadata) {
  const packages = observeLanePackages(metadata);
  if (packages.length === 0) {
    throw new Error(`no workspace member declares the '${OBSERVE_FEATURE}' feature`);
  }
  return [
    ...packages.flatMap((name) => ["-p", name]),
    "--features",
    packages.map((name) => `${name}/${OBSERVE_FEATURE}`).join(","),
  ];
}

/** The nextest arguments for the lane, over the lane's build selection. */
export function buildObserveLaneArgs(metadata, filterExpr) {
  return [
    "nextest",
    "run",
    "--no-fail-fast",
    ...buildObserveLaneBuildArgs(metadata),
    "-E",
    filterExpr,
  ];
}

function readMetadata() {
  const out = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--locked"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (out.status !== 0) {
    throw new Error(`cargo metadata failed:\n${out.stderr}`);
  }
  return JSON.parse(out.stdout);
}

function usage() {
  return "Usage:\n  node scripts/observe-lane.mjs args\n  node scripts/observe-lane.mjs run [extra nextest args...]\n";
}

export function main(argv) {
  const [command, ...rest] = argv;
  if (command !== "args" && command !== "run") {
    process.stderr.write(usage());
    return 2;
  }
  const metadata = readMetadata();
  const args = buildObserveLaneArgs(metadata, buildProviderLaneFilterExpr("core"));
  if (command === "args") {
    if (rest.length !== 0) {
      process.stderr.write(usage());
      return 2;
    }
    process.stdout.write(`${JSON.stringify(args)}\n`);
    return 0;
  }
  const prepare = spawnSync(
    process.execPath,
    [
      resolve(SCRIPT_DIR, "compile-fixture-artifacts.mjs"),
      "prepare",
      ...buildObserveLaneBuildArgs(metadata),
    ],
    { cwd: REPO_ROOT, stdio: "inherit" },
  );
  if (prepare.error) throw prepare.error;
  if (prepare.status !== 0) return prepare.status ?? 1;
  const run = spawnSync("cargo", [...args, ...rest], { cwd: REPO_ROOT, stdio: "inherit" });
  if (run.error) throw run.error;
  return run.status ?? 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exit(main(process.argv.slice(2)));
}
