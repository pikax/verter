#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EXIT_MEMORY,
  IS_WINDOWS,
  MEMORY_KILL_GRACE_MS,
  buildCargoEnv,
  deriveGateResourceLimits,
  mapStepReason,
  parseMemorySize,
  parsePosixProcessTableRss,
  parseWindowsProcessTableRss,
  runContainedStep,
} from "./gate-internals.mjs";

const MiB = 1024 ** 2;
const GiB = 1024 ** 3;

assert.equal(parseMemorySize("12288MiB"), 12 * GiB);
assert.equal(parseMemorySize("12GiB"), 12 * GiB);
assert.equal(parseMemorySize("1.5GiB"), 1.5 * GiB);
assert.throws(() => parseMemorySize("0"), /positive/);
assert.throws(() => parseMemorySize("12GB"), /MiB|GiB/);

assert.deepEqual(deriveGateResourceLimits({ cpuCount: 8, totalMemBytes: 24 * GiB }), {
  buildJobs: 4,
  testThreads: 4,
  memoryLimitBytes: 12 * GiB,
});
assert.deepEqual(
  deriveGateResourceLimits({
    cpuCount: 8,
    totalMemBytes: 24 * GiB,
    buildJobs: 2,
    testThreads: 3,
    memoryLimitBytes: 8 * GiB,
  }),
  { buildJobs: 2, testThreads: 3, memoryLimitBytes: 8 * GiB },
);

const cargoEnv = buildCargoEnv(
  { PATH: "/usr/bin", CARGO_BUILD_JOBS: "99" },
  "/tmp/verter-gate-target",
  false,
  4,
);
assert.equal(cargoEnv.CARGO_BUILD_JOBS, "4");

// `ps -axo pid=,ppid=,rss=` rows: root (100), a direct child (101), a GRANDCHILD (102, ppid=101, not
// ppid=100) proving the walk is recursive rather than one hop deep, and an unrelated process (103) that
// must stay excluded. This is also the exact shape of the bug this tree walk fixes: nextest reassigns each
// executing test to its OWN process group, so a process-group-membership sum (the prior implementation)
// would miss 101/102 entirely despite them being real descendants — only parent-pid ancestry is reliable.
const posix = parsePosixProcessTableRss(
  [" 100 1 1024", " 101 100 2048", " 102 101 4096", " 103 999 8192"].join("\n"),
  100,
);
assert.deepEqual(posix, { ok: true, rssBytes: 7 * MiB, processCount: 3 });

const windows = parseWindowsProcessTableRss(
  ["100\t1\t1048576", "101\t100\t2097152", "102\t101\t4194304", "999\t1\t8388608"].join("\n"),
  100,
);
assert.deepEqual(windows, { ok: true, rssBytes: 7 * MiB, processCount: 3 });

const targetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-"));
try {
  const result = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", "setInterval(() => {}, 1000)"],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir,
    memoryLimitBytes: 1 * MiB,
    memoryPollMs: 50,
  });
  assert.equal(result.reason, "MEMORY");
  assert.equal(result.reapConfirmedDead, true);
  assert.ok(result.peakRssBytes > result.memoryLimitBytes);
  assert.equal(mapStepReason(result), EXIT_MEMORY);

  const unavailable = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", "setInterval(() => {}, 1000)"],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir,
    memoryLimitBytes: 1 * GiB,
    memoryPollMs: 50,
    memorySampleFailureLimit: 2,
    memorySampler: () => ({ ok: false, detail: "injected sampler outage" }),
  });
  assert.equal(unavailable.reason, "MEMORY_MONITOR");
  assert.equal(unavailable.reapConfirmedDead, true);
  assert.equal(mapStepReason(unavailable), EXIT_MEMORY);
} finally {
  rmSync(targetDir, { recursive: true, force: true });
}

// MEMORY_KILL_GRACE_MS is the constant runContainedStep's reapNow uses for a MEMORY / MEMORY_MONITOR reap
// instead of the (far slower) TIMEOUT/STALL killGraceMs default (5000ms). Guard the constant itself first —
// cheap, and catches an accidental widen even before the timing scenario below runs.
assert.ok(
  MEMORY_KILL_GRACE_MS > 0 && MEMORY_KILL_GRACE_MS < 1000,
  `MEMORY_KILL_GRACE_MS (${MEMORY_KILL_GRACE_MS}ms) must stay far below the 5000ms TIMEOUT/STALL default`,
);

// Discriminating timing check: a MEMORY-triggered reap must escalate to SIGKILL materially faster than a
// TIMEOUT/STALL reap using the SAME killGraceMs. Both scenarios below run a child that TRAPS SIGTERM (only
// SIGKILL can end it) and pass the SAME slow explicit killGraceMs — proving the MEMORY path does NOT fall
// back to killGraceMs (it must use the separate, short memoryKillGraceMs instead). If reapNow ever regresses
// to sharing one grace value across all four reap reasons again, the MEMORY scenario's elapsed time would
// rise to match the TIMEOUT scenario's and both assertions below would fail.
//
// Windows' reapTree branch always force-kills immediately (`taskkill /T /F`) and never waits out
// killGraceMs at all (a pre-existing, unrelated property of the Windows tree-kill primitive), so the slow
// TIMEOUT baseline this comparison depends on does not exist there. Skip on Windows rather than assert a
// property Windows was never guaranteed to have; MEMORY_KILL_GRACE_MS's value-level guard above already
// covers Windows.
if (!IS_WINDOWS) {
  const SLOW_SHARED_GRACE_MS = 1500;
  const trapArgs = ["-e", "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);"];

  const timeoutTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-timeout-"));
  let timeoutElapsedMs;
  try {
    const startMs = Date.now();
    const timeoutResult = await runContainedStep({
      cmd: process.execPath,
      args: trapArgs,
      cwd: process.cwd(),
      env: process.env,
      phase: "test",
      deadlineMs: Date.now() + 200, // trips almost immediately
      stallMs: 60_000,
      targetDir: timeoutTargetDir,
      killGraceMs: SLOW_SHARED_GRACE_MS,
    });
    timeoutElapsedMs = Date.now() - startMs;
    assert.equal(timeoutResult.reason, "TIMEOUT");
    assert.equal(timeoutResult.reapConfirmedDead, true);
    assert.ok(
      timeoutElapsedMs >= SLOW_SHARED_GRACE_MS * 0.8,
      `TIMEOUT reap of a SIGTERM-trapping child completed in ${timeoutElapsedMs}ms — expected to respect ` +
        `the ~${SLOW_SHARED_GRACE_MS}ms killGraceMs it was given`,
    );
  } finally {
    rmSync(timeoutTargetDir, { recursive: true, force: true });
  }

  const memGraceTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-memgrace-"));
  try {
    const startMs = Date.now();
    const memGraceResult = await runContainedStep({
      cmd: process.execPath,
      args: trapArgs,
      cwd: process.cwd(),
      env: process.env,
      phase: "test",
      deadlineMs: Date.now() + 30_000,
      stallMs: 30_000,
      targetDir: memGraceTargetDir,
      memoryLimitBytes: 1 * MiB,
      memoryPollMs: 50,
      // Deliberately the SAME slow value used above — the assertions below only pass if MEMORY ignores it.
      killGraceMs: SLOW_SHARED_GRACE_MS,
    });
    const memElapsedMs = Date.now() - startMs;
    assert.equal(memGraceResult.reason, "MEMORY");
    assert.equal(memGraceResult.reapConfirmedDead, true);
    assert.ok(
      memElapsedMs < SLOW_SHARED_GRACE_MS,
      `MEMORY-triggered reap took ${memElapsedMs}ms — expected the short memoryKillGraceMs ` +
        `(~${MEMORY_KILL_GRACE_MS}ms), not the slow killGraceMs (${SLOW_SHARED_GRACE_MS}ms) passed to this call`,
    );
    assert.ok(
      timeoutElapsedMs - memElapsedMs >= 400,
      `MEMORY reap (${memElapsedMs}ms) must escalate materially faster than the TIMEOUT reap ` +
        `(${timeoutElapsedMs}ms) using the identical killGraceMs value`,
    );
  } finally {
    rmSync(memGraceTargetDir, { recursive: true, force: true });
  }
}

// Real multi-process tree: the direct child spawns its own child (a grandchild relative to
// runContainedStep), which allocates. Proves the memory ceiling aggregates RSS across a REAL process tree
// (not just a single sampled pid) and that the reap kills the whole tree together — the synthetic
// single-process scenarios above cannot exercise the tree-aggregation/tree-kill path.
const multiTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-multiproc-"));
try {
  // Fill with real random bytes, not a zeroed/constant-fill Buffer.alloc: an all-zero (or all-one-byte)
  // page is trivially compressible/dedupable by the OS and may never actually raise resident RSS, which
  // would make this scenario time out instead of tripping the ceiling. Random content forces real growth.
  const grandchildScript =
    "const cr=require('crypto');let b=[];" +
    "setInterval(()=>{b.push(cr.randomBytes(1024*1024));},10);"; // ~1MiB/10ms real allocator
  const parentScript =
    "const{spawn}=require('child_process');" +
    `spawn(process.execPath,['-e',${JSON.stringify(grandchildScript)}],{stdio:'ignore'});` +
    "setInterval(()=>{},1000);";
  const multiResult = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", parentScript],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir: multiTargetDir,
    memoryLimitBytes: 200 * MiB,
    memoryPollMs: 50,
  });
  assert.equal(multiResult.reason, "MEMORY");
  assert.equal(multiResult.reapConfirmedDead, true);
  assert.ok(
    multiResult.memoryProcessCount >= 2,
    "expected the sampled tree to include the grandchild allocator (>=2 processes), got " +
      `${multiResult.memoryProcessCount}`,
  );
} finally {
  rmSync(multiTargetDir, { recursive: true, force: true });
}

process.stderr.write("gate memory self-test passed\n");
