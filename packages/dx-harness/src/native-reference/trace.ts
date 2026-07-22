/**
 * Full-request-trace writer for the native reference lane.
 *
 * One JSONL file per engine session. Every client→server request/notification
 * and every server-initiated message appends one line with a wall-clock stamp
 * and approximate payload size, so a later diff against Verter's provider
 * traffic can compare COUNTS, KINDS, and BYTES per user-visible operation.
 *
 * Privacy: file identity is recorded as the SAMPLE INDEX by default. Relative
 * corpus paths appear only when the lane runs with
 * `VERTER_NATIVE_REF_FILE_DETAIL=1`. Absolute paths are never written.
 */
import { appendFileSync, mkdirSync } from "node:fs";
import path from "node:path";

export interface TraceLine {
  /** Epoch milliseconds. */
  readonly t: number;
  readonly ev: string;
  readonly [key: string]: unknown;
}

export class NativeTraceWriter {
  private readonly filePath: string | null;
  private readonly tallies = new Map<string, number>();

  constructor(traceDir: string | null, engine: string) {
    if (traceDir === null) {
      this.filePath = null;
      return;
    }
    mkdirSync(traceDir, { recursive: true });
    this.filePath = path.join(traceDir, `native-trace-${engine}.jsonl`);
  }

  get file(): string | null {
    return this.filePath;
  }

  line(entry: TraceLine): void {
    if (this.filePath === null) return;
    try {
      appendFileSync(this.filePath, `${JSON.stringify(entry)}\n`);
    } catch {
      // Tracing must never fail the measurement.
    }
  }

  /** Count a server-initiated message by its method/event NAME only. */
  tally(name: string): void {
    this.tallies.set(name, (this.tallies.get(name) ?? 0) + 1);
  }

  talliesSnapshot(): Record<string, number> {
    return Object.fromEntries([...this.tallies.entries()].sort());
  }
}
