#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { StringDecoder } from "node:string_decoder";
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
    "  node scripts/provider-ci.mjs run <tsserver|tsgo> [--shard <i>/<n>]\n" +
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
  // The paired real-provider selectors hold almost all of a lane's serial
  // time (hundreds of tests at seconds each), so a sharded run splits them
  // by test name; every other invocation stays whole (see `shardInvocations`).
  return {
    args: [...args, "--", ...libtestArgs],
    label: selector.label,
    package: selector.package,
    split: selector.kind === "regex",
  };
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

/** `i/n` (1 <= i <= n) as `{ index, count }`. */
export function parseShard(text) {
  const found = /^([1-9]\d*)\/([1-9]\d*)$/.exec(String(text));
  if (!found || Number(found[1]) > Number(found[2])) {
    throw new Error(`invalid shard '${text}', expected i/n with 1 <= i <= n`);
  }
  return { index: Number(found[1]), count: Number(found[2]) };
}

/**
 * The 1-based shard that owns `key`. FNV-1a over the UTF-8 bytes: stable
 * across runs, runners and Node versions, so a test keeps its shard and every
 * shard of one lane agrees on the split without talking to the others.
 */
export function shardOf(key, count) {
  let hash = 0x811c9dc5;
  for (const byte of Buffer.from(key, "utf8")) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return (hash % count) + 1;
}

/** The test names a libtest `--list --format=terse` run printed, once each. */
export function parseTerseList(text) {
  const names = [];
  const seen = new Set();
  for (const line of String(text).split(/\r?\n/)) {
    const found = /^(.+): test$/.exec(line);
    if (found && !seen.has(found[1])) {
      seen.add(found[1]);
      names.push(found[1]);
    }
  }
  return names;
}

/** Lists what `invocation` would run, with its own filters, without running it. */
export function listInvocationTests(invocation, spawnCargo = spawn) {
  return new Promise((settle, reject) => {
    const child = spawnCargo("cargo", [...invocation.args, "--list", "--format=terse"], {
      stdio: ["ignore", "pipe", "inherit"],
      windowsHide: true,
    });
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(Buffer.from(chunk)));
    child.on("error", reject);
    child.on("close", (status, signal) => {
      if (status !== 0 || signal) {
        reject(
          new Error(
            `listing ${invocation.label} failed (${signal ? `signal ${signal}` : `exit ${status}`})`,
          ),
        );
        return;
      }
      settle(parseTerseList(Buffer.concat(chunks).toString("utf8")));
    });
  });
}

/**
 * The invocations shard `shard` of `lane` runs. One shard (`1/1`) is the whole
 * lane, unchanged. Across `n` shards every test runs exactly once: a splittable
 * invocation (the paired real-provider variants) is listed and its tests are
 * dealt out, each shard running its share with `--exact`; every other
 * invocation runs whole on the shard its label hashes to. Each shard still
 * runs its share serially, exactly as the unsharded lane does.
 *
 * A splittable invocation that lists zero tests fails the shard: a split of
 * nothing cannot prove the lane ran. A shard whose share of one is empty
 * skips it; it never falls back to an unfiltered `cargo test`, which would run
 * the whole package.
 */
export async function shardInvocations(lane, shard, listTests) {
  const invocations = providerCargoInvocations(lane);
  if (shard.count === 1) return invocations;
  const selected = [];
  for (const invocation of invocations) {
    if (!invocation.split) {
      if (shardOf(invocation.label, shard.count) === shard.index) selected.push(invocation);
      continue;
    }
    const names = await listTests(invocation);
    if (names.length === 0) {
      throw new Error(`${invocation.label}: the listing selected zero tests`);
    }
    // Dealt round-robin over the sorted names: a matrix family's sizes sort
    // together, so its expensive members land on different shards. Every
    // shard lists the same binary, so all of them deal the same hand.
    const mine = [...names]
      .sort()
      .filter((_, position) => position % shard.count === shard.index - 1);
    if (mine.length === 0) continue;
    selected.push({
      args: [
        "test",
        "--locked",
        "--no-fail-fast",
        "-p",
        invocation.package,
        "--",
        "--exact",
        "--test-threads=1",
        ...mine,
      ],
      label: `${invocation.label} (shard ${shard.index}/${shard.count}: ${mine.length} of ${names.length} tests)`,
    });
  }
  return selected;
}

// Stable libtest has no per-test timing format. In serial pretty output it
// flushes the test name before running the body and prints its status afterward.
// These stream-edge durations are advisory: pipe delivery can batch very short
// tests. Cargo setup and captured failure output are outside the measured span.
export function providerTestTimer(record, now = () => performance.now()) {
  let buffer = "";
  let inSuite = false;
  let active = null;
  function begin() {
    if (!inSuite || active) return;
    const header = /^test (.+?)(?: - should panic)? \.\.\. /.exec(buffer);
    if (header) active = { name: header[1], started: now() };
  }
  return {
    push(text) {
      buffer += text;
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        begin();
        const line = buffer.slice(0, newline).replace(/\r$/, "");
        buffer = buffer.slice(newline + 1);
        if (/^running \d+ tests?$/.test(line)) inSuite = true;
        if (/^(?:failures:|test result:)/.test(line)) inSuite = false;
        const result = /^test (.+?)(?: - should panic)? \.\.\. (ok|FAILED|ignored)(?:, .*)?$/.exec(
          line,
        );
        // Libtest's long-running notice inserts a newline between the name
        // and the terminal status. Serial execution still identifies its owner.
        const status = result?.[1] === active?.name ? result?.[2] : /^(ok|FAILED)$/.exec(line)?.[1];
        if (inSuite && active && status) {
          record({ name: active.name, status, elapsedMs: now() - active.started });
          active = null;
        }
        if (!inSuite) active = null;
      }
      begin();
    },
    finish() {
      if (active)
        record({ name: active.name, status: "incomplete", elapsedMs: now() - active.started });
      active = null;
    },
  };
}

export async function runProviderLane(
  lane,
  {
    spawnCargo = spawn,
    stopChild = (child) => {
      killListingTree(child);
      child.kill();
    },
    writeStdout = (chunk) => process.stdout.write(chunk),
    writeStderr = (text) => process.stderr.write(text),
    shard = { index: 1, count: 1 },
    listTests = (invocation) => listInvocationTests(invocation, spawnCargo),
    saveTimings = (timings) => {
      const directory = resolve(process.env.CARGO_TARGET_DIR || "target", "provider-timings");
      const name = shard.count === 1 ? lane : `${lane}-${shard.index}of${shard.count}`;
      mkdirSync(directory, { recursive: true });
      writeFileSync(
        resolve(directory, `${name}.json`),
        JSON.stringify({ lane, shard: `${shard.index}/${shard.count}`, timings }, null, 2),
      );
    },
  } = {},
) {
  let failed = false;
  let interrupted = false;
  const timings = [];
  // An unknown lane is a usage error, not a lane failure.
  providerCargoInvocations(lane);
  let invocations;
  try {
    invocations = await shardInvocations(lane, shard, listTests);
  } catch (error) {
    writeStderr(`PROVIDER CI (${lane} ${shard.index}/${shard.count}): ${error.message}\n`);
    return 1;
  }
  for (const invocation of invocations) {
    writeStderr(`\nPROVIDER CI (${lane}): ${invocation.label}\n`);
    const timer = providerTestTimer((row) => {
      timings.push({ invocation: invocation.label, ...row });
      writeStderr(
        `PROVIDER TEST TIME (${lane}): ${row.name} ${row.status} ${(row.elapsedMs / 1000).toFixed(3)}s\n`,
      );
    });
    const result = await new Promise((settle) => {
      const child = spawnCargo("cargo", [...invocation.args, "--format=pretty", "--color=never"], {
        stdio: ["inherit", "pipe", "inherit"],
        windowsHide: true,
      });
      const decoder = new StringDecoder("utf8");
      let spawnError;
      const forward = () => {
        interrupted = true;
        stopChild(child);
      };
      process.once("SIGINT", forward);
      process.once("SIGTERM", forward);
      child.stdout.on("data", (chunk) => {
        writeStdout(chunk);
        timer.push(decoder.write(chunk));
      });
      child.on("error", (error) => {
        spawnError = error;
      });
      child.on("close", (status, signal) => {
        process.off("SIGINT", forward);
        process.off("SIGTERM", forward);
        timer.push(decoder.end());
        timer.finish();
        settle({ status, signal, error: spawnError });
      });
    });
    if (result.error) {
      writeStderr(`PROVIDER CI: could not start cargo: ${result.error.message}\n`);
      failed = true;
      if (interrupted) break;
      continue;
    }
    if (result.signal) {
      writeStderr(`PROVIDER CI: cargo was killed by ${result.signal}\n`);
      failed = true;
      if (interrupted) break;
      continue;
    }
    if (result.status !== 0) failed = true;
    if (interrupted) break;
  }
  // Timing persistence cannot change the test verdict.
  try {
    saveTimings(timings);
  } catch (error) {
    writeStderr(`PROVIDER TEST TIME unavailable: ${error.message}\n`);
  }
  return failed || interrupted ? 1 : 0;
}

export function main(args = process.argv.slice(2)) {
  if (args[0] === "filter" && args.length === 2) {
    if (!PROVIDER_CI_LANES.includes(args[1])) return fail(`unknown lane '${args[1]}'\n${usage()}`);
    process.stdout.write(buildProviderLaneFilterExpr(args[1]));
    return 0;
  }
  const sharded = args[0] === "run" && args.length === 4 && args[2] === "--shard";
  if (args[0] === "run" && (args.length === 2 || sharded)) {
    try {
      const shard = sharded ? parseShard(args[3]) : { index: 1, count: 1 };
      return runProviderLane(args[1], { shard }).catch((error) =>
        fail(`${error.message}\n${usage()}`),
      );
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
