/**
 * Process-tree discovery and provider-sample attribution for the corpus gate.
 *
 * The gate bounds per-process memory. Sampling ONE advertised pid cannot do
 * that: a route observed a `node` child holding ~3.9 GB while the receipt
 * recorded that route's provider at 357 MB — the ceiling could not see the
 * process it exists to bound. Two fixes, both here:
 *
 *  1. TREE COVERAGE. Everything the harness spawned is sampled: the server
 *     process, the relay (when the route has one), and every descendant of
 *     either. A tree member discovered but never sampled is recorded as an
 *     `unattributedPid` — a LOUD failure, because that is exactly the shape of
 *     a blow-up escaping the ceiling.
 *  2. STRUCTURAL ATTRIBUTION. The provider pid is verified STRUCTURALLY — it
 *     must exist in the process table and be a descendant of the server or of
 *     the relay the harness itself spawned. Matching on process names would be
 *     brittle and would silently pass the wrong process; parentage cannot be
 *     faked by a rename. A pid that is the server itself, the harness itself,
 *     or outside both trees is `mismatched` and FAILS.
 *
 * When the platform cannot enumerate processes at all the status is
 * `unobservable`: an explicit, recorded non-enforcement — never a silent pass.
 */
import { execFile } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { promisify } from "node:util";

import { downsampleSeries } from "./metrics.js";
import { RssSampler } from "../endurance/rss.js";
import type {
  CorpusProcessMemoryTrend,
  CorpusProcessRole,
  CorpusProviderAttribution,
  CorpusProviderAttributionStatus,
} from "./types.js";

const execFileAsync = promisify(execFile);

/** One row of the system process table. */
export interface ProcessRow {
  readonly pid: number;
  readonly ppid: number;
  /** Image/command name as reported by the platform (evidence only). */
  readonly image: string;
}

/** Hard cap on tree walks so a pathological table can never spin the gate. */
const MAX_TREE_NODES = 512;

/** Read the whole process table, or null when the platform cannot report it. */
export async function snapshotProcessTable(): Promise<readonly ProcessRow[] | null> {
  try {
    if (process.platform === "linux") return await snapshotLinux();
    if (process.platform === "darwin") return await snapshotDarwin();
    if (process.platform === "win32") return await snapshotWindows();
    return null;
  } catch {
    return null;
  }
}

async function snapshotLinux(): Promise<readonly ProcessRow[]> {
  const entries = await readdir("/proc");
  const rows: ProcessRow[] = [];
  for (const entry of entries) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const stat = await readFile(`/proc/${entry}/stat`, "utf8");
      // `pid (comm) state ppid ...`; comm may contain spaces and parens, so the
      // fields after it are found from the LAST ')'.
      const close = stat.lastIndexOf(")");
      const open = stat.indexOf("(");
      if (close < 0 || open < 0 || close < open) continue;
      const image = stat.slice(open + 1, close);
      const rest = stat.slice(close + 2).split(/\s+/);
      const ppid = Number(rest[1]);
      const pid = Number(entry);
      if (!Number.isSafeInteger(pid) || !Number.isSafeInteger(ppid)) continue;
      rows.push({ pid, ppid, image });
    } catch {
      // The process exited between readdir and read: not an error, just gone.
    }
  }
  return rows;
}

async function snapshotDarwin(): Promise<readonly ProcessRow[]> {
  const { stdout } = await execFileAsync("ps", ["-Ao", "pid=,ppid=,comm="], {
    maxBuffer: 32 * 1024 * 1024,
    timeout: 20_000,
  });
  const rows: ProcessRow[] = [];
  for (const line of stdout.split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line);
    if (!match) continue;
    rows.push({ pid: Number(match[1]), ppid: Number(match[2]), image: match[3].trim() });
  }
  return rows;
}

async function snapshotWindows(): Promise<readonly ProcessRow[]> {
  const { stdout } = await execFileAsync(
    "powershell",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Json -Compress",
    ],
    { maxBuffer: 64 * 1024 * 1024, timeout: 30_000, windowsHide: true },
  );
  const parsed: unknown = JSON.parse(stdout);
  const list = Array.isArray(parsed) ? parsed : [parsed];
  const rows: ProcessRow[] = [];
  for (const entry of list) {
    const record = entry as { ProcessId?: unknown; ParentProcessId?: unknown; Name?: unknown };
    const pid = Number(record?.ProcessId);
    const ppid = Number(record?.ParentProcessId);
    if (!Number.isSafeInteger(pid) || !Number.isSafeInteger(ppid)) continue;
    rows.push({ pid, ppid, image: typeof record.Name === "string" ? record.Name : "" });
  }
  return rows;
}

/** Every descendant pid of `rootPid` (excluding the root), cycle-safe (pure). */
export function descendantPids(rows: readonly ProcessRow[], rootPid: number): number[] {
  const children = new Map<number, number[]>();
  for (const row of rows) {
    if (row.pid === row.ppid) continue; // self-parented rows cannot form a tree
    const bucket = children.get(row.ppid);
    if (bucket) bucket.push(row.pid);
    else children.set(row.ppid, [row.pid]);
  }
  const seen = new Set<number>([rootPid]);
  const out: number[] = [];
  const queue: number[] = [rootPid];
  while (queue.length > 0 && out.length < MAX_TREE_NODES) {
    const current = queue.shift() as number;
    for (const child of children.get(current) ?? []) {
      if (seen.has(child)) continue;
      seen.add(child);
      out.push(child);
      queue.push(child);
    }
  }
  return out;
}

/** Image name for `pid`, or null when the table has no such row (pure). */
export function processImage(rows: readonly ProcessRow[], pid: number): string | null {
  return rows.find((row) => row.pid === pid)?.image ?? null;
}

export interface AttributionInput {
  /** The spawned `verter-lsp` pid. */
  readonly serverPid: number | null;
  /** The relay pid, for routes that own one (its subtree holds the provider). */
  readonly relayPid: number | null;
  /** The pid the server advertised as its type provider. */
  readonly providerPid: number | null;
  /** The process table, or null when the platform could not report it. */
  readonly rows: readonly ProcessRow[] | null;
  /** This harness process, so self-attribution is caught rather than trusted. */
  readonly harnessPid: number;
}

/**
 * Classify the provider attribution STRUCTURALLY (pure). Returns the status
 * plus the evidence string; the caller adds tree-coverage facts.
 */
export function classifyProviderAttribution(input: AttributionInput): {
  readonly status: CorpusProviderAttributionStatus;
  readonly detail: string;
  readonly image: string | null;
} {
  const { rows, providerPid, serverPid, relayPid, harnessPid } = input;
  if (rows === null) {
    return {
      status: "unobservable",
      detail:
        `this platform (${process.platform}) could not enumerate the process table — ` +
        `the provider RSS ceiling is explicitly UNENFORCED for this route`,
      image: null,
    };
  }
  if (providerPid === null) {
    return {
      status: "missing",
      detail:
        "no provider process was ever advertised or observed — the per-process RSS ceiling " +
        "had no provider to bound",
      image: null,
    };
  }
  const image = processImage(rows, providerPid);
  if (image === null) {
    return {
      status: "missing",
      detail: `advertised provider pid ${providerPid} is absent from the process table`,
      image: null,
    };
  }
  if (providerPid === harnessPid) {
    return {
      status: "mismatched",
      detail: `provider pid ${providerPid} is the HARNESS process itself (${image})`,
      image,
    };
  }
  if (serverPid !== null && providerPid === serverPid) {
    return {
      status: "mismatched",
      detail: `provider pid ${providerPid} is the verter-lsp process itself (${image})`,
      image,
    };
  }
  const roots = [serverPid, relayPid].filter((pid): pid is number => pid !== null);
  const inTree = roots.some((root) => descendantPids(rows, root).includes(providerPid));
  if (!inTree) {
    return {
      status: "mismatched",
      detail:
        `provider pid ${providerPid} (${image}) is not a descendant of the spawned server` +
        `${relayPid !== null ? " or relay" : ""} ` +
        `[roots: ${roots.join(", ") || "none"}] — the sampler is bounding the wrong process`,
      image,
    };
  }
  return {
    status: "verified",
    detail: `provider pid ${providerPid} (${image}) is a descendant of the spawned tree`,
    image,
  };
}

interface TrackedProcess {
  readonly pid: number;
  role: CorpusProcessRole;
  label: string;
  parentPid: number | null;
  image: string | null;
  readonly discoveredAtMs: number;
  readonly sampler: RssSampler;
}

/**
 * Tree members discovered but never actually sampled — unbounded memory (pure).
 *
 * A member discovered less than `graceMs` ago is excluded: its first read is
 * still in flight, and a false alarm on a teardown race would be noise, not
 * evidence. A member that never sampled while OTHER members sampled fine is
 * real: the platform works, this process was simply never bounded.
 */
export function unsampledTreeMembers(
  members: readonly {
    readonly pid: number;
    readonly discoveredAtMs: number;
    readonly maxRssBytes: number | null;
  }[],
  nowMs: number,
  graceMs: number,
): number[] {
  const anySampled = members.some((member) => member.maxRssBytes !== null);
  if (!anySampled) return []; // platform-wide read failure is `unobservable`, not a gap
  return members
    .filter((member) => member.maxRssBytes === null && nowMs - member.discoveredAtMs >= graceMs)
    .map((member) => member.pid);
}

export interface ProcessTreeRoots {
  readonly serverPid: number | null;
  readonly relayPid: number | null;
  readonly providerPid: number | null;
}

const MEMORY_SERIES_MAX_POINTS = 60;

/**
 * Samples RSS for the WHOLE spawned tree and records provable attribution.
 *
 * Topology (who is whose child) is refreshed on a slower cadence than RSS —
 * enumerating the process table is the expensive call, while reading one pid's
 * RSS is cheap — so the sampler stays affordable at a 2 s RSS interval.
 */
export class ProcessTreeSampler {
  private readonly tracked = new Map<number, TrackedProcess>();
  private readonly unattributed = new Set<number>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private roots: ProcessTreeRoots = { serverPid: null, relayPid: null, providerPid: null };
  private lastRows: readonly ProcessRow[] | null = null;
  private tableObservable: boolean | null = null;
  private attributionStatus: CorpusProviderAttributionStatus = "missing";
  private attributionDetail = "the process tree was never sampled";

  constructor(
    private readonly intervalMs: number,
    private readonly harnessPid: number = process.pid,
  ) {}

  /** Update the known roots (the provider pid arrives after startup). */
  setRoots(roots: ProcessTreeRoots): void {
    this.roots = roots;
  }

  start(): void {
    if (this.timer) return;
    void this.refreshTopology();
    this.timer = setInterval(() => void this.refreshTopology(), Math.max(this.intervalMs, 1_000));
    this.timer.unref?.();
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    for (const entry of this.tracked.values()) entry.sampler.stop();
  }

  /**
   * Re-enumerate the tree, start samplers for newly discovered members, and
   * re-classify provider attribution. Safe to call concurrently with sampling.
   */
  async refreshTopology(): Promise<void> {
    const rows = await snapshotProcessTable();
    if (rows !== null) {
      this.lastRows = rows;
      this.tableObservable = true;
    } else if (this.tableObservable === null) {
      this.tableObservable = false;
    }

    const { serverPid, relayPid, providerPid } = this.roots;
    const attribution = classifyProviderAttribution({
      serverPid,
      relayPid,
      providerPid,
      rows: this.lastRows,
      harnessPid: this.harnessPid,
    });
    // `missing` is only meaningful once the server has actually advertised a
    // provider; keep the last non-missing verdict from going backwards when a
    // provider legitimately exits during teardown.
    if (!(attribution.status === "missing" && this.attributionStatus === "verified")) {
      this.attributionStatus = attribution.status;
      this.attributionDetail = attribution.detail;
    }

    const providerSubtree = new Set<number>(
      providerPid !== null && this.lastRows !== null
        ? [providerPid, ...descendantPids(this.lastRows, providerPid)]
        : providerPid !== null
          ? [providerPid]
          : [],
    );
    const discovered = new Map<number, CorpusProcessRole>();
    if (serverPid !== null) discovered.set(serverPid, "server");
    for (const root of [serverPid, relayPid]) {
      if (root === null || this.lastRows === null) continue;
      for (const pid of descendantPids(this.lastRows, root)) {
        discovered.set(pid, providerSubtree.has(pid) ? "provider" : "descendant");
      }
    }
    if (relayPid !== null) discovered.set(relayPid, discovered.get(relayPid) ?? "descendant");
    for (const pid of providerSubtree) discovered.set(pid, "provider");

    for (const [pid, role] of discovered) {
      const existing = this.tracked.get(pid);
      if (existing) {
        // A descendant can be re-classified once the provider pid is known.
        if (existing.role !== "server" && role === "provider") {
          existing.role = "provider";
          existing.label = pid === providerPid ? "provider" : "provider-child";
        }
        continue;
      }
      if (this.tracked.size >= MAX_TREE_NODES) {
        this.unattributed.add(pid);
        continue;
      }
      const sampler = new RssSampler(pid, this.intervalMs);
      sampler.start();
      const row = this.lastRows?.find((candidate) => candidate.pid === pid) ?? null;
      this.tracked.set(pid, {
        pid,
        role,
        discoveredAtMs: Date.now(),
        label:
          role === "server"
            ? "verter-lsp"
            : pid === providerPid
              ? "provider"
              : role === "provider"
                ? "provider-child"
                : "server-descendant",
        parentPid: row?.ppid ?? null,
        image: row?.image ?? null,
        sampler,
      });
    }
  }

  /** Per-process RSS trends for every sampled tree member. */
  trends(): CorpusProcessMemoryTrend[] {
    return [...this.tracked.values()]
      .sort((left, right) => roleRank(left.role) - roleRank(right.role) || left.pid - right.pid)
      .map((entry) => {
        const history = entry.sampler.history;
        return {
          label: entry.label,
          pid: entry.pid,
          supported: entry.sampler.supported && history.length > 0,
          sampleCount: history.length,
          firstRssBytes: history.length > 0 ? history[0].rssBytes : null,
          lastRssBytes: history.length > 0 ? history[history.length - 1].rssBytes : null,
          maxRssBytes: entry.sampler.maxRssBytes,
          samples: downsampleSeries(history, MEMORY_SERIES_MAX_POINTS),
          role: entry.role,
          parentPid: entry.parentPid,
          image: entry.image,
        };
      });
  }

  /** The attribution record for the receipt (evidence, never a bare claim). */
  attribution(): CorpusProviderAttribution {
    const members = [...this.tracked.values()].map((entry) => ({
      pid: entry.pid,
      discoveredAtMs: entry.discoveredAtMs,
      maxRssBytes: entry.sampler.maxRssBytes,
    }));
    const unsampled = unsampledTreeMembers(
      members,
      Date.now(),
      Math.max(this.intervalMs * 3, 5000),
    );
    return {
      status: this.attributionStatus,
      providerPid: this.roots.providerPid,
      detail: this.attributionDetail,
      unattributedPids: [...new Set([...this.unattributed, ...unsampled])],
      sampledProcessCount: this.tracked.size,
    };
  }

  /** Highest RSS observed across every sampled process (bytes), or null. */
  maxObservedRssBytes(): number | null {
    let max: number | null = null;
    for (const entry of this.tracked.values()) {
      const value = entry.sampler.maxRssBytes;
      if (value !== null && (max === null || value > max)) max = value;
    }
    return max;
  }
}

function roleRank(role: CorpusProcessRole): number {
  return role === "server" ? 0 : role === "provider" ? 1 : 2;
}
