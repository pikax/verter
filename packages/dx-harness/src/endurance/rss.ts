/**
 * Cross-platform resident-set sampling for the spawned verter-lsp process.
 *
 *  - Linux:   `/proc/<pid>/status` VmRSS (kB).
 *  - macOS:   `ps -o rss= -p <pid>` (KiB).
 *  - Windows: `tasklist /FI "PID eq <pid>" /FO CSV /NH` — the "Mem Usage"
 *             (working set) column, e.g. `"123,456 K"`; non-digits stripped.
 *
 * Unsupported/failed reads resolve to null: the sampler reports
 * `supported: false` and the RSS assertion is SKIPPED with an explicit note —
 * never silently passed, never failed for a platform limitation.
 */
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * Read the resident set of `pid` in bytes, or null when the platform/read is
 * unsupported (process gone, tool missing, unparsable output).
 */
export async function readProcessRssBytes(pid: number): Promise<number | null> {
  try {
    if (process.platform === "linux") {
      const status = await readFile(`/proc/${pid}/status`, "utf8");
      const match = /^VmRSS:\s*(\d+)\s*kB/im.exec(status);
      return match ? Number(match[1]) * 1024 : null;
    }
    if (process.platform === "darwin") {
      const { stdout } = await execFileAsync("ps", ["-o", "rss=", "-p", String(pid)]);
      const kb = Number(stdout.trim());
      return Number.isFinite(kb) && kb > 0 ? kb * 1024 : null;
    }
    if (process.platform === "win32") {
      const { stdout } = await execFileAsync(
        "tasklist",
        ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"],
        { windowsHide: true },
      );
      // CSV: "image","pid","session","session#","mem usage" — mem like "123,456 K".
      const line = stdout.split(/\r?\n/).find((entry) => entry.includes(`"${pid}"`));
      if (!line) return null;
      const columns = line.split('","');
      const memColumn = columns[columns.length - 1] ?? "";
      const digits = memColumn.replace(/[^\d]/g, "");
      if (!digits) return null;
      return Number(digits) * 1024;
    }
    return null;
  } catch {
    return null;
  }
}

export interface RssSample {
  readonly atMs: number;
  readonly rssBytes: number;
}

/** Periodic RSS poller over one pid; keeps the max and a bounded history. */
export class RssSampler {
  private timer: ReturnType<typeof setInterval> | null = null;
  private samples: RssSample[] = [];
  private maxBytes = 0;
  private readsAttempted = 0;
  private readsFailed = 0;
  private startedAt = 0;
  /** True once at least one read succeeded; false ⇒ skip the RSS assertion. */
  private supported_: boolean | null = null;

  constructor(
    private readonly pid: number,
    private readonly intervalMs: number,
    private readonly maxSamples = 10_000,
  ) {}

  start(): void {
    if (this.timer) return;
    this.startedAt = Date.now();
    void this.tick();
    this.timer = setInterval(() => void this.tick(), this.intervalMs);
    this.timer.unref?.();
  }

  private async tick(): Promise<void> {
    const rss = await readProcessRssBytes(this.pid);
    this.readsAttempted += 1;
    if (rss === null) {
      this.readsFailed += 1;
      if (
        this.supported_ === null &&
        this.readsFailed >= 3 &&
        this.readsAttempted === this.readsFailed
      ) {
        this.supported_ = false;
      }
      return;
    }
    this.supported_ = true;
    if (rss > this.maxBytes) this.maxBytes = rss;
    if (this.samples.length < this.maxSamples) {
      this.samples.push({ atMs: Date.now() - this.startedAt, rssBytes: rss });
    }
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  get supported(): boolean {
    // Reads still in flight (fewer than 3 attempts): treat as supported so a
    // very short run does not silently skip the assertion on a good platform.
    return this.supported_ !== false;
  }

  get maxRssBytes(): number | null {
    return this.maxBytes > 0 ? this.maxBytes : null;
  }

  get history(): readonly RssSample[] {
    return this.samples;
  }
}
