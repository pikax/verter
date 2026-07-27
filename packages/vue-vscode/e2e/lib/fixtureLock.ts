/**
 * Exclusive, cross-process ownership of one fixture's dependency tree.
 *
 * Replacing a fixture's `node_modules` is DESTRUCTIVE, and the deciding read is
 * separated from the delete and the install by an `npm install` that runs for
 * seconds. Without ownership, two processes that both decide to install race:
 * one deletes the tree the other is still creating, and a launcher can end up
 * pointing VS Code at a directory another process is midway through replacing.
 *
 * The rules here are the ones `crates/verter_lsp/src/test_harness_fixture_dependencies.rs`
 * arrived at for the same hazard, not its mechanism — Node has no `flock`, and
 * this needs mutual exclusion around a critical section rather than publication
 * of a built tree:
 *
 *   - OWNERSHIP DECIDES, NOT AGE. A lock is reclaimed only when its recorded
 *     owner is provably gone. A slow install is indistinguishable from a hung one
 *     by wall clock, and a clock jump must not hand a live tree to a second
 *     writer.
 *   - VISIBLE AND OWNED IN THE SAME INSTANT. The lock is written to a private
 *     path and `link()`ed into place. `link` fails when the destination exists,
 *     so it is the atomic claim; and because the payload is already in the file,
 *     no one can observe a lock that exists but names no owner. (`rename` would
 *     be wrong here — it would silently overwrite a live owner's lock.) Where the
 *     filesystem has no hard links, an exclusive create takes over and trades
 *     only this property; see {@link claimExclusively}.
 *   - RECLAMATION IS CLAIMED, NOT RACED. Removing a dead owner's lock is itself
 *     exclusive: a reclaimer first claims `<lock>.reclaim-<dead token>` with the
 *     same `link`, then proves under that claim that the file it is about to
 *     remove is still the one it decided about. `unlink` names a path, and the
 *     decision was made about an owner; without both gates a process holding a
 *     stale read deletes the lock of the process that reclaimed first, and BOTH
 *     then enter the destructive section.
 *   - FAIL CLOSED. An unreadable, malformed, or foreign-host lock is treated as
 *     HELD, never as debris. Wrongly waiting costs time; wrongly stealing deletes
 *     a running suite's dependencies. A wedged lock surfaces as a timeout that
 *     names the file to remove, which is a decision for an operator to make.
 */

import { createHash, randomUUID } from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

/** Recorded so a later process can ask whether this owner still exists. */
interface LockOwner {
  readonly token: string;
  readonly pid: number;
  readonly host: string;
  readonly startedAt: string;
  readonly subject: string;
}

export interface FixtureLockOptions {
  /** How long to wait for a held lock before giving up. */
  readonly timeoutMs?: number;
  /** How long to sleep between attempts. */
  readonly pollMs?: number;
  /**
   * Whether a pid on THIS host is still running. Injected only so the reclaim
   * rule is testable without spawning; production always uses the real check.
   */
  readonly isProcessAlive?: (pid: number) => boolean;
  /**
   * The hard-link primitive. Injected only so the no-hard-link fallback is
   * testable on a filesystem that supports them; production always uses
   * `fs.linkSync`.
   */
  readonly link?: (existingPath: string, newPath: string) => void;
}

const DEFAULT_TIMEOUT_MS = 10 * 60_000;
const DEFAULT_POLL_MS = 50;

/** What a lock file is called, and therefore what the reaper recognises as one. */
const LOCK_SUFFIX = ".lock";

/** Locks live outside the repository — the fixture directory is git-tracked. */
function lockDirectory(): string {
  const dir = path.join(fs.realpathSync(os.tmpdir()), "verter-e2e-fixture-locks");
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * The lock file for a fixture directory.
 *
 * Keyed by the REAL path, so two spellings of one directory (a symlinked
 * checkout, `/tmp` vs `/private/tmp` on macOS) contend for the same lock instead
 * of each believing it holds one.
 */
export function fixtureLockPath(fixtureDir: string): string {
  let real = fixtureDir;
  try {
    real = fs.realpathSync(fixtureDir);
  } catch {
    // Not yet on disk: the lexical path is the best identity available.
  }
  const key = createHash("sha256").update(path.resolve(real)).digest("hex").slice(0, 32);
  return path.join(lockDirectory(), `${key}${LOCK_SUFFIX}`);
}

/** Whether a pid on this host is still running. */
function processIsAlive(pid: number): boolean {
  if (!Number.isInteger(pid) || pid <= 0) return true; // unusable pid → fail closed
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    // ESRCH: no such process. EPERM: alive, owned by another user — still alive.
    return (error as NodeJS.ErrnoException).code !== "ESRCH";
  }
}

/** A lock's payload together with the identity of the file it was read from. */
interface LockRead {
  readonly owner: LockOwner;
  /** `dev:ino` of the file the payload came from. */
  readonly file: string;
}

/**
 * Read a lock, tying its payload to the FILE it came from.
 *
 * The stat and the read share one descriptor deliberately: a path can be
 * unlinked and recreated between two syscalls, and a reclaim that acted on
 * "the file at this path" rather than "the file I read" would delete a lock
 * that had since become someone else's.
 */
function readLock(lockPath: string): LockRead | undefined {
  let fd: number | undefined;
  try {
    fd = fs.openSync(lockPath, "r");
    const stat = fs.fstatSync(fd);
    const parsed: unknown = JSON.parse(fs.readFileSync(fd, "utf-8"));
    if (
      parsed &&
      typeof parsed === "object" &&
      typeof (parsed as LockOwner).token === "string" &&
      typeof (parsed as LockOwner).pid === "number" &&
      typeof (parsed as LockOwner).host === "string"
    ) {
      return { owner: parsed as LockOwner, file: `${stat.dev}:${stat.ino}` };
    }
  } catch {
    // Absent, unreadable, or not our shape.
  } finally {
    if (fd !== undefined) {
      try {
        fs.closeSync(fd);
      } catch {
        /* already closed */
      }
    }
  }
  return undefined;
}

function readOwner(lockPath: string): LockOwner | undefined {
  return readLock(lockPath)?.owner;
}

/** Block this thread. The whole runner is synchronous (`execSync`), so the wait is too. */
function sleepSync(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * Whether a held lock may be reclaimed.
 *
 * Only one case says yes: a well-formed lock, written by THIS host, whose pid no
 * longer exists. A pid from another host is meaningless here, so a foreign lock
 * is held as far as this process is concerned.
 */
function lockIsReclaimable(
  owner: LockOwner | undefined,
  isAlive: (pid: number) => boolean,
): boolean {
  if (!owner) return false;
  if (owner.host !== os.hostname()) return false;
  return !isAlive(owner.pid);
}

function describeErrno(error: unknown): string {
  const errno = error as NodeJS.ErrnoException;
  return errno?.code ? `${errno.code}: ${errno.message}` : String(errno?.message ?? error);
}

/**
 * Errnos that mean the FILESYSTEM does not do hard links, as opposed to this
 * attempt not having worked.
 *
 * POSIX gives `link` EPERM for "the file system containing the files does not
 * support links", which is what Linux vfat reports; ENOTSUP/EOPNOTSUPP and
 * ENOSYS are the other spellings of the same statement, and Windows maps
 * `ERROR_ACCESS_DENIED` from a FAT/exFAT volume to EACCES. EACCES is in the set
 * for a reason worth stating: the fallback it enables creates a file in the same
 * directory with the same credentials, and a `pending` file was already written
 * there — so a permission answer that applies to `link` ALONE is operationally
 * "no hard links here". EXDEV cannot happen for a name beside its source, and is
 * here only because it is the same statement about the same operation.
 *
 * Deliberately NOT here: EIO, EBUSY, ESTALE, EAGAIN, ETIMEDOUT, ENOSPC, EROFS,
 * EMLINK. Each says an attempt failed. None says the capability is absent.
 */
const NO_HARD_LINK_CODES = new Set(["EPERM", "EACCES", "ENOTSUP", "EOPNOTSUPP", "ENOSYS", "EXDEV"]);

/** How many times the capability probe is attempted before it answers. */
const CAPABILITY_PROBE_ATTEMPTS = 3;

/** How many times a claim that failed for an unexplained reason is attempted. */
const CLAIM_ATTEMPTS = 3;

/**
 * Between attempts, so a retry is a second sample rather than the same instant.
 * Every loop that uses it sits behind a wall-clock deadline, so the cost is
 * polling granularity, never total wait.
 */
const RETRY_PAUSE_MS = 20;

/**
 * What this directory says about hard links, and how sure it is.
 *
 * Three answers, not two, because a probe that FAILS is proof of the attempt and
 * not of the capability — and only one of these three may engage the weaker
 * primitive.
 */
type HardLinkCapability =
  /** A probe link succeeded here. Proof. */
  | { readonly kind: "available" }
  /** Every attempt failed, and every failure SAYS the capability is absent. */
  | { readonly kind: "unsupported"; readonly detail: string }
  /** Every attempt failed, and the failures do not say why. */
  | { readonly kind: "indeterminate"; readonly detail: string };

/**
 * Ask this directory whether it can do hard links AT ALL.
 *
 * Asked only after a link has already failed, and only to decide whether that
 * failure was the filesystem saying "not supported" or something else. A probe
 * rather than an errno list ALONE, because an errno list is a guess about what
 * each platform reports for an unsupported operation — while a link that
 * succeeds in this very directory, between two files this process owns, is
 * proof.
 *
 * A probe that fails proves nothing by itself, which is the asymmetry this
 * function exists to keep. One IO error can hit the claim AND the probe — a
 * network mount having a fit reaches both — and reading that as "no hard links
 * here" engages a primitive that is NOT atomic on NFSv3, on precisely the kind
 * of mount that is one. So a failed probe is classified rather than counted:
 *
 *   - a BOUNDED RETRY, so a failure has to persist to be believed at all; and
 *   - {@link NO_HARD_LINK_CODES}, so the answer "this filesystem has no hard
 *     links" is only ever returned by a failure that SAYS so. Anything else is
 *     `indeterminate`, and the caller refuses.
 *
 * The probe is a name under `pending`, so it carries this process's payload and
 * matches the `.pending-` shape {@link reapAbandonedFiles} already collects: one
 * killed between the link and the unlink leaves debris a later run removes.
 */
function hardLinkCapability(
  pending: string,
  link: (existingPath: string, newPath: string) => void,
): HardLinkCapability {
  const failures: unknown[] = [];
  for (let attempt = 0; attempt < CAPABILITY_PROBE_ATTEMPTS; attempt += 1) {
    if (attempt > 0) sleepSync(RETRY_PAUSE_MS);
    // A fresh name per probe: `link` refuses a destination that exists, so a
    // probe one earlier attempt could not unlink would answer EEXIST here and be
    // read as "this filesystem has no hard links" — the one answer that must be
    // earned.
    const probe = `${pending}-hardlink-probe-${randomUUID().slice(0, 8)}`;
    try {
      link(pending, probe);
    } catch (error) {
      failures.push(error);
      continue;
    }
    try {
      fs.unlinkSync(probe);
    } catch {
      /* left for the reaper */
    }
    return { kind: "available" };
  }

  const detail = failures.map(describeErrno).join(", then ");
  // `every` on an empty list is true, and "no attempt was made" must never read
  // as "the capability is absent".
  const saysUnsupported =
    failures.length > 0 &&
    failures.every((error) => NO_HARD_LINK_CODES.has((error as NodeJS.ErrnoException)?.code ?? ""));
  return saysUnsupported ? { kind: "unsupported", detail } : { kind: "indeterminate", detail };
}

/** One attempt at the atomic claim, with the two ordinary outcomes named. */
type LinkAttempt =
  | { readonly outcome: "claimed" }
  | { readonly outcome: "taken" }
  | { readonly outcome: "failed"; readonly error: unknown };

function attemptLink(
  target: string,
  pending: string,
  link: (existingPath: string, newPath: string) => void,
): LinkAttempt {
  try {
    link(pending, target);
    return { outcome: "claimed" };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "EEXIST") return { outcome: "taken" };
    return { outcome: "failed", error };
  }
}

/**
 * Publish `payload` at `target`, or report that someone got there first.
 *
 * `link` is the primitive because it is exclusive AND atomic in one step: the
 * destination cannot exist, and the payload is already in the file, so no reader
 * can see a lock that exists but names no owner.
 *
 * Hard links are not universal, though. A Windows `%TEMP%` on FAT or exFAT
 * refuses `link` while ordinary writes succeed, and rethrowing that raw fails
 * every run on those machines with an errno naming neither the cause nor the
 * constraint. An exclusive create is the portable second choice — and it is a
 * WEAKER one, in two ways that are stated here because the fallback's scope is
 * decided by them:
 *
 *   - the payload lands in a second step, so a process killed between them
 *     leaves an EMPTY file. Every rule here reads that as unparseable and so as
 *     HELD, which is the fail-closed direction — but it leaves a residue in each
 *     of the two files this function publishes, and both are recorded here
 *     because neither is fixed:
 *
 *     an empty LOCK is treated as held, so the outcome is the timeout
 *     diagnostic naming the file rather than a stolen lock. Nothing reclaims it:
 *     an unparseable lock names no owner, so no rule here can prove it is
 *     debris.
 *
 *     an empty RECLAIM CLAIM is worse in one respect, because the reaper's gate
 *     for a `.reclaim-` file is its OWNER, read from the payload — so an empty
 *     one is never collected either, and while it exists no process can ever
 *     reclaim that owner's generation: every reclaimer finds the claim taken and
 *     goes back to waiting. If the lock it guarded is still there, the fixture
 *     stays wedged until an operator acts, which surfaces as the same timeout
 *     naming the lock. Removing the lock frees the fixture and leaves the claim
 *     behind as debris nothing collects. Tokens are UUIDs, so the wedged
 *     generation never recurs.
 *
 *     Both are reachable only through THIS fallback, and only from a kill inside
 *     the two syscalls: the `link` path publishes a payload that is already in
 *     the file, so there is no instant at which either file exists empty.
 *   - `O_EXCL` is NOT atomic on NFSv3. Two contenders on such a mount can both
 *     be told they created the file, and both would then report `"claimed"`.
 *     `link` does not have that problem — it is the primitive NFS clients
 *     implement exclusively — which is why it stays the first choice, and why a
 *     mount that refuses `link` is precisely the one where this second choice is
 *     least trustworthy.
 *
 * So it engages ONLY where hard links are POSITIVELY CLASSIFIED as unavailable
 * by {@link hardLinkCapability} — a probe that fails every attempt, with an
 * errno that says the capability is absent — never inferred from the claim's own
 * errno and never from a probe that merely failed. Everything else refuses:
 *
 *   - a failure this directory contradicts (`available`) is not a missing
 *     capability, whatever it was;
 *   - a failure the probe cannot explain (`indeterminate`) is not proof of one
 *     either, and treating it as one is how a single EIO storm hands the run to
 *     the primitive that is unsafe on the mounts most likely to have produced
 *     it.
 *
 * Neither of those is fatal on the first sample, though. An unexplained failure
 * that does not recur is a blip, and ending a ten-minute suite over one syscall
 * is its own defect — so the claim is RETRIED, bounded, and only a failure that
 * survives the retries refuses. Fail-closed is where this ends up, not where it
 * starts.
 */
function claimExclusively(
  target: string,
  pending: string,
  payload: string,
  link: (existingPath: string, newPath: string) => void,
): "claimed" | "taken" {
  const first = attemptLink(target, pending, link);
  if (first.outcome !== "failed") return first.outcome;

  const capability = hardLinkCapability(pending, link);
  if (capability.kind !== "unsupported") {
    let linkError = first.error;
    for (let attempt = 1; attempt < CLAIM_ATTEMPTS; attempt += 1) {
      sleepSync(RETRY_PAUSE_MS);
      const retry = attemptLink(target, pending, link);
      if (retry.outcome !== "failed") return retry.outcome;
      linkError = retry.error;
    }
    throw new Error(
      `cannot create the fixture dependency lock file ${target}.\n` +
        `  hard link (${CLAIM_ATTEMPTS} attempts): ${describeErrno(linkError)}\n` +
        (capability.kind === "available"
          ? `  This directory DOES support hard links — a probe link beside that path ` +
            `succeeded — so the failure above is not a missing capability.\n`
          : `  capability probe (${CAPABILITY_PROBE_ATTEMPTS} attempts): ${capability.detail}\n` +
            `  The probe could not establish whether this directory supports hard links ` +
            `either, and a failure that does not say the capability is absent is not proof ` +
            `that it is.\n`) +
        `  The weaker exclusive-create fallback engages only where hard links are ` +
        `demonstrably unavailable, because it is not atomic on NFSv3: on such a mount it ` +
        `would report an exclusive claim to two processes at once. Fixture dependency ` +
        `installs are serialised through this file, so the run stops here rather than ` +
        `replacing a node_modules tree unserialised.` +
        (capability.kind === "available"
          ? ""
          : `\n  If this filesystem genuinely has no hard links, it is saying so with an errno ` +
            `this harness does not recognise as that statement — report the one above so it ` +
            `can be classified, or point TMPDIR/TEMP at a directory that does.`),
    );
  }

  try {
    const fd = fs.openSync(target, "wx");
    try {
      fs.writeFileSync(fd, payload);
    } finally {
      fs.closeSync(fd);
    }
    return "claimed";
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "EEXIST") return "taken";
    throw new Error(
      `cannot create the fixture dependency lock file ${target}.\n` +
        `  hard link:        ${describeErrno(first.error)}\n` +
        `  capability probe: ${capability.detail}\n` +
        `  exclusive create: ${describeErrno(error)}\n` +
        `  This directory must accept either a hard link or a newly created file. ` +
        `Hard links need a filesystem that supports them (a Windows %TEMP% on FAT or ` +
        `exFAT does not) and both need it writable; point TMPDIR/TEMP at such a ` +
        `directory. Fixture dependency installs are serialised through this file, so ` +
        `the run stops here rather than replacing a node_modules tree unserialised.`,
    );
  }
}

/**
 * The exclusive right to remove ONE dead owner's lock.
 *
 * Derived from the dead owner's token, so it is the reclamation of that exact
 * lock generation that is claimed — and `link` refuses a destination that
 * exists, so exactly one process ever holds it. A claim is never stolen, not
 * even from a dead claimant: two concurrent holders would each pass their own
 * check and the second would delete the lock the first had already replaced,
 * which is the defect the claim exists to prevent.
 *
 * The token is HASHED rather than interpolated. It is read from a file this
 * process did not write, and a value read from disk must not shape a path: a
 * crafted token (`../..`, an absolute path) otherwise decided where a claim was
 * created and deleted. A hash is a fixed-width name inside this directory by
 * construction, so there is no rule to get wrong and nothing to validate — and
 * it is still a function of the token alone, which is what makes two processes
 * reclaiming the same generation contend for the same claim.
 */
function reclaimClaimPath(lockPath: string, deadToken: string): string {
  const generation = createHash("sha256").update(deadToken).digest("hex").slice(0, 32);
  return `${lockPath}.reclaim-${generation}`;
}

/**
 * Remove a dead owner's lock, or report that someone else is dealing with it.
 *
 * Removal is gated twice, because `unlink` names a PATH and the decision to
 * reclaim was made about an OWNER:
 *
 *   1. the reclaim claim, so at most one process reaches the removal at all; and
 *   2. a re-read under that claim, so the file about to be removed is still the
 *      very file the decision was made about — not a live successor that took
 *      the path while this process was deciding.
 *
 * Without (1) the re-read is only a narrower window, and without (2) the claim
 * still lets the reclaimer of a generation delete its successor.
 */
function reclaimDeadLock(
  lockPath: string,
  dead: LockRead,
  pending: string,
  payload: string,
  link: (existingPath: string, newPath: string) => void,
): boolean {
  const claimPath = reclaimClaimPath(lockPath, dead.owner.token);
  try {
    // Taken: another process owns this reclamation. Thrown: the claim could not
    // be established at all, which is not a licence to remove the lock either.
    if (claimExclusively(claimPath, pending, payload, link) !== "claimed") return false;
  } catch {
    return false;
  }
  try {
    const current = readLock(lockPath);
    if (!current || current.owner.token !== dead.owner.token || current.file !== dead.file) {
      return false;
    }
    try {
      fs.unlinkSync(lockPath);
    } catch (error) {
      // ENOENT: already gone, which is the outcome asked for. Anything else
      // (a Windows scanner holding the file open reports EPERM/EBUSY) means the
      // lock is still there, so it stays HELD rather than being retried hot.
      return (error as NodeJS.ErrnoException).code === "ENOENT";
    }
    return true;
  } finally {
    try {
      fs.unlinkSync(claimPath);
    } catch (error) {
      // ENOENT is the outcome asked for: never created, or already reaped.
      // Anything else is the same errno class the lock's own unlink handles
      // explicitly ten lines above, and swallowing it here is worse: while that
      // claim exists, this owner's generation can never be reclaimed again. The
      // lock it guarded is already gone, so nothing is blocked now, and the
      // reaper collects the claim once this process is — but a wedge nobody can
      // see is not one anybody fixes.
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
        console.warn(
          `\n  Could not remove the fixture dependency lock reclaim claim (${describeErrno(
            error,
          )}):\n    ${claimPath}\n` +
            `  The lock it guarded was reclaimed, so nothing is waiting on it. While that ` +
            `file remains, this owner's lock generation cannot be reclaimed again; a later ` +
            `run removes it once this process is gone. Remove it by hand if one does not.\n`,
        );
      }
    }
  }
}

/**
 * Whether a lock names a fixture that is no longer on disk.
 *
 * A lock whose fixture still exists will be reclaimed by the next process that
 * wants that fixture, under the claim, which is where the decision belongs. A
 * lock whose fixture is GONE will not: no such process can ever run, so nothing
 * else will ever collect it.
 *
 * Fail closed on anything unclear. A lock with no recorded subject says nothing
 * about a fixture, and is left where it is.
 */
function subjectIsGone(owner: LockOwner): boolean {
  if (typeof owner.subject !== "string" || owner.subject.length === 0) return false;
  // A relative subject means whatever the writer's working directory was, and
  // this reader's is a different question. Answering it from here would be
  // answering a different one.
  if (!path.isAbsolute(owner.subject)) return false;
  return !fs.existsSync(owner.subject);
}

/**
 * Remove lock-directory debris left by processes that are provably gone.
 *
 * Three things accumulate. A process killed between writing its private payload
 * and claiming the lock leaves a `.pending-` file nothing removes; one killed
 * inside the two syscalls of a reclaim leaves a `.reclaim-` claim; and a process
 * killed while HOLDING a lock over a fixture that is itself transient leaves the
 * `.lock`. All three pile up forever in a directory shared by every fixture and
 * every run.
 *
 * The predicate is the lock's own: this host, and an owner that no longer
 * exists. Unreadable debris is left alone — it may be a file another process is
 * midway through writing, and an unparseable lock names no owner at all, so
 * nothing here can prove it is debris rather than a live claim. An EMPTY file is
 * the one shape that can be neither collected here nor reclaimed elsewhere; it
 * comes from one place, and what it costs is recorded at {@link claimExclusively}.
 *
 * The three differ in HOW they are removed, and the difference is the point:
 *
 *   - a `.pending-` file is private to its writer, so an unlink is enough.
 *   - a `.reclaim-` claim is debris ONLY once the lock it names is gone. While
 *     that lock exists the claim is the only thing preventing two processes from
 *     removing it, so it is left in place even when its claimant is dead: the
 *     wedge is recoverable by an operator, and handing the claim to a second
 *     process is not.
 *   - a `.lock` is removed through {@link reclaimDeadLock} and never by an
 *     unlink here, because removing one is exactly what that claim exists to
 *     serialise — and ONLY when its fixture is gone, which is the one shape no
 *     ordinary acquire can ever reach.
 */
function reapAbandonedFiles(
  isAlive: (pid: number) => boolean,
  pending: string,
  payload: string,
  link: (existingPath: string, newPath: string) => void,
): void {
  const directory = lockDirectory();
  let entries: string[];
  try {
    entries = fs.readdirSync(directory);
  } catch {
    return;
  }
  // This process's own payload, and any probe named beside it, are in this
  // directory while the sweep runs — the payload deliberately so, because the
  // reclaim below publishes it. They are not debris under any predicate: the one
  // owner this sweep can prove is not gone is the one performing it. Left to
  // `isAlive`, an answer of "gone" deletes the very file the claim is about to
  // link FROM, and the claim fails ENOENT for a reason that has nothing to do
  // with locking.
  const own = path.basename(pending);
  for (const name of entries) {
    if (name.startsWith(own)) continue;
    const file = path.join(directory, name);
    const reclaimAt = name.indexOf(".reclaim-");
    if (reclaimAt < 0 && !name.includes(".pending-")) {
      if (!name.endsWith(LOCK_SUFFIX)) continue;
      const held = readLock(file);
      if (!held || !lockIsReclaimable(held.owner, isAlive) || !subjectIsGone(held.owner)) continue;
      // Best effort, and gated exactly as an acquirer's reclaim is: another
      // process may hold the claim, or may have taken the lock since it was
      // read, and either answer is "leave it".
      reclaimDeadLock(file, held, pending, payload, link);
      continue;
    }
    if (!lockIsReclaimable(readOwner(file), isAlive)) continue;
    if (reclaimAt >= 0 && fs.existsSync(path.join(directory, name.slice(0, reclaimAt)))) continue;
    try {
      fs.unlinkSync(file);
    } catch {
      /* raced with another sweep, or with its owner's own cleanup */
    }
  }
}

export interface FixtureLock {
  readonly path: string;
  readonly token: string;
}

/**
 * Take exclusive ownership of `fixtureDir`, or throw once `timeoutMs` elapses.
 *
 * Prefer {@link withFixtureLock}, which cannot leak the lock on an early return
 * or a throw.
 */
export function acquireFixtureLock(
  fixtureDir: string,
  options: FixtureLockOptions = {},
): FixtureLock {
  const lockPath = fixtureLockPath(fixtureDir);
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const pollMs = options.pollMs ?? DEFAULT_POLL_MS;
  const isAlive = options.isProcessAlive ?? processIsAlive;
  const link = options.link ?? fs.linkSync;

  const token = randomUUID();
  const pending = `${lockPath}.pending-${process.pid}-${token}`;
  const payload = JSON.stringify(
    {
      token,
      pid: process.pid,
      host: os.hostname(),
      startedAt: new Date().toISOString(),
      subject: fixtureDir,
    } satisfies LockOwner,
    null,
    2,
  );
  fs.writeFileSync(pending, payload);

  // After the pending file exists, because the sweep reclaims through the same
  // claim an acquirer uses and that claim is published by linking this payload.
  reapAbandonedFiles(isAlive, pending, payload, link);

  const deadline = Date.now() + timeoutMs;
  try {
    for (;;) {
      if (claimExclusively(lockPath, pending, payload, link) === "claimed") {
        return { path: lockPath, token };
      }

      // A reclaim that cannot proceed — the claim is another process's, or the
      // removal failed with anything but ENOENT — is a WAIT, not a retry. It
      // falls through to the deadline and the poll below with every other
      // branch. Looping back to the link from here instead is an uninterruptible
      // 100%-CPU spin: both the deadline test and the sleep sit below it, and a
      // synchronous loop never yields to a test timeout either.
      const held = readLock(lockPath);
      if (
        held &&
        lockIsReclaimable(held.owner, isAlive) &&
        reclaimDeadLock(lockPath, held, pending, payload, link)
      ) {
        // The path is free; race for it immediately rather than sleeping. It is
        // not ours yet — another waiter may link first, and that is fine.
        //
        // This is the one branch that skips the deadline, and it terminates for
        // a reason the others do not need: it consumes a dead lock GENERATION,
        // and a generation is reclaimed once. Repeating it requires other
        // processes to keep taking the lock and dying, which is progress rather
        // than a spin — and every such iteration re-attempts the link above.
        continue;
      }

      if (Date.now() >= deadline) {
        const owner = readOwner(lockPath);
        throw new Error(
          `timed out after ${timeoutMs}ms waiting for the fixture dependency lock on ` +
            `${fixtureDir}.\n  lock: ${lockPath}\n  held by: ${
              owner
                ? `pid ${owner.pid} on ${owner.host} since ${owner.startedAt}`
                : "an unreadable lock file"
            }\n` +
            `  If no such process is running, remove that file. It is not removed ` +
            `automatically: deleting a lock whose owner might be alive would let a second ` +
            `process delete this fixture's node_modules mid-install.`,
        );
      }
      sleepSync(pollMs);
    }
  } finally {
    try {
      fs.unlinkSync(pending);
    } catch {
      /* already gone */
    }
  }
}

/** Release a lock, but only if it is still ours. */
export function releaseFixtureLock(lock: FixtureLock): void {
  const owner = readOwner(lock.path);
  // A reclaimed lock now belongs to someone else; deleting it would be the very
  // race this module exists to prevent.
  if (owner?.token !== lock.token) return;
  try {
    fs.unlinkSync(lock.path);
  } catch {
    /* already gone */
  }
}

/** Run `body` under exclusive ownership of `fixtureDir`. */
export function withFixtureLock<T>(
  fixtureDir: string,
  body: () => T,
  options: FixtureLockOptions = {},
): T {
  const lock = acquireFixtureLock(fixtureDir, options);
  try {
    return body();
  } finally {
    releaseFixtureLock(lock);
  }
}
