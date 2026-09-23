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
 * ## Completeness is the whole point, so a PARTIAL reading is not a reading
 *
 * The question this instrument answers is "what does the WHOLE session retain",
 * and the type-provider engine — the member most likely to retain a per-version
 * copy of every document — is a CHILD. A sample that covers the server root but
 * not that child is not a smaller measurement of the same thing; it is a
 * measurement of a different thing that can be flat while the session grows
 * without bound. Two failure modes therefore both make the sample UNAVAILABLE
 * rather than narrower:
 *
 *  - the platform cannot enumerate the process table, so the tree's membership
 *    is unknown and the root is all that could be sampled
 *    ({@link ProcessTreeRssUnavailable.kind} `"topology-unavailable"`);
 *  - a member WAS discovered but its RSS could not be read, so a known member's
 *    bytes are missing from the sum (`"member-unreadable"`).
 *
 * A member whose read fails because it EXITED between the enumeration and the
 * read is not such a member: a process that is gone retains nothing. Each
 * unreadable member is re-checked against a fresh process table, and only the
 * ones that table no longer places in the tree are dropped (and listed in
 * `exitedPids`). A member that is still there but unreadable keeps the sample
 * UNAVAILABLE.
 *
 * `totalBytes` is `null` in both cases and callers must report the metric
 * unavailable. Only a fully-observed tree yields a number, because only a fully
 * observed tree is the quantity the acceptance bound is written about.
 *
 * Ownership: the process-table enumeration and descendant walk live with the
 * corpus gate ({@link ../corpus-gate/processTree.js}), which owns
 * process-topology discovery for the whole harness; this module adds no second
 * implementation of either.
 */
import {
  descendantPids,
  processImage,
  snapshotProcessTable,
  type ProcessRow,
} from "../corpus-gate/processTree.js";
import { readProcessRssBytes } from "./rss.js";

/** One tree member's reading within a single {@link sampleProcessTreeRss} pass. */
export interface ProcessTreeRssMember {
  readonly pid: number;
  /** Image/command name as the platform reports it — evidence only. */
  readonly image: string | null;
  /** Resident bytes, or null when this member's read failed (e.g. it exited). */
  readonly rssBytes: number | null;
}

/** Why a sample is not a usable whole-tree observation. */
export interface ProcessTreeRssUnavailable {
  /**
   * `topology-unavailable`: the process table could not be enumerated, so the
   * tree's MEMBERSHIP is unknown — the provider child may or may not exist and
   * cannot be proven either way.
   *
   * `member-unreadable`: the membership is known, but at least one discovered
   * member's RSS could not be read, so a member the tree definitely has is
   * missing from the sum.
   */
  readonly kind: "topology-unavailable" | "member-unreadable";
  readonly detail: string;
}

/** One aligned whole-tree reading. */
export interface ProcessTreeRssSample {
  /**
   * True only when the platform enumerated the table AND every discovered
   * member's RSS was read. False means the metric is UNAVAILABLE for this
   * checkpoint — the caller must say so, never treat it as a passing
   * measurement of a narrower tree.
   */
  readonly observable: boolean;
  /** The whole tree's resident bytes, or null when the sample is incomplete. */
  readonly totalBytes: number | null;
  readonly members: readonly ProcessTreeRssMember[];
  /** Members discovered in the tree whose RSS could not be read in this pass. */
  readonly unreadablePids: readonly number[];
  /**
   * Members that were discovered but had exited by the time they were read
   * (absent from a fresh process table): short-lived children such as a
   * probe. They retain nothing and are not in `members`.
   */
  readonly exitedPids?: readonly number[];
  /** Null exactly when {@link observable} is true. */
  readonly unavailable: ProcessTreeRssUnavailable | null;
  readonly atMs: number;
}

/**
 * The platform readers this module depends on, injectable so the incomplete
 * paths — which cannot be provoked on a healthy host — are testable.
 */
export interface ProcessTreeRssDeps {
  readonly snapshotProcessTable: () => Promise<readonly ProcessRow[] | null>;
  readonly readProcessRssBytes: (pid: number) => Promise<number | null>;
}

const PLATFORM_DEPS: ProcessTreeRssDeps = { snapshotProcessTable, readProcessRssBytes };

/**
 * Read the resident set of `rootPid` and every descendant of it, in one pass.
 *
 * Returns an UNAVAILABLE sample (see {@link ProcessTreeRssUnavailable}) when the
 * tree's membership cannot be established or any established member cannot be
 * read. A narrower-but-honest figure is deliberately NOT offered: the caller's
 * only use for the number is a whole-session growth bound, which a root-only or
 * provider-less subset silently satisfies while the session leaks.
 */
export async function sampleProcessTreeRss(
  rootPid: number,
  deps: ProcessTreeRssDeps = PLATFORM_DEPS,
): Promise<ProcessTreeRssSample> {
  const atMs = Date.now();
  const rows = await deps.snapshotProcessTable();
  if (rows === null) {
    return {
      observable: false,
      totalBytes: null,
      members: [],
      unreadablePids: [],
      unavailable: {
        kind: "topology-unavailable",
        detail:
          `this platform (${process.platform}) could not enumerate the process table, so the ` +
          `tree rooted at pid ${rootPid} has unknown membership — the type-provider child ` +
          "cannot be proven present or absent, and a root-only reading is not a whole-tree one",
      },
      atMs,
    };
  }

  let pids = [rootPid, ...descendantPids(rows, rootPid)];
  let members: ProcessTreeRssMember[] = [];
  let unreadablePids: number[] = [];
  let total = 0;
  for (const pid of pids) {
    const rssBytes = await deps.readProcessRssBytes(pid);
    members.push({ pid, image: processImage(rows, pid), rssBytes });
    if (rssBytes === null) {
      unreadablePids.push(pid);
      continue;
    }
    total += rssBytes;
  }
  // A member whose read failed because it EXITED retained nothing. Re-enumerate
  // once and drop only the unreadable members the fresh table no longer places
  // in the tree; one still present stays unreadable. The root is never dropped.
  let exitedPids: number[] = [];
  if (unreadablePids.length > 0) {
    const fresh = await deps.snapshotProcessTable();
    if (fresh !== null) {
      const live = new Set([rootPid, ...descendantPids(fresh, rootPid)]);
      exitedPids = unreadablePids.filter((pid) => !live.has(pid));
      if (exitedPids.length > 0) {
        const exited = new Set(exitedPids);
        pids = pids.filter((pid) => !exited.has(pid));
        members = members.filter((member) => !exited.has(member.pid));
        unreadablePids = unreadablePids.filter((pid) => !exited.has(pid));
      }
    }
  }
  if (unreadablePids.length > 0) {
    return {
      observable: false,
      totalBytes: null,
      members,
      unreadablePids,
      exitedPids,
      unavailable: {
        kind: "member-unreadable",
        detail:
          `${unreadablePids.length} of ${pids.length} discovered tree member(s) could not be ` +
          `read (pid(s) ${unreadablePids.join(", ")}) — their bytes are missing from the sum, ` +
          "so this is not a whole-tree observation",
      },
      atMs,
    };
  }
  return {
    observable: true,
    totalBytes: total,
    members,
    unreadablePids,
    exitedPids,
    unavailable: null,
    atMs,
  };
}

/** Render a sample as a one-line evidence string (per-process, largest first). */
export function describeProcessTreeRss(sample: ProcessTreeRssSample): string {
  if (!sample.observable) {
    return `process-tree RSS UNAVAILABLE (${sample.unavailable?.kind ?? "unknown"}): ${
      sample.unavailable?.detail ?? "no detail"
    }`;
  }
  const parts = [...sample.members]
    .filter(
      (member): member is ProcessTreeRssMember & { rssBytes: number } => member.rssBytes !== null,
    )
    .sort((left, right) => right.rssBytes - left.rssBytes)
    .map((member) => `${member.image ?? "pid"}#${member.pid}=${mib(member.rssBytes)}`);
  return `${mib(sample.totalBytes ?? 0)} total [${parts.join(", ")}]`;
}

function mib(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(1)}MiB`;
}
