/**
 * Point-in-time resident-set reading for a spawned process TREE.
 *
 * {@link RssSampler} bounds ONE pid on a timer. That is the wrong instrument for
 * a retention question: the server's memory is only part of the cost an editing
 * session imposes, the type-provider engine runs in a CHILD process, and a
 * "did the session grow?" comparison needs two ALIGNED readings taken at points
 * the caller chose (after quiescence), not the maxima of two independently
 * started per-process timers.
 *
 * So this is a synchronous whole-tree read: enumerate the process table once,
 * take every descendant of the root, and read each one's RSS in that pass. The
 * tree membership is re-derived on EVERY sample, so a provider that respawned
 * between two samples is still attributed to the tree.
 *
 * Non-observability is explicit and never silently passes: a platform that
 * cannot enumerate the process table, or a tree in which no member's RSS could
 * be read, returns `observable: false` with `totalBytes: null`. A caller must
 * label the metric unavailable rather than compare against a fabricated zero.
 *
 * Ownership: the process-table enumeration and descendant walk live with the
 * corpus gate ({@link ../corpus-gate/processTree.js}), which owns
 * process-topology discovery for the whole harness; this module adds no second
 * implementation of either.
 */
import { descendantPids, processImage, snapshotProcessTable } from "../corpus-gate/processTree.js";
import { readProcessRssBytes } from "./rss.js";

/** One tree member's reading within a single {@link sampleProcessTreeRss} pass. */
export interface ProcessTreeRssMember {
  readonly pid: number;
  /** Image/command name as the platform reports it — evidence only. */
  readonly image: string | null;
  /** Resident bytes, or null when this member's read failed (e.g. it exited). */
  readonly rssBytes: number | null;
}

/** One aligned whole-tree reading. */
export interface ProcessTreeRssSample {
  /**
   * True when the platform enumerated the table AND at least one member's RSS
   * was readable. False means the metric is UNAVAILABLE on this host — the
   * caller must say so, never treat it as a passing measurement.
   */
  readonly observable: boolean;
  /** Sum over the members whose read succeeded, or null when not observable. */
  readonly totalBytes: number | null;
  readonly members: readonly ProcessTreeRssMember[];
  /** Members discovered in the tree whose RSS could not be read in this pass. */
  readonly unreadablePids: readonly number[];
  readonly atMs: number;
}

/**
 * Read the resident set of `rootPid` and every descendant of it, in one pass.
 *
 * `rootPid` itself is always a member even when the table is unavailable, so a
 * platform that can read one pid's RSS but not the process table still yields a
 * (narrower, honestly reported) figure instead of nothing.
 */
export async function sampleProcessTreeRss(rootPid: number): Promise<ProcessTreeRssSample> {
  const rows = await snapshotProcessTable();
  const pids = rows === null ? [rootPid] : [rootPid, ...descendantPids(rows, rootPid)];
  const members: ProcessTreeRssMember[] = [];
  const unreadablePids: number[] = [];
  let total = 0;
  let readAny = false;
  for (const pid of pids) {
    const rssBytes = await readProcessRssBytes(pid);
    members.push({ pid, image: rows === null ? null : processImage(rows, pid), rssBytes });
    if (rssBytes === null) {
      unreadablePids.push(pid);
      continue;
    }
    readAny = true;
    total += rssBytes;
  }
  return {
    observable: readAny,
    totalBytes: readAny ? total : null,
    members,
    unreadablePids,
    atMs: Date.now(),
  };
}

/** Render a sample as a one-line evidence string (per-process, largest first). */
export function describeProcessTreeRss(sample: ProcessTreeRssSample): string {
  if (!sample.observable) {
    return `process-tree RSS unobservable (${sample.unreadablePids.length} unreadable pid(s))`;
  }
  const parts = [...sample.members]
    .filter((member): member is ProcessTreeRssMember & { rssBytes: number } =>
      member.rssBytes !== null,
    )
    .sort((left, right) => right.rssBytes - left.rssBytes)
    .map((member) => `${member.image ?? "pid"}#${member.pid}=${mib(member.rssBytes)}`);
  return `${mib(sample.totalBytes ?? 0)} total [${parts.join(", ")}]`;
}

function mib(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
}
