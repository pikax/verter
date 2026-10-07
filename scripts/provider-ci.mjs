#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  PROVIDER_CI_LANES,
  PROVIDER_LIVE_SELECTORS,
  buildProviderLaneFilterExpr,
  verifyProviderCiPartition,
} from "./provider-ci-internals.mjs";

function usage() {
  return (
    "Usage:\n" +
    "  node scripts/provider-ci.mjs filter <core|tsserver|tsgo>\n" +
    "  node scripts/provider-ci.mjs run <tsserver|tsgo>\n" +
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

// Listing an archive runs every test binary with `--list`. Healthy listings
// finish in well under two minutes; the bound turns a binary that never
// returns into a prompt, named failure instead of a job-budget timeout.
export const LIST_TIMEOUT_MS = 10 * 60 * 1000;
const EXIT_LIST_TIMEOUT = 124;

// POSIX only. Parentage, not process group: a descendant that started its own
// session is still found.
function descendantPids(rootPid) {
  const children = new Map();
  const table = spawnSync("ps", ["-A", "-o", "pid=,ppid="], { encoding: "utf8", timeout: 10_000 });
  for (const line of (table.stdout || "").split("\n")) {
    const [pid, ppid] = line.trim().split(/\s+/).map(Number);
    if (!pid) continue;
    if (!children.has(ppid)) children.set(ppid, []);
    children.get(ppid).push(pid);
  }
  const found = [];
  const pending = [rootPid];
  while (pending.length > 0) {
    for (const child of children.get(pending.pop()) || []) {
      found.push(child);
      pending.push(child);
    }
  }
  return { pids: found, children };
}

function readProc(path) {
  try {
    return readFileSync(path, "utf8").trim();
  } catch (error) {
    return `<${error.code || error.message}>`;
  }
}

function captureCommand(command, args, timeout) {
  const run = spawnSync(command, args, { encoding: "utf8", timeout, windowsHide: true });
  if (run.error) return `<${command} unavailable: ${run.error.message}>`;
  return `${run.stdout || ""}${run.stderr || ""}`.trim();
}

// Names the binary that is still listing and where its threads are blocked.
// Linux only; every probe is best effort and bounded.
function linuxListingDiagnostics(rootPid) {
  const { pids, children } = descendantPids(rootPid);
  const lines = [`still-running processes under cargo (pid ${rootPid}):`];
  lines.push(
    captureCommand(
      "ps",
      ["-o", "pid,ppid,etime,stat,wchan:32,args", "-p", [rootPid, ...pids].join(",")],
      10_000,
    ),
  );
  const sudo = spawnSync("sudo", ["-n", "true"], { timeout: 5_000 }).status === 0;
  const leaves = pids.filter((pid) => !children.has(pid)).slice(0, 4);
  for (const pid of leaves) {
    lines.push(`--- pid ${pid}: ${readProc(`/proc/${pid}/cmdline`).replaceAll("\0", " ").trim()}`);
    let tasks = [];
    try {
      tasks = readdirSync(`/proc/${pid}/task`).slice(0, 64);
    } catch {}
    for (const tid of tasks) {
      const task = `/proc/${pid}/task/${tid}`;
      lines.push(
        `  thread ${tid} (${readProc(`${task}/comm`)}) wchan=${readProc(`${task}/wchan`)}`,
      );
    }
    // Kernel stacks need privilege; one bounded read covers every thread.
    const stacks = tasks.map((tid) => `/proc/${pid}/task/${tid}/stack`);
    if (sudo && stacks.length > 0)
      lines.push(captureCommand("sudo", ["-n", "head", "-n", "32", ...stacks], 10_000));
    const gdb = ["-batch", "-nx", "-p", String(pid), "-ex", "thread apply all bt 40"];
    lines.push(
      sudo
        ? captureCommand("sudo", ["-n", "gdb", ...gdb], 60_000)
        : captureCommand("gdb", gdb, 60_000),
    );
  }
  return lines.join("\n");
}

function killListingTree(child) {
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/T", "/F", "/PID", String(child.pid)], { windowsHide: true });
    return;
  }
  // Collected before any kill: once cargo dies its children are reparented.
  const descendants = descendantPids(child.pid).pids;
  for (const pid of [-child.pid, ...descendants]) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
}

function listArchive(archive, { cargo, cargoArgsPrefix, listTimeoutMs, write }) {
  return new Promise((settle) => {
    const child = spawn(
      cargo,
      [
        ...cargoArgsPrefix,
        "nextest",
        "list",
        "--archive-file",
        archive,
        "--message-format",
        "json",
      ],
      // Its own process group, so the whole listing tree is reaped on timeout.
      {
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      },
    );
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    const forward = (signal) => {
      killListingTree(child);
      process.exit(128 + (signal === "SIGINT" ? 2 : 15));
    };
    process.once("SIGINT", forward);
    process.once("SIGTERM", forward);
    const timer = setTimeout(() => {
      timedOut = true;
      write(
        `PROVIDER CI PARTITION: a test binary did not return from --list within ` +
          `${listTimeoutMs / 1000}s (cargo nextest list, pid ${child.pid}); reaping the listing.\n`,
      );
      if (process.platform === "linux") write(`${linuxListingDiagnostics(child.pid)}\n`);
      killListingTree(child);
      // A descendant outside the process group could still hold the pipes open.
      child.stdout.destroy();
      child.stderr.destroy();
    }, listTimeoutMs);
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    const finish = (result) => {
      clearTimeout(timer);
      process.off("SIGINT", forward);
      process.off("SIGTERM", forward);
      settle({
        ...result,
        timedOut,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      });
    };
    child.on("error", (error) => finish({ error }));
    child.on("close", (status, signal) => finish({ status, signal }));
  });
}

export async function verifyArchive(
  args,
  {
    cargo = "cargo",
    cargoArgsPrefix = [],
    listTimeoutMs = LIST_TIMEOUT_MS,
    write = (text) => process.stderr.write(text),
  } = {},
) {
  const reject = (message, code = 127) => {
    write(`PROVIDER CI PARTITION: ${message}\n`);
    return code;
  };
  const archive = archivePath(args);
  if (!archive) return reject(`verify requires exactly --archive-file <path>\n${usage()}`);
  const listed = await listArchive(archive, { cargo, cargoArgsPrefix, listTimeoutMs, write });
  if (listed.stderr) write(listed.stderr);
  if (listed.timedOut)
    return reject(
      `cargo nextest list did not complete within ${listTimeoutMs / 1000}s: a test binary did not return from --list`,
      EXIT_LIST_TIMEOUT,
    );
  if (listed.error) return reject(`could not start cargo nextest list: ${listed.error.message}`);
  if (listed.signal) return reject(`cargo nextest list was killed by ${listed.signal}`);
  if (listed.status !== 0)
    return reject(`cargo nextest list exited with ${listed.status}`, listed.status || 1);

  let parsed;
  try {
    parsed = JSON.parse(listed.stdout);
  } catch (error) {
    return reject(`cargo nextest list returned invalid JSON: ${error.message}`);
  }
  const verdict = verifyProviderCiPartition(parsed);
  if (!verdict.ok) {
    for (const error of verdict.errors) write(`PROVIDER CI PARTITION: ${error}\n`);
    return 127;
  }
  write(
    `Provider CI partition admitted one disjoint canonical inventory: ` +
      `core=${verdict.counts.core}, tsserver=${verdict.counts.tsserver}, tsgo=${verdict.counts.tsgo}.\n`,
  );
  return 0;
}

function selectorCargoInvocation(selector) {
  const args = ["test", "--locked", "--no-fail-fast", "-p", selector.package];
  const libtestArgs = ["--test-threads=1"];
  switch (selector.kind) {
    case "package":
      break;
    case "prefix":
      args.push(selector.value);
      break;
    case "regex":
      args.push("real_provider_tests::");
      libtestArgs.push("--skip", selector.lane === "tsserver" ? "_tsgo" : "_tsserver");
      break;
    case "exact":
      throw new Error("exact selectors expand to one cargo invocation per test");
    default:
      throw new Error(`unknown provider CI selector kind: ${selector.kind}`);
  }
  return { args: [...args, "--", ...libtestArgs], label: selector.label };
}

export function providerCargoInvocations(lane) {
  if (!PROVIDER_CI_LANES.includes(lane) || lane === "core") {
    throw new Error(`provider runner requires a live lane, got '${lane}'`);
  }
  return PROVIDER_LIVE_SELECTORS.filter((selector) => selector.lane === lane).flatMap(
    (selector) => {
      if (selector.kind !== "exact") return [selectorCargoInvocation(selector)];
      return selector.values.map((testName) => ({
        args: [
          "test",
          "--locked",
          "--no-fail-fast",
          "-p",
          selector.package,
          testName,
          "--",
          "--exact",
          "--test-threads=1",
        ],
        label: `${selector.label}: ${testName}`,
      }));
    },
  );
}

function runProviderLane(lane) {
  let failed = false;
  for (const invocation of providerCargoInvocations(lane)) {
    process.stderr.write(`\nPROVIDER CI (${lane}): ${invocation.label}\n`);
    const result = spawnSync("cargo", invocation.args, {
      stdio: "inherit",
      windowsHide: true,
    });
    if (result.error) {
      process.stderr.write(`PROVIDER CI: could not start cargo: ${result.error.message}\n`);
      failed = true;
      continue;
    }
    if (result.signal) {
      process.stderr.write(`PROVIDER CI: cargo was killed by ${result.signal}\n`);
      failed = true;
      continue;
    }
    if (result.status !== 0) failed = true;
  }
  return failed ? 1 : 0;
}

export function main(args = process.argv.slice(2)) {
  if (args[0] === "filter" && args.length === 2) {
    if (!PROVIDER_CI_LANES.includes(args[1])) return fail(`unknown lane '${args[1]}'\n${usage()}`);
    process.stdout.write(buildProviderLaneFilterExpr(args[1]));
    return 0;
  }
  if (args[0] === "run" && args.length === 2) {
    try {
      return runProviderLane(args[1]);
    } catch (error) {
      return fail(`${error.message}\n${usage()}`);
    }
  }
  if (args[0] === "verify") return verifyArchive(args.slice(1));
  return fail(usage());
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  process.exitCode = await main();
}
