/**
 * Cross-process ownership of a fixture's dependency tree.
 *
 * The hazard is inter-process, so the important cases here are driven by a REAL
 * second process holding a REAL lock file, not by a mock. A mock would prove the
 * predicate reads its own fixture correctly and nothing about whether two
 * processes can both decide to replace the same directory.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { createHash } from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  acquireFixtureLock,
  fixtureLockPath,
  releaseFixtureLock,
  withFixtureLock,
} from "./fixtureLock";

const dirs: string[] = [];
const children: ChildProcess[] = [];

afterEach(() => {
  // Killed by collected handle (pid), never by name.
  for (const child of children.splice(0)) {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function tempDir(): string {
  const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "verter-fixlock-"));
  dirs.push(dir);
  return dir;
}

/**
 * The claim suffix the module derives for a dead owner's token.
 *
 * Recomputed here rather than exported: this is the production derivation
 * restated, so a change to it fails these tests loudly instead of silently
 * making them describe a path nothing uses.
 */
function reclaimGeneration(deadToken: string): string {
  return createHash("sha256").update(deadToken).digest("hex").slice(0, 32);
}

/** Write a lock file by hand, as some other process would have. */
function plantLock(subject: string, owner: Record<string, unknown>): string {
  const lockPath = fixtureLockPath(subject);
  fs.writeFileSync(lockPath, JSON.stringify(owner, null, 2));
  dirs.push(lockPath); // rmSync on a file is fine
  return lockPath;
}

/**
 * A real child process that takes the lock file for `subject`, holds it for
 * `holdMs`, then releases and exits. Resolves once the lock is actually on disk,
 * so the caller never races the child's startup.
 */
function childHoldingLock(subject: string, holdMs: number): Promise<ChildProcess> {
  const lockPath = fixtureLockPath(subject);
  // The long-hold cases end with the child SIGKILLed while it still owns the
  // file, so the file outlives the test unless the test owns its removal.
  dirs.push(lockPath);
  const script = `
    const fs = require("node:fs");
    const os = require("node:os");
    fs.writeFileSync(${JSON.stringify(lockPath)}, JSON.stringify({
      token: "child-token", pid: process.pid, host: os.hostname(),
      startedAt: new Date().toISOString(), subject: ${JSON.stringify(subject)},
    }));
    setTimeout(() => { try { fs.unlinkSync(${JSON.stringify(lockPath)}); } catch {} }, ${holdMs});
    setTimeout(() => {}, ${holdMs + 5000});
  `;
  const child = spawn(process.execPath, ["-e", script], { stdio: "ignore" });
  children.push(child);

  return new Promise((resolve, reject) => {
    const started = Date.now();
    const poll = (): void => {
      if (fs.existsSync(lockPath)) return resolve(child);
      if (Date.now() - started > 10_000) return reject(new Error("child never took the lock"));
      setTimeout(poll, 10);
    };
    poll();
  });
}

/**
 * A copy of the production module a child process can import.
 *
 * The extension package is CommonJS, so Node loads a `.ts` file under it as
 * CommonJS and refuses its ESM syntax; `.mts` is unambiguous. The child runs the
 * REAL implementation — the same bytes, copied at test time — rather than a
 * transcription of it, which is the whole point: a lock defect that only appears
 * between two processes cannot be found by a mock inside one.
 *
 * `fixtureLock.ts` imports nothing but `node:` builtins, so the copy resolves
 * standalone. If that stops being true the copy fails to load and the test errors
 * loudly rather than quietly proving less.
 */
function productionModuleCopy(into: string): string {
  const source = path.join(__dirname, "fixtureLock.ts");
  const copy = path.join(into, "fixtureLock.mts");
  fs.copyFileSync(source, copy);
  // The child must be running the implementation, not an empty or partial file.
  expect(fs.readFileSync(copy, "utf-8")).toBe(fs.readFileSync(source, "utf-8"));
  return copy;
}

/**
 * Run a child to completion, or kill it and say it never finished.
 *
 * Every defect in this module that is not a wrong ANSWER is a process that does
 * not come back — a spin, a wedge, a wait with no deadline. Those cannot be
 * caught in-process: the loops here are synchronous, so they never yield to a
 * test timeout, and the suite hangs instead of failing. A child with a deadline
 * turns "never returns" into an ordinary assertion.
 */
function runToCompletion(
  script: string,
  args: string[],
  deadlineMs: number,
): Promise<{ timedOut: boolean; code: number | null; signal: NodeJS.Signals | null }> {
  const child = spawn(process.execPath, [script, ...args], { stdio: "ignore" });
  children.push(child);
  return new Promise((resolve) => {
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      // By collected handle, never by name.
      child.kill("SIGKILL");
    }, deadlineMs);
    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ timedOut, code, signal });
    });
  });
}

/**
 * One waiter: it asks for a lock it cannot have and reports how it waited.
 *
 * Counting liveness calls is what distinguishes "polled to the deadline" from
 * "spun at 100% CPU", and the count has to be taken inside the process making
 * the call.
 */
const WAITER_SOURCE = `
import * as fs from "node:fs";
import { acquireFixtureLock } from "./fixtureLock.mts";

const [subject, deadPid, report] = process.argv.slice(2);
let liveness = 0;
let message = "IT ACQUIRED A LOCK IT COULD NOT RECLAIM";
const started = Date.now();
try {
  acquireFixtureLock(subject, {
    timeoutMs: 250,
    pollMs: 10,
    isProcessAlive: (pid) => {
      liveness += 1;
      return pid !== Number(deadPid);
    },
  });
} catch (error) {
  message = String(error && error.message ? error.message : error);
}
fs.writeFileSync(report, JSON.stringify({ elapsed: Date.now() - started, liveness, message }));
`;

/**
 * One racer: it starts at an assigned instant, acquires the lock through the
 * real API, and enters a critical section that only one process can occupy.
 *
 * Two independent witnesses, because they miss different things:
 *
 *   - `mkdir`, the same exclusive-create the lock itself relies on. A second
 *     entrant while the first is inside gets EEXIST, which is precisely "two
 *     processes both replacing one fixture's node_modules". It sees nothing if
 *     the first process has already left.
 *   - the interval this process HELD the lock, sampled just after the acquire
 *     and just before the release, so it is strictly inside the true interval
 *     and two of them can never overlap by accident. Two that do overlap are two
 *     owners, whatever either did with the time.
 *
 * The start instant is absolute wall clock rather than "when I saw the barrier",
 * so the schedule is the same in every racer and the parent can check afterwards
 * that the one it intended is the one that ran.
 */
const RACER_SOURCE = `
import * as fs from "node:fs";
import { acquireFixtureLock, releaseFixtureLock } from "./fixtureLock.mts";

const [subject, critical, ready, go, holdMs, staggerMs, index, report] = process.argv.slice(2);

function sleep(ms) {
  if (ms > 0) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

fs.writeFileSync(ready, "ready");
let goAt = 0;
for (;;) {
  try {
    goAt = Number(fs.readFileSync(go, "utf-8"));
  } catch {}
  if (goAt > 0) break;
  sleep(1);
}
// Sleeping rather than spinning: four processes spinning would take CPU from
// the very read this schedule is built around.
const startAt = goAt + Number(index) * Number(staggerMs);
sleep(startAt - Date.now() - 2);
while (Date.now() < startAt) {}

const outcome = {
  index: Number(index),
  startedAt: Date.now(),
  acquired: false,
  token: "",
  violation: "",
  heldFrom: 0,
  heldTo: 0,
};
try {
  const lock = acquireFixtureLock(subject, { timeoutMs: 20000, pollMs: 5 });
  outcome.heldFrom = Date.now();
  outcome.acquired = true;
  outcome.token = lock.token;
  try {
    try {
      fs.mkdirSync(critical);
    } catch (error) {
      outcome.violation = "entered while another process was inside: " + error.code;
      throw error;
    }
    const until = Date.now() + Number(holdMs);
    while (Date.now() < until) {}
    fs.rmdirSync(critical);
  } finally {
    outcome.heldTo = Date.now();
    releaseFixtureLock(lock);
  }
} catch (error) {
  outcome.error = String(error && error.message ? error.message : error);
}
fs.writeFileSync(report, JSON.stringify(outcome));
`;

describe("fixtureLockPath", () => {
  it("keys two spellings of one directory to the same lock", () => {
    const dir = tempDir();
    const real = fs.realpathSync(dir);
    expect(fixtureLockPath(dir)).toBe(fixtureLockPath(real));
    expect(fixtureLockPath(dir)).toBe(fixtureLockPath(path.join(dir, ".", "")));
  });

  it("keys different directories to different locks", () => {
    // Without this, one lock would serialise every fixture — correct but a
    // silent throughput collapse — and the test above would hold vacuously.
    expect(fixtureLockPath(tempDir())).not.toBe(fixtureLockPath(tempDir()));
  });

  it("keeps the lock out of the fixture directory, which is git-tracked", () => {
    const dir = tempDir();
    const lock = fixtureLockPath(dir);
    expect(lock.startsWith(path.resolve(dir))).toBe(false);
    withFixtureLock(dir, () => {
      expect(fs.readdirSync(dir)).toEqual([]);
    });
  });
});

describe("acquireFixtureLock", () => {
  it("takes and releases a free lock", () => {
    const dir = tempDir();
    const lock = acquireFixtureLock(dir, { timeoutMs: 2_000 });
    expect(fs.existsSync(lock.path)).toBe(true);
    releaseFixtureLock(lock);
    expect(fs.existsSync(lock.path)).toBe(false);
  });

  it("refuses a lock a LIVE other process holds, rather than stealing it", async () => {
    const dir = tempDir();
    const child = await childHoldingLock(dir, 60_000);

    // Positive control: the lock really is held by a process that is really alive.
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);
    expect(child.exitCode).toBeNull();

    expect(() => acquireFixtureLock(dir, { timeoutMs: 250, pollMs: 25 })).toThrow(
      /timed out .* waiting for the fixture dependency lock/s,
    );
    // Still the child's — a timeout must not have deleted it on the way out.
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);
  });

  it("acquires once the holding process releases", async () => {
    const dir = tempDir();
    await childHoldingLock(dir, 400);

    const started = Date.now();
    const lock = acquireFixtureLock(dir, { timeoutMs: 20_000, pollMs: 25 });
    const waited = Date.now() - started;

    // It waited rather than stealing, and it did eventually get in.
    expect(waited).toBeGreaterThan(150);
    expect(fs.existsSync(lock.path)).toBe(true);
    releaseFixtureLock(lock);
  });

  it("reclaims a lock whose owner is gone", () => {
    const dir = tempDir();
    // A pid that cannot be running: the child exits before we look.
    plantLock(dir, {
      token: "dead",
      pid: 999_999_998,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });
    const lock = acquireFixtureLock(dir, {
      timeoutMs: 2_000,
      pollMs: 10,
      isProcessAlive: () => false,
    });
    expect(lock.token).not.toBe("dead");
    releaseFixtureLock(lock);
  });

  it("does not evict the lock a WINNER took while this process was deciding", () => {
    // The reclaim race, made deterministic. Two processes read the same dead
    // owner; one reclaims it and takes the lock; the other must not then delete
    // the winner's live lock on the strength of its now-stale read.
    //
    // `isProcessAlive` is the injection point because it is called between the
    // read of the lock and the decision to remove it — exactly the window a
    // second process interleaves into. The hook plays the winner: it replaces
    // the dead lock with a LIVE one, then answers the question that was asked
    // about the DEAD owner.
    const dir = tempDir();
    const lockPath = fixtureLockPath(dir);
    const deadPid = 999_999_997;
    plantLock(dir, {
      token: "dead-owner",
      pid: deadPid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });

    let interleaved = false;
    const winner = {
      token: "the-winner",
      pid: process.pid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    };

    expect(() =>
      acquireFixtureLock(dir, {
        timeoutMs: 300,
        pollMs: 10,
        isProcessAlive: (pid) => {
          if (!interleaved) {
            interleaved = true;
            fs.unlinkSync(lockPath);
            fs.writeFileSync(lockPath, JSON.stringify(winner, null, 2));
          }
          return pid !== deadPid;
        },
      }),
    ).toThrow(/timed out/);

    // Positive control: the interleaving really happened, so the assertions
    // below are about a lock that really was taken.
    expect(interleaved).toBe(true);
    // The winner still owns the fixture. Without this, both processes would
    // enter the destructive section and one would stamp a tree the other was
    // midway through deleting.
    expect(fs.existsSync(lockPath)).toBe(true);
    expect(JSON.parse(fs.readFileSync(lockPath, "utf-8")).token).toBe("the-winner");
  });

  it("obeys the deadline and the poll while a reclaim cannot proceed", async () => {
    // A reclaim that cannot complete must rejoin the ordinary wait, not retry
    // itself. When the removal branch loops above the deadline test and the
    // sleep, a launcher spins at 100% CPU forever: no timeout, no output, no
    // error. The reachable form of "cannot complete" is another process already
    // reclaiming this owner; a persistent `unlink` failure (a Windows scanner
    // holding the file open reports EPERM/EBUSY) reaches the same branch.
    //
    // The acquire runs in a CHILD with a deadline, because the defect is a
    // synchronous loop: in-process it never yields, so vitest's own timeout
    // never fires and the reintroduced spin HANGS the suite at 99% CPU instead
    // of failing it. A gate that hangs CI is a gate that gets disabled.
    const dir = tempDir();
    const workspace = tempDir();
    const deadPid = 999_999_996;
    const lockPath = plantLock(dir, {
      token: "dead-and-being-reclaimed",
      pid: deadPid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });
    // Another process owns this owner's reclamation. The claim is named after
    // the HASH of the dead token, never the token itself.
    const claimPath = `${lockPath}.reclaim-${reclaimGeneration("dead-and-being-reclaimed")}`;
    fs.writeFileSync(claimPath, "{}");
    dirs.push(claimPath);

    productionModuleCopy(workspace);
    const script = path.join(workspace, "waiter.mjs");
    fs.writeFileSync(script, WAITER_SOURCE);
    const report = path.join(workspace, "report");

    const run = await runToCompletion(script, [dir, String(deadPid), report], 8_000);

    // The defect's shape is a process that never returns, so that is asserted
    // first and by itself.
    expect(run.timedOut).toBe(false);
    expect(run.code).toBe(0);

    const outcome = JSON.parse(fs.readFileSync(report, "utf-8")) as {
      elapsed: number;
      liveness: number;
      message: string;
    };

    // Positive control: the reclaim branch really was entered, so the bounds
    // below describe the loop under test rather than an untaken path.
    expect(outcome.liveness).toBeGreaterThan(0);
    expect(outcome.message).toMatch(/timed out/);
    // It waited, and it stopped waiting. A hot loop never reaches either bound.
    expect(outcome.elapsed).toBeGreaterThanOrEqual(240);
    // ~25 polls are expected in a 250ms budget at 10ms. A spinning loop makes
    // this call hundreds of thousands of times; a slow machine makes FEWER, so
    // the bound only tightens in the direction that matters.
    expect(outcome.liveness).toBeLessThan(400);
    // Fail closed: the lock it could not reclaim is still there.
    expect(fs.existsSync(lockPath)).toBe(true);
  });

  it("reclaims from a REALLY dead process, using the real liveness check", async () => {
    // Every other reclaim test injects `isProcessAlive`, so the production
    // predicate — the one that decides whether a fixture's dependencies may be
    // replaced — was never run. Nothing here is injected: a real child exits,
    // and its pid is the one recorded in the lock.
    const dir = tempDir();
    const child = spawn(process.execPath, ["-e", "process.exit(0)"], { stdio: "ignore" });
    children.push(child);
    const deadPid = child.pid as number;
    await new Promise<void>((resolve) => child.on("exit", () => resolve()));

    // Positive control: the pid really is gone, and it is a plausible one — a
    // reclaim rule that only worked on absurd pids would pass a planted 999999998.
    expect(child.exitCode).toBe(0);
    expect(() => process.kill(deadPid, 0)).toThrow();

    plantLock(dir, {
      token: "killed",
      pid: deadPid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });
    const lock = acquireFixtureLock(dir, { timeoutMs: 5_000, pollMs: 10 });
    expect(lock.token).not.toBe("killed");
    releaseFixtureLock(lock);
    // Without this the suite would hang for the full 10-minute default on every
    // run that followed a SIGKILL, which is the outcome the reclaim rule exists
    // to avoid.
  });

  it.skipIf(process.platform === "win32")(
    "treats a process it may not signal as ALIVE, not as gone",
    () => {
      // `kill(pid, 0)` answers EPERM for a live process owned by another user.
      // Reading that as "gone" would let any unprivileged run steal the lock of
      // a root-owned one. pid 1 is the portable POSIX instance of that; Windows
      // has no equivalent init pid, so the case is skipped rather than faked.
      // Running as root (a CI container) turns EPERM into success — still alive,
      // so the assertion holds, but it then proves the ordinary path instead.
      const dir = tempDir();
      plantLock(dir, {
        token: "root-owned",
        pid: 1,
        host: os.hostname(),
        startedAt: new Date().toISOString(),
        subject: dir,
      });
      expect(() => acquireFixtureLock(dir, { timeoutMs: 200, pollMs: 20 })).toThrow(/timed out/);
      expect(JSON.parse(fs.readFileSync(fixtureLockPath(dir), "utf-8")).token).toBe("root-owned");
    },
  );

  it("treats a malformed lock as HELD, never as debris", () => {
    const dir = tempDir();
    dirs.push(fixtureLockPath(dir));
    fs.writeFileSync(fixtureLockPath(dir), "{ not json");
    expect(() => acquireFixtureLock(dir, { timeoutMs: 200, pollMs: 20 })).toThrow(
      /unreadable lock file/,
    );
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(true);
  });

  it("still locks where the filesystem refuses hard links", () => {
    // `link` is not universal. A Windows `%TEMP%` on FAT/exFAT, and some network
    // mounts, refuse it with EPERM while ordinary writes succeed. Rethrowing that
    // raw fails every run on those machines with an errno that names neither the
    // cause nor the constraint.
    const dir = tempDir();
    let attempts = 0;
    const noHardLinks = (): never => {
      attempts += 1;
      const error: NodeJS.ErrnoException = new Error(
        "EPERM: operation not permitted, link 'pending' -> 'lock'",
      );
      error.code = "EPERM";
      throw error;
    };

    const lock = acquireFixtureLock(dir, { timeoutMs: 2_000, link: noHardLinks });
    dirs.push(lock.path);

    // Positive control: the fallback is what produced this lock, not `link`.
    expect(attempts).toBeGreaterThan(0);
    // And it is a real lock, not an empty placeholder: it names its owner, so a
    // later process can ask whether that owner is still alive.
    const owner = JSON.parse(fs.readFileSync(lock.path, "utf-8"));
    expect(owner.token).toBe(lock.token);
    expect(owner.pid).toBe(process.pid);

    // The property that matters is unchanged: it still excludes.
    expect(() =>
      acquireFixtureLock(dir, { timeoutMs: 150, pollMs: 20, link: noHardLinks }),
    ).toThrow(/timed out/);

    releaseFixtureLock(lock);
    expect(fs.existsSync(lock.path)).toBe(false);
  });

  it("never lets a lock payload shape a path", () => {
    // The token is read from a file on disk, and it used to be interpolated
    // straight into the reclaim claim's filename. A crafted one therefore
    // decided where this process created and deleted a file — outside the lock
    // directory, on a path no part of this module chose. It cannot clobber
    // (`link` refuses a destination that exists), but a value read from disk
    // must not shape a path at all.
    const dir = tempDir();
    const lockDir = path.dirname(fixtureLockPath(dir));
    plantLock(dir, {
      token: "../../verter-gb11-escaped-claim",
      pid: 999_999_994,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });

    const targets: string[] = [];
    const recordingLink = (existingPath: string, newPath: string): void => {
      targets.push(newPath);
      fs.linkSync(existingPath, newPath);
    };

    // The reclaim still works: what the payload SAYS its token is has no
    // bearing on where the claim goes. Before the fix this timed out, because
    // the crafted path named a directory that does not exist.
    const lock = acquireFixtureLock(dir, {
      timeoutMs: 2_000,
      pollMs: 10,
      isProcessAlive: () => false,
      link: recordingLink,
    });
    releaseFixtureLock(lock);

    // Positive control: the claim really was attempted, so the paths below are
    // the ones the reclaim used rather than an empty list.
    expect(targets.length).toBeGreaterThan(1);
    // Every path this module created stayed directly inside the lock directory.
    expect(targets.filter((target) => path.dirname(path.resolve(target)) !== lockDir)).toEqual([]);
    expect(fs.existsSync(path.resolve(lockDir, "..", "..", "verter-gb11-escaped-claim"))).toBe(
      false,
    );
  });

  it("says so when the reclaim claim it made cannot be removed", () => {
    // The claim is removed in a `finally`, and every errno there was swallowed —
    // including the ones the unlink ten lines above handles explicitly. A claim
    // that outlives its claimant is the one thing that stops this generation ever
    // being reclaimed again, so a silent failure there is a wedge that the docs
    // describe as "only a crash inside the window".
    //
    // A directory at the claim path is the portable way to make that unlink fail:
    // `unlink` refuses one everywhere, with EPERM or EISDIR depending on the
    // platform. The injected link plants it, which is exactly the state a claim
    // this process cannot remove leaves behind.
    const dir = tempDir();
    const lockPath = fixtureLockPath(dir);
    const claimPath = `${lockPath}.reclaim-${reclaimGeneration("dead-owner-unremovable-claim")}`;
    dirs.push(claimPath);
    plantLock(dir, {
      token: "dead-owner-unremovable-claim",
      pid: 999_999_991,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: dir,
    });

    const plantingLink = (existingPath: string, newPath: string): void => {
      if (newPath !== claimPath) return fs.linkSync(existingPath, newPath);
      fs.mkdirSync(newPath, { recursive: true });
      fs.writeFileSync(path.join(newPath, "occupied"), "");
    };

    const warnings: string[] = [];
    const warn = vi.spyOn(console, "warn").mockImplementation((...args: unknown[]) => {
      warnings.push(args.map(String).join(" "));
    });
    try {
      const lock = acquireFixtureLock(dir, {
        timeoutMs: 2_000,
        pollMs: 10,
        isProcessAlive: () => false,
        link: plantingLink,
      });
      releaseFixtureLock(lock);
    } finally {
      warn.mockRestore();
    }

    // Positive control: the claim really was planted, so the unlink really did
    // fail rather than never running.
    expect(fs.existsSync(claimPath)).toBe(true);
    expect(warnings.filter((line) => line.includes(claimPath))).toHaveLength(1);
  });

  it("REFUSES when the link failed but this directory can do hard links", () => {
    // The fallback trades exclusivity guarantees for portability, so it may only
    // engage where the primitive it replaces is genuinely unavailable. It used to
    // engage on EVERY non-EEXIST link error, so a transient IO failure silently
    // downgraded the claim on a filesystem that had hard links all along.
    //
    // The gate is a probe rather than an errno list: an errno is a guess about
    // what a platform reports, while a link that succeeds in this very directory
    // is proof the capability is there.
    const dir = tempDir();
    const lockPath = fixtureLockPath(dir);
    let probes = 0;
    const flakyLink = (existingPath: string, newPath: string): void => {
      if (newPath === lockPath) {
        const error: NodeJS.ErrnoException = new Error("EIO: i/o error, link");
        error.code = "EIO";
        throw error;
      }
      probes += 1;
      fs.linkSync(existingPath, newPath);
    };

    expect(() => acquireFixtureLock(dir, { timeoutMs: 2_000, link: flakyLink })).toThrow(/EIO/);
    // Positive control: the probe really ran, so the refusal is the considered
    // outcome rather than a link that was never attempted.
    expect(probes).toBeGreaterThan(0);
    // And it refused rather than taking the lock through the weaker primitive.
    expect(fs.existsSync(lockPath)).toBe(false);
    // The probe cleans up after itself; the directory is as it was.
    expect(fs.readdirSync(path.dirname(lockPath)).filter((n) => n.includes(".pending-"))).toEqual(
      [],
    );
  });

  it("never sweeps away its own in-flight payload", () => {
    // The sweep removes debris left by processes that are PROVABLY GONE, and the
    // sweeper is the one process it can prove is not: it is running. Its own
    // `.pending-` payload is nevertheless sitting in the shared directory while
    // it sweeps — written first on purpose, so the reclaim it may perform has a
    // payload to publish — and the sweep's predicate is an injection point.
    //
    // So the rule cannot be left to the predicate. Answer "gone" for everything
    // and the sweep deletes the very file the claim is about to link FROM, which
    // fails ENOENT for a reason that has nothing to do with locking. The
    // exclusion is unharmed either way, so nothing here was ever unsafe — it is
    // a self-inflicted failure of the operation it is trying to perform.
    const dir = tempDir();
    const sources: string[] = [];
    const present: boolean[] = [];
    const watchingLink = (existingPath: string, newPath: string): void => {
      sources.push(existingPath);
      present.push(fs.existsSync(existingPath));
      fs.linkSync(existingPath, newPath);
    };

    const lock = acquireFixtureLock(dir, {
      timeoutMs: 2_000,
      pollMs: 10,
      // Every owner reads as gone — including this process's own.
      isProcessAlive: () => false,
      link: watchingLink,
    });
    releaseFixtureLock(lock);

    // Positive control: a link really was attempted, from the pending payload.
    expect(sources.length).toBeGreaterThan(0);
    expect(sources.every((source) => source.includes(".pending-"))).toBe(true);
    // And the file it linked from was still there each time.
    expect(present).not.toContain(false);
  });

  it("REFUSES when the probe cannot say whether hard links are available either", () => {
    // The gap a probe alone leaves. An error that hits the claim AND the probe —
    // one EIO storm on a network mount reaches both — made the probe answer
    // "no hard links here", which is the one answer that engages the weaker
    // primitive. The fallback is not atomic on NFSv3, and a mount having an IO
    // fit is exactly the kind that is one.
    //
    // A probe failure is proof of the ATTEMPT, not of the capability. Only a
    // positively classified absence may downgrade; an unexplained failure
    // refuses.
    const dir = tempDir();
    const lockPath = fixtureLockPath(dir);
    dirs.push(lockPath);
    let links = 0;
    const everythingFails = (): never => {
      links += 1;
      const error: NodeJS.ErrnoException = new Error("EIO: i/o error, link");
      error.code = "EIO";
      throw error;
    };

    let message = "IT TOOK A LOCK THROUGH THE WEAKER PRIMITIVE";
    try {
      acquireFixtureLock(dir, { timeoutMs: 2_000, link: everythingFails });
    } catch (error) {
      message = String((error as Error).message);
    }

    // Positive control: the claim AND the probe really were attempted, so the
    // refusal is a classified outcome rather than a link nobody tried.
    expect(links).toBeGreaterThan(1);
    expect(message).toMatch(/EIO/);
    // Classified as UNDECIDED, not as "this filesystem has no hard links" — the
    // two failures differ only in the errno, and only one of them may downgrade.
    expect(message).toMatch(/could not establish/);
    // And nothing was created: no lock through the weaker primitive, and no
    // probe debris left behind by the attempts that failed.
    expect(fs.existsSync(lockPath)).toBe(false);
    expect(
      fs.readdirSync(path.dirname(lockPath)).filter((n) => n.includes("-hardlink-probe-")),
    ).toEqual([]);
  });

  it("retries a claim that failed transiently instead of stopping the run", () => {
    // The other half of the same gap. Once the probe proves this directory can
    // link, a claim that failed for some other reason is unexplained — and an
    // unexplained failure that does NOT recur is a blip, not a state anyone has
    // to act on. Refusing on the first one ends a ten-minute suite because a
    // single syscall hiccupped.
    //
    // So it is retried, bounded, and only a failure that survives the retries
    // refuses. Fail-closed is the destination, not the first step.
    const dir = tempDir();
    const lockPath = fixtureLockPath(dir);
    let claims = 0;
    const failsOnce = (existingPath: string, newPath: string): void => {
      if (newPath === lockPath) {
        claims += 1;
        if (claims === 1) {
          const error: NodeJS.ErrnoException = new Error("EIO: i/o error, link");
          error.code = "EIO";
          throw error;
        }
      }
      fs.linkSync(existingPath, newPath);
    };

    const lock = acquireFixtureLock(dir, { timeoutMs: 2_000, link: failsOnce });
    dirs.push(lock.path);

    // Positive control: the first claim really did fail, so this lock came from
    // a retry rather than from a link that never had a problem.
    expect(claims).toBeGreaterThan(1);
    // And it is the real thing: the strong primitive published it whole, so it
    // names its owner.
    const owner = JSON.parse(fs.readFileSync(lock.path, "utf-8"));
    expect(owner.token).toBe(lock.token);
    expect(owner.pid).toBe(process.pid);
    releaseFixtureLock(lock);
    expect(fs.existsSync(lock.path)).toBe(false);
  });

  it("treats a lock from another host as HELD, whatever its pid says", () => {
    const dir = tempDir();
    plantLock(dir, {
      token: "elsewhere",
      pid: process.pid,
      host: `${os.hostname()}-some-other-machine`,
      startedAt: new Date().toISOString(),
      subject: dir,
    });
    // A pid is meaningless across hosts, so liveness must not be consulted at all.
    expect(() =>
      acquireFixtureLock(dir, { timeoutMs: 200, pollMs: 20, isProcessAlive: () => false }),
    ).toThrow(/timed out/);
  });
});

describe("two real acquirers racing for one fixture", () => {
  /**
   * The window this race has to land in.
   *
   * The interleaving that matters is between READING the dead owner's lock and
   * acting on what it said, and the window is as wide as everything the WINNER
   * does in between. That is not one unlink and one link: an acquire PARSES the
   * dead owner's payload three times before the lock changes hands — once in the
   * debris sweep, once in the loop, and once more under the reclaim claim to
   * prove the file is still the one it decided about. Measured here at 2.5x the
   * cost of a single read.
   *
   * Three parses of a SMALL lock is still microseconds, though, so a plain
   * simultaneous release is decided by scheduling jitter: the reviewers who ran
   * the previous version of this test against the pre-fix module got 6 passes
   * and 7 failures out of 13. A gate that fires half the time is not a gate.
   *
   * So the window is MEASURED and the racers are released INTO it: reading the
   * planted lock is made to take a known number of milliseconds, and racer `i`
   * starts a fraction of that after racer 0. Every racer is then still inside
   * its own read when the first one takes the lock, which is the state that
   * produces a stale decision — by construction rather than by luck.
   *
   * The payload is sized at runtime rather than pinned, because the whole point
   * is a window wider than this machine's jitter, and machines differ. Many
   * small keys rather than one long string: JSON.parse costs about eight times
   * as much per byte for them, so the file stays small enough that every other
   * acquire in the suite is not paying to read it.
   */
  const READ_WINDOW_FLOOR_MS = 20;
  const RACERS = 4;
  const ROUNDS = 3;

  function slowDeadOwner(subject: string, token: string, keys: number): string {
    const padding: Record<string, number> = {};
    for (let i = 0; i < keys; i += 1) padding[`slow-to-parse-key-number-${i}`] = i;
    return JSON.stringify({
      token,
      pid: 999_999_995,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject,
      padding,
    });
  }

  /** Plant the lock a SIGKILLed run leaves, and report how long reading it takes. */
  function plantSlowDeadLock(subject: string, token: string): { path: string; readMs: number } {
    const lockPath = fixtureLockPath(subject);
    for (let keys = 150_000; ; keys *= 2) {
      fs.writeFileSync(lockPath, slowDeadOwner(subject, token, keys));
      const started = Date.now();
      JSON.parse(fs.readFileSync(lockPath, "utf-8"));
      const readMs = Date.now() - started;
      if (readMs >= READ_WINDOW_FLOOR_MS || keys >= 2_400_000) return { path: lockPath, readMs };
    }
  }

  interface RacerOutcome {
    index: number;
    startedAt: number;
    acquired: boolean;
    token: string;
    violation: string;
    heldFrom: number;
    heldTo: number;
    error?: string;
  }

  /** Two owners at once, whatever either of them did with the time. */
  function overlappingHolds(outcomes: readonly RacerOutcome[]): string[] {
    const held = outcomes.filter((o) => o.acquired && o.heldTo > o.heldFrom);
    const overlaps: string[] = [];
    for (let a = 0; a < held.length; a += 1) {
      for (let b = a + 1; b < held.length; b += 1) {
        if (held[a].heldFrom < held[b].heldTo && held[b].heldFrom < held[a].heldTo) {
          overlaps.push(`racer ${held[a].index} and racer ${held[b].index} held the lock at once`);
        }
      }
    }
    return overlaps;
  }

  it("admits exactly one at a time, starting from a dead owner's lock", async () => {
    // The state a SIGKILLed run leaves behind, which is the state the reclaim
    // race needs: a lock naming a pid that no longer exists. Every racer here
    // is a separate process calling the real API. The earlier tests drive one
    // acquirer against a planted file; this one is the case that shipped the
    // defect, because nothing here writes a lock by hand.
    const dir = tempDir();
    const workspace = tempDir();
    dirs.push(fixtureLockPath(dir));
    productionModuleCopy(workspace);
    const racerPath = path.join(workspace, "racer.mjs");
    fs.writeFileSync(racerPath, RACER_SOURCE);

    let scheduled = 0;
    const slipped: string[] = [];

    for (let round = 0; round < ROUNDS; round += 1) {
      const token = `killed-run-${round}`;
      const { path: lockPath, readMs } = plantSlowDeadLock(dir, token);
      // A quarter of the read, so the last racer starts three quarters of the
      // way into the first one's — inside it with room to spare either way.
      const stagger = Math.max(2, Math.round(readMs / 6));
      // Long enough that the last racer's attempt still lands while the first
      // one is inside, or `mkdir` would see nothing to collide with.
      const hold = Math.max(150, (RACERS - 1) * stagger + readMs + 50);

      const critical = path.join(workspace, `critical-${round}`);
      const go = path.join(workspace, `go-${round}`);
      const reports = Array.from({ length: RACERS }, (_, i) =>
        path.join(workspace, `report-${round}-${i}`),
      );
      const readies = Array.from({ length: RACERS }, (_, i) =>
        path.join(workspace, `ready-${round}-${i}`),
      );

      const running = reports.map((report, i) => {
        const child = spawn(
          process.execPath,
          [
            racerPath,
            dir,
            critical,
            readies[i],
            go,
            String(hold),
            String(stagger),
            String(i),
            report,
          ],
          { stdio: "ignore" },
        );
        children.push(child);
        return new Promise<number | null>((resolve) => child.on("exit", resolve));
      });

      await new Promise<void>((resolve, reject) => {
        const started = Date.now();
        const poll = (): void => {
          if (readies.every((file) => fs.existsSync(file))) return resolve();
          if (Date.now() - started > 20_000) {
            return reject(new Error("racers never reported ready"));
          }
          setTimeout(poll, 5);
        };
        poll();
      });
      // An absolute instant in the near future, so no racer starts before the
      // slowest one has read the barrier.
      fs.writeFileSync(go, String(Date.now() + 150));
      await Promise.all(running);

      const outcomes = reports.map(
        (report) => JSON.parse(fs.readFileSync(report, "utf-8")) as RacerOutcome,
      );

      // Positive controls: every racer really ran and really got in, so "no
      // violation" is a statement about four completed critical sections
      // rather than about four processes that all timed out.
      expect(outcomes).toHaveLength(RACERS);
      expect(outcomes.filter((o) => o.acquired)).toHaveLength(RACERS);
      expect(new Set(outcomes.map((o) => o.token)).size).toBe(RACERS);
      // The dead owner really was reclaimed — it is not still sitting there.
      expect(outcomes.map((o) => o.token)).not.toContain(token);

      // Did the schedule this round was built on actually happen? Every racer
      // has to have started while the first one was still reading, or the
      // stale decision is never reached and a clean result means nothing. A
      // round whose processes were descheduled proves nothing either way, and
      // is recorded as such below rather than counted as a clean one.
      const spread =
        Math.max(...outcomes.map((o) => o.startedAt)) -
        Math.min(...outcomes.map((o) => o.startedAt));
      if (spread >= readMs) {
        slipped.push(`round ${round}: starts spread over ${spread}ms of a ${readMs}ms read`);
      } else {
        scheduled += 1;
      }

      expect(outcomes.map((o) => o.violation).filter(Boolean)).toEqual([]);
      expect(overlappingHolds(outcomes)).toEqual([]);
      expect(outcomes.map((o) => o.error).filter(Boolean)).toEqual([]);
      // Nobody left the critical section occupied, and the lock is free again.
      expect(fs.existsSync(critical)).toBe(false);
      expect(fs.existsSync(lockPath)).toBe(false);
    }

    // EVERY round has to have EXERCISED the race, not merely survived it. A
    // "one of them did" rule leaves the other two free to prove nothing while
    // the run still reports clean, and it is not as though the rounds are
    // independent evidence of different things — they are the same experiment
    // repeated because one sample of a scheduling-sensitive race is thin.
    // Requiring all of them costs nothing on a machine that can schedule them,
    // and says plainly that it cannot on one that could not. The detail rides
    // in the assertion so a slip is diagnosable from the failure alone.
    expect(slipped).toEqual([]);
    expect(scheduled).toBe(ROUNDS);
  }, 60_000);
});

describe("lock directory debris", () => {
  function owner(subject: string, token: string, pid: number): Record<string, unknown> {
    return {
      token,
      pid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject,
    };
  }

  function ownerRecord(subject: string, token: string, pid: number): string {
    return JSON.stringify(owner(subject, token, pid));
  }

  it("reaps what a killed run left, and only what a killed run left", () => {
    // A process killed between writing its private payload and linking it into
    // place leaves a `.pending-` file nothing ever removes. Harmless, but it
    // accumulates in a shared directory forever.
    const dir = tempDir();
    const other = tempDir();
    const lockPath = fixtureLockPath(dir);
    const otherLockPath = fixtureLockPath(other);
    const deadPid = 999_999_993;

    const deadPending = `${lockPath}.pending-${deadPid}-abandoned`;
    fs.writeFileSync(deadPending, ownerRecord(dir, "abandoned", deadPid));
    dirs.push(deadPending);

    // A pending belonging to a process that is still running. Reaping this is
    // the difference between a reaper and a delete-everything sweep.
    const livePending = `${lockPath}.pending-${process.pid}-in-flight`;
    fs.writeFileSync(livePending, ownerRecord(dir, "in-flight", process.pid));
    dirs.push(livePending);

    // A reclaim claim whose lock is gone: its work can no longer be in flight.
    const orphanClaim = `${lockPath}.reclaim-a-lock-that-is-gone`;
    fs.writeFileSync(orphanClaim, ownerRecord(dir, "gone", deadPid));
    dirs.push(orphanClaim);

    // A reclaim claim whose LOCK still exists. Never reaped, whatever its
    // claimant's pid says: a claim is the only thing serialising removal of that
    // lock, and handing it to a second process is the race the claim prevents.
    fs.writeFileSync(otherLockPath, ownerRecord(other, "still-held", process.pid));
    dirs.push(otherLockPath);
    const heldClaim = `${otherLockPath}.reclaim-still-held`;
    fs.writeFileSync(heldClaim, ownerRecord(other, "reclaimer", deadPid));
    dirs.push(heldClaim);

    // Positive controls: all four really are on disk before the sweep.
    expect([deadPending, livePending, orphanClaim, heldClaim].map(fs.existsSync)).toEqual([
      true,
      true,
      true,
      true,
    ]);

    const lock = acquireFixtureLock(dir, { timeoutMs: 5_000, pollMs: 10 });
    releaseFixtureLock(lock);

    expect(fs.existsSync(deadPending)).toBe(false);
    expect(fs.existsSync(orphanClaim)).toBe(false);
    expect(fs.existsSync(livePending)).toBe(true);
    expect(fs.existsSync(heldClaim)).toBe(true);
    // And the sweep did not touch a lock that is still someone's.
    expect(fs.existsSync(otherLockPath)).toBe(true);
  });

  it("reclaims a killed run's lock once its fixture is gone, and only then", () => {
    // The class nothing else can ever collect. A dead owner's lock is normally
    // reclaimed by the next process that wants that fixture — but when the
    // fixture itself is gone, no such process exists, so the file stays in a
    // directory shared by every fixture and every run forever. This is what
    // filled the machine's lock directory with 43 leftovers from one day.
    //
    // Removal goes through the ordinary two-gate reclaim, not a bare unlink
    // here: a sweep that removed locks on its own authority would be the very
    // race the claim exists to prevent, running on every acquire.
    const deadPid = 999_999_992;

    // (1) Dead owner, fixture gone: the only sweepable shape.
    const vanished = tempDir();
    const vanishedLock = plantLock(vanished, owner(vanished, "killed-run", deadPid));
    fs.rmSync(vanished, { recursive: true, force: true });

    // (2) Dead owner, fixture still there. Left alone: the next process that
    // wants that fixture reclaims it under the claim, which is where the
    // decision belongs.
    const stillThere = tempDir();
    const stillThereLock = plantLock(stillThere, owner(stillThere, "also-dead", deadPid));

    // (3) LIVE owner, fixture gone. Left alone whatever its subject says — a
    // fixture can be absent for a moment while the process that owns it works.
    const liveButGone = tempDir();
    const liveLock = plantLock(liveButGone, owner(liveButGone, "live", process.pid));
    fs.rmSync(liveButGone, { recursive: true, force: true });

    // (4) Dead owner, and a subject that names no absolute path. Left alone:
    // a relative subject means the writer's working directory, which is not a
    // question this reader can answer.
    const relative = tempDir();
    const relativeLock = plantLock(relative, {
      ...owner(relative, "relative-subject", deadPid),
      subject: "fixtures/gone-as-far-as-this-process-knows",
    });

    // Positive controls: all four really are on disk, and the two "gone"
    // fixtures really are gone.
    expect([vanishedLock, stillThereLock, liveLock, relativeLock].map(fs.existsSync)).toEqual([
      true,
      true,
      true,
      true,
    ]);
    expect([fs.existsSync(vanished), fs.existsSync(liveButGone)]).toEqual([false, false]);

    const lock = acquireFixtureLock(tempDir(), { timeoutMs: 5_000, pollMs: 10 });
    releaseFixtureLock(lock);

    expect(fs.existsSync(vanishedLock)).toBe(false);
    expect(fs.existsSync(stillThereLock)).toBe(true);
    expect(fs.existsSync(liveLock)).toBe(true);
    expect(fs.existsSync(relativeLock)).toBe(true);
  });
});

describe("releaseFixtureLock", () => {
  it("does not delete a lock that has since become someone else's", () => {
    const dir = tempDir();
    const lock = acquireFixtureLock(dir, { timeoutMs: 2_000 });
    // The point of the test is that nothing here removes it, so the test must.
    dirs.push(lock.path);

    // Simulate this lock having been reclaimed and retaken by another process.
    fs.writeFileSync(
      lock.path,
      JSON.stringify({
        token: "someone-else",
        pid: process.pid,
        host: os.hostname(),
        startedAt: new Date().toISOString(),
        subject: dir,
      }),
    );

    releaseFixtureLock(lock);
    expect(fs.existsSync(lock.path)).toBe(true);
    expect(JSON.parse(fs.readFileSync(lock.path, "utf-8")).token).toBe("someone-else");
  });
});

describe("withFixtureLock", () => {
  it("releases the lock when the body throws", () => {
    const dir = tempDir();
    expect(() =>
      withFixtureLock(dir, () => {
        throw new Error("body exploded");
      }),
    ).toThrow(/body exploded/);
    expect(fs.existsSync(fixtureLockPath(dir))).toBe(false);
  });

  it("is re-entrant across sequential calls", () => {
    const dir = tempDir();
    expect(withFixtureLock(dir, () => 1)).toBe(1);
    expect(withFixtureLock(dir, () => 2)).toBe(2);
  });
});
