/**
 * Buffered, line-addressable capture of a child process's stderr.
 *
 * A real language server logs to stderr (verter-lsp under `VERTER_LOG`/
 * `RUST_LOG`). The DX log collector needs that output, so this buffer retains
 * it and exposes it as text, lines, and a `waitForLine` predicate gate instead
 * of dropping it on the floor.
 */

export interface StderrBufferOptions {
  /**
   * Maximum number of UTF-8 bytes to retain. When the buffer grows past this,
   * the oldest characters are dropped (a coarse ring-buffer trim). Defaults to
   * `Infinity`, which retains the whole session — long-running harnesses
   * should `clear()` between measurement windows. Line delivery to
   * `onLine`/`waitForLine` is unaffected by trimming.
   */
  maxBytes?: number;
  /** Invoked for every appended chunk, already decoded as UTF-8. */
  onData?: (chunk: string) => void;
  /** Invoked for every completed line (newline-terminated, EOL stripped). */
  onLine?: (line: string) => void;
}

const EOL = /\r\n|\n|\r/;

interface LineWaiter {
  predicate: (line: string) => boolean;
  resolve: (line: string) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class StderrBuffer {
  private retained = "";
  private partialLine = "";
  private readonly maxBytes: number;
  private readonly onData?: (chunk: string) => void;
  private readonly onLine?: (line: string) => void;
  private readonly lineWaiters = new Set<LineWaiter>();

  constructor(options: StderrBufferOptions = {}) {
    this.maxBytes = options.maxBytes ?? Infinity;
    this.onData = options.onData;
    this.onLine = options.onLine;
  }

  /** Feed a chunk of child stderr (string or Buffer) into the buffer. */
  append(chunk: string | Buffer): void {
    const str = typeof chunk === "string" ? chunk : chunk.toString("utf-8");
    if (str.length === 0) return;
    this.retained += str;
    this.trim();
    this.onData?.(str);
    this.deliverLines(str);
  }

  private deliverLines(chunk: string): void {
    this.partialLine += chunk;
    const parts = this.partialLine.split(EOL);
    this.partialLine = parts.pop() ?? "";
    for (const line of parts) {
      this.onLine?.(line);
      this.notifyWaiters(line);
    }
  }

  private notifyWaiters(line: string): void {
    for (const waiter of [...this.lineWaiters]) {
      let matched = false;
      try {
        matched = waiter.predicate(line);
      } catch {
        matched = false;
      }
      if (matched) {
        clearTimeout(waiter.timer);
        this.lineWaiters.delete(waiter);
        waiter.resolve(line);
      }
    }
  }

  private trim(): void {
    if (this.maxBytes === Infinity) return;
    let bytes = Buffer.byteLength(this.retained, "utf-8");
    if (bytes <= this.maxBytes) return;
    let start = 0;
    while (start < this.retained.length && bytes > this.maxBytes) {
      const codePoint = this.retained.codePointAt(start)!;
      const u16 = codePoint > 0xffff ? 2 : 1;
      bytes -= Buffer.byteLength(this.retained.slice(start, start + u16), "utf-8");
      start += u16;
    }
    this.retained = this.retained.slice(start);
  }

  /** All retained stderr text. */
  text(): string {
    return this.retained;
  }

  /** Retained text length in UTF-16 code units. */
  get length(): number {
    return this.retained.length;
  }

  /** Retained text length in UTF-8 bytes. */
  get byteLength(): number {
    return Buffer.byteLength(this.retained, "utf-8");
  }

  /**
   * Retained text split into lines. A trailing newline does not yield a final
   * empty entry; an unterminated trailing line IS included.
   */
  lines(): string[] {
    const all = this.retained.split(EOL);
    if (all.length > 0 && all[all.length - 1] === "") all.pop();
    return all;
  }

  /** Drop all retained text and in-flight partial-line state. */
  clear(): void {
    this.retained = "";
    this.partialLine = "";
  }

  /**
   * Resolve with the first stderr line matching `predicate`. Already-buffered
   * lines are checked first; otherwise the promise resolves when a future line
   * matches, or rejects after `timeoutMs`.
   */
  waitForLine(predicate: (line: string) => boolean, timeoutMs = 5000): Promise<string> {
    for (const line of this.lines()) {
      if (predicate(line)) return Promise.resolve(line);
    }
    return new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.lineWaiters.delete(waiter);
        reject(new Error(`Timed out after ${timeoutMs}ms waiting for a matching stderr line`));
      }, timeoutMs);
      timer.unref();
      const waiter: LineWaiter = { predicate, resolve, reject, timer };
      this.lineWaiters.add(waiter);
    });
  }

  /**
   * Reject every in-flight {@link waitForLine} waiter with `err`. The owning
   * client calls this when its child exits or fails to spawn, so a pending
   * `waitForLine` fails fast instead of blocking until its own timeout for a
   * line that can no longer arrive.
   */
  rejectWaiters(err: Error): void {
    for (const waiter of [...this.lineWaiters]) {
      clearTimeout(waiter.timer);
      this.lineWaiters.delete(waiter);
      waiter.reject(new Error(`stderr line wait aborted: ${err.message}`));
    }
  }
}
