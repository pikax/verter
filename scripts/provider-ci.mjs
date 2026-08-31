#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  PROVIDER_CI_LANES,
  buildProviderLaneFilterExpr,
  verifyProviderCiPartition,
} from "./provider-ci-internals.mjs";

function usage() {
  return (
    "Usage:\n" +
    "  node scripts/provider-ci.mjs filter <core|tsserver|tsgo>\n" +
    "  node scripts/provider-ci.mjs verify --archive-file <path>\n"
  );
}

function fail(message, code = 127) {
  process.stderr.write(`PROVIDER CI PARTITION: ${message}\n`);
  return code;
}

function archivePath(args) {
  const index = args.indexOf("--archive-file");
  if (index < 0 || !args[index + 1] || index + 2 !== args.length) return null;
  return resolve(args[index + 1]);
}

function verifyArchive(args) {
  const archive = archivePath(args);
  if (!archive) return fail(`verify requires exactly --archive-file <path>\n${usage()}`);
  const listed = spawnSync(
    "cargo",
    ["nextest", "list", "--archive-file", archive, "--message-format", "json"],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, windowsHide: true },
  );
  if (listed.stderr) process.stderr.write(listed.stderr);
  if (listed.error) return fail(`could not start cargo nextest list: ${listed.error.message}`);
  if (listed.signal) return fail(`cargo nextest list was killed by ${listed.signal}`);
  if (listed.status !== 0)
    return fail(`cargo nextest list exited with ${listed.status}`, listed.status || 1);

  let parsed;
  try {
    parsed = JSON.parse(listed.stdout);
  } catch (error) {
    return fail(`cargo nextest list returned invalid JSON: ${error.message}`);
  }
  const verdict = verifyProviderCiPartition(parsed);
  if (!verdict.ok) {
    for (const error of verdict.errors) process.stderr.write(`PROVIDER CI PARTITION: ${error}\n`);
    return 127;
  }
  process.stderr.write(
    `Provider CI partition admitted one disjoint canonical inventory: ` +
      `core=${verdict.counts.core}, tsserver=${verdict.counts.tsserver}, tsgo=${verdict.counts.tsgo}.\n`,
  );
  return 0;
}

export function main(args = process.argv.slice(2)) {
  if (args[0] === "filter" && args.length === 2) {
    if (!PROVIDER_CI_LANES.includes(args[1])) return fail(`unknown lane '${args[1]}'\n${usage()}`);
    process.stdout.write(buildProviderLaneFilterExpr(args[1]));
    return 0;
  }
  if (args[0] === "verify") return verifyArchive(args.slice(1));
  return fail(usage());
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  process.exitCode = main();
}
