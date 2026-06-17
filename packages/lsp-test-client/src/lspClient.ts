/**
 * A side-effect-free JSON-RPC LSP client over child-process stdio.
 *
 * Importing this module does no I/O and mutates no globals. Constructing an
 * {@link LspClient} spawns a language-server child, frames JSON-RPC messages
 * with `Content-Length` headers (byte-accurate, so multi-byte payloads survive
 * intact), and exposes request/notification transport, notification waiting,
 * buffered stderr, position-encoding negotiation, and clean teardown.
 */
import { spawn, type ChildProcess } from "node:child_process";

import { StderrBuffer, type StderrBufferOptions } from "./stderrBuffer.js";
import {
  adoptServerEncoding,
  DEFAULT_POSITION_ENCODING,
  defaultClientPositionEncodings,
  DocumentPositions,
  withPositionEncodings,
  type InitializeParamsLike,
  type LspPosition,
  type PositionEncoding,
} from "./positionEncoding.js";

const DEFAULT_REQUEST_TIMEOUT = 30_000;
const HEADER_SEPARATOR = "\r\n\r\n";

export interface LspClientOptions {
  /** Extra environment variables merged over `process.env`. */
  env?: NodeJS.ProcessEnv;
  /** Default timeout (ms) for `sendRequest` and `waitForNotification`. */
  defaultTimeout?: number;
  /** Encodings advertised in `general.positionEncodings` (priority order). */
  positionEncodings?: PositionEncoding[];
  /** Options for the buffered stderr capture. */
  stderr?: StderrBufferOptions;
  /** Invoked if the child emits an `error` event (e.g. spawn failure). */
  onError?: (err: Error) => void;
  /**
   * Invoked for every inbound server→client notification, before any per-method
   * handler runs. A wildcard observer (unlike {@link LspClient.onNotification},
   * which is keyed by method) — the DX log/diagnostics collectors trace the full
   * notification stream through it.
   */
  onAnyNotification?: (method: string, params: any) => void;
}

interface PendingRequest {
  resolve: (value: any) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface NotificationWaiter {
  method: string;
  reject: (err: Error) => void;
  /** Clear the timeout and unregister the handler — idempotent. */
  cancel: () => void;
}

type NotificationHandler = (params: any) => void;
type RequestHandler = (params: any) => unknown;

export class LspClient {
  readonly name: string;
  readonly process: ChildProcess;
  readonly stderr: StderrBuffer;

  private stdoutBuf: Buffer = Buffer.alloc(0);
  private nextId = 1;
  private readonly pendingRequests = new Map<number, PendingRequest>();
  private readonly notificationWaiters = new Set<NotificationWaiter>();
  private readonly notificationHandlers = new Map<string, NotificationHandler[]>();
  private readonly requestHandlers = new Map<string, RequestHandler>();
  private readonly defaultTimeout: number;
  private readonly advertisedEncodings: PositionEncoding[];
  private readonly onAnyNotification?: (method: string, params: any) => void;

  private terminated = false;
  private exitCode_: number | null = null;
  private spawnError_: Error | null = null;
  private negotiatedEncoding: PositionEncoding = DEFAULT_POSITION_ENCODING;
  private serverCapabilities_: unknown = undefined;

  constructor(
    name: string,
    command: string,
    args: string[] = [],
    cwd?: string,
    options: LspClientOptions = {},
  ) {
    this.name = name;
    this.defaultTimeout = options.defaultTimeout ?? DEFAULT_REQUEST_TIMEOUT;
    this.advertisedEncodings = options.positionEncodings ?? defaultClientPositionEncodings();
    this.onAnyNotification = options.onAnyNotification;
    this.stderr = new StderrBuffer(options.stderr);

    this.process = spawn(command, args, {
      stdio: ["pipe", "pipe", "pipe"],
      cwd,
      env: { ...process.env, ...options.env },
      // A POSIX process group lets `kill()` reap the whole tree; Windows uses
      // taskkill /T instead, so detaching there is unnecessary.
      detached: process.platform !== "win32",
    });

    this.process.stdout!.on("data", (chunk: Buffer) => {
      this.stdoutBuf = this.stdoutBuf.length === 0 ? chunk : Buffer.concat([this.stdoutBuf, chunk]);
      this.drainMessages();
    });

    this.process.stderr!.on("data", (chunk: Buffer) => {
      this.stderr.append(chunk);
    });

    this.process.on("error", (err: Error) => {
      this.spawnError_ = err;
      this.terminated = true;
      this.rejectInFlight(err);
      options.onError?.(err);
    });

    this.process.on("exit", (code) => {
      this.exitCode_ = code;
      this.terminated = true;
      this.rejectInFlight(new Error(`${this.name} process exited with code ${code}`));
    });
  }

  /** The negotiated position encoding (UTF-16 until `initialize` resolves). */
  get positionEncoding(): PositionEncoding {
    return this.negotiatedEncoding;
  }

  /** The server capabilities returned from `initialize`, if any. */
  get serverCapabilities(): unknown {
    return this.serverCapabilities_;
  }

  /** The child's exit code, or null if it has not exited (or died via signal). */
  get exitCode(): number | null {
    return this.exitCode_;
  }

  /** The spawn error, if the child failed to start. */
  get spawnError(): Error | null {
    return this.spawnError_;
  }

  /** Whether the child is still running. */
  isAlive(): boolean {
    return !this.terminated && this.process.exitCode === null && this.spawnError_ === null;
  }

  /**
   * Reject everything still waiting on the child: pending requests, notification
   * waiters, and buffered-stderr line waiters. Invoked when the child exits or
   * fails to spawn so no waiter hangs until its own timeout for an answer, event,
   * or line that can no longer arrive.
   */
  private rejectInFlight(err: Error): void {
    for (const pending of this.pendingRequests.values()) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pendingRequests.clear();

    for (const waiter of [...this.notificationWaiters]) {
      waiter.cancel();
      waiter.reject(
        new Error(`${this.name} notification '${waiter.method}' aborted: ${err.message}`),
      );
    }
    this.notificationWaiters.clear();

    this.stderr.rejectWaiters(err);
  }

  private drainMessages(): void {
    for (;;) {
      const headerEnd = this.stdoutBuf.indexOf(HEADER_SEPARATOR);
      if (headerEnd === -1) break;

      const header = this.stdoutBuf.subarray(0, headerEnd).toString("utf-8");
      const match = header.match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        // Drop the malformed header and resync on the next separator.
        this.stdoutBuf = this.stdoutBuf.subarray(headerEnd + HEADER_SEPARATOR.length);
        continue;
      }

      const contentLength = Number.parseInt(match[1], 10);
      const bodyStart = headerEnd + HEADER_SEPARATOR.length;
      const bodyEnd = bodyStart + contentLength;
      if (this.stdoutBuf.length < bodyEnd) break; // incomplete body (byte-accurate)

      const body = this.stdoutBuf.subarray(bodyStart, bodyEnd).toString("utf-8");
      this.stdoutBuf = this.stdoutBuf.subarray(bodyEnd);

      let msg: any;
      try {
        msg = JSON.parse(body);
      } catch {
        continue; // skip undecodable body
      }
      this.handleMessage(msg);
    }
  }

  private writeMessage(payload: unknown): boolean {
    const stdin = this.process.stdin;
    if (!stdin || !stdin.writable) return false;
    const body = JSON.stringify(payload);
    const header = `Content-Length: ${Buffer.byteLength(body, "utf-8")}${HEADER_SEPARATOR}`;
    stdin.write(header + body);
    return true;
  }

  private handleMessage(msg: any): void {
    if ("id" in msg && !("method" in msg)) {
      // Response to one of our requests.
      const pending = this.pendingRequests.get(msg.id);
      if (!pending) return;
      this.pendingRequests.delete(msg.id);
      clearTimeout(pending.timer);
      if (msg.error) {
        pending.reject(new Error(`${this.name} LSP error: ${JSON.stringify(msg.error)}`));
      } else {
        pending.resolve(msg.result);
      }
    } else if ("method" in msg && !("id" in msg)) {
      // Server → client notification.
      this.onAnyNotification?.(msg.method, msg.params);
      const handlers = this.notificationHandlers.get(msg.method);
      if (handlers) {
        for (const handler of [...handlers]) handler(msg.params);
      }
    } else if ("method" in msg && "id" in msg) {
      // Server → client request.
      const handler = this.requestHandlers.get(msg.method);
      if (handler) {
        try {
          const result = handler(msg.params);
          this.writeMessage({ jsonrpc: "2.0", id: msg.id, result });
        } catch (err: any) {
          this.writeMessage({
            jsonrpc: "2.0",
            id: msg.id,
            error: { code: -32603, message: err?.message ?? String(err) },
          });
        }
      } else if (
        msg.method === "window/workDoneProgress/create" ||
        msg.method === "client/registerCapability"
      ) {
        // Acknowledge the standard server-initiated requests benchmarks ignore.
        this.writeMessage({ jsonrpc: "2.0", id: msg.id, result: null });
      } else {
        // Any other server→client request still carries an id and therefore
        // expects a reply. Answer method-not-found rather than dropping it, so a
        // real language server awaiting our response is never left deadlocked.
        this.writeMessage({
          jsonrpc: "2.0",
          id: msg.id,
          error: { code: -32601, message: `Method not found: ${msg.method}` },
        });
      }
    }
  }

  sendRequest<T = any>(method: string, params?: any, timeout = this.defaultTimeout): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (this.terminated) {
        reject(new Error(`${this.name} cannot send '${method}': process is not running`));
        return;
      }
      const id = this.nextId++;
      const timer = setTimeout(() => {
        if (this.pendingRequests.delete(id)) {
          reject(new Error(`${this.name} request '${method}' timed out after ${timeout}ms`));
        }
      }, timeout);
      timer.unref();
      this.pendingRequests.set(id, { resolve, reject, timer });
      if (!this.writeMessage({ jsonrpc: "2.0", id, method, params })) {
        clearTimeout(timer);
        this.pendingRequests.delete(id);
        reject(new Error(`${this.name} cannot send '${method}': stdin is not writable`));
      }
    });
  }

  sendNotification(method: string, params?: any): void {
    this.writeMessage({ jsonrpc: "2.0", method, params });
  }

  onNotification(method: string, handler: NotificationHandler): void {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);
  }

  offNotification(method: string, handler: NotificationHandler): void {
    const handlers = this.notificationHandlers.get(method);
    if (!handlers) return;
    const idx = handlers.indexOf(handler);
    if (idx >= 0) handlers.splice(idx, 1);
  }

  /** Register a handler for a server-to-client request. */
  onRequest(method: string, handler: RequestHandler): void {
    this.requestHandlers.set(method, handler);
  }

  /**
   * Resolve with the params of the next notification on `method` that
   * satisfies `predicate` (or any, if no predicate). Rejects after `timeout`.
   */
  waitForNotification(
    method: string,
    timeout = this.defaultTimeout,
    predicate?: (params: any) => boolean,
  ): Promise<any> {
    return new Promise((resolve, reject) => {
      const cancel = () => {
        clearTimeout(timer);
        this.offNotification(method, handler);
        this.notificationWaiters.delete(waiter);
      };
      const timer = setTimeout(() => {
        cancel();
        reject(new Error(`${this.name} notification '${method}' timed out after ${timeout}ms`));
      }, timeout);
      timer.unref();
      const handler: NotificationHandler = (params) => {
        if (predicate && !predicate(params)) return;
        cancel();
        resolve(params);
      };
      const waiter: NotificationWaiter = { method, reject, cancel };
      this.notificationWaiters.add(waiter);
      this.onNotification(method, handler);
    });
  }

  /**
   * Send `initialize`, advertising `general.positionEncodings`, and adopt the
   * server's chosen `positionEncoding` from the result. Other capabilities the
   * caller set are preserved.
   */
  async initialize<T = any>(
    params: InitializeParamsLike,
    timeout = this.defaultTimeout,
  ): Promise<T> {
    const withEncodings = withPositionEncodings(params, this.advertisedEncodings);
    const result = await this.sendRequest<any>("initialize", withEncodings, timeout);
    this.serverCapabilities_ = result?.capabilities;
    this.negotiatedEncoding = adoptServerEncoding(result?.capabilities?.positionEncoding);
    return result as T;
  }

  /** Build an encoding-aware position converter for `text`. */
  documentPositions(text: string): DocumentPositions {
    return new DocumentPositions(text);
  }

  /** UTF-8 byte offset → LSP position using the negotiated encoding. */
  byteOffsetToPosition(text: string, byteOffset: number): LspPosition {
    return new DocumentPositions(text).byteToPosition(byteOffset, this.negotiatedEncoding);
  }

  /** LSP position (in the negotiated encoding) → UTF-8 byte offset. */
  positionToByteOffset(text: string, position: LspPosition): number {
    return new DocumentPositions(text).positionToByte(position, this.negotiatedEncoding);
  }

  /**
   * Terminate the child process. Sends SIGTERM, then force-kills the whole
   * tree (taskkill /T /F on Windows, SIGKILL to the process group on POSIX) if
   * it has not exited within a short grace period. Always resolves — including
   * when the child failed to spawn (an `error` with no `exit` and no pid), so a
   * caller awaiting teardown can never hang.
   */
  kill(): Promise<void> {
    return new Promise<void>((resolve) => {
      // `terminated` is the reliable "already done" signal: a signal-killed
      // child reports `exitCode === null` on Windows, so the raw exit code
      // cannot gate this — guarding on it would re-await an `exit` that has
      // already fired and hang forever.
      if (this.terminated) {
        resolve();
        return;
      }

      // Resolve from whichever signal arrives first — `exit`, `error`, the
      // force path, or the grace timer — exactly once.
      let settled = false;
      const finish = () => {
        if (settled) return;
        settled = true;
        clearTimeout(forceTimer);
        resolve();
      };

      const pid = this.process.pid;
      const forceTimer = setTimeout(() => {
        if (pid === undefined) {
          // The child never acquired a pid (a failed spawn): there is nothing to
          // force-kill and no `exit` will ever come, so resolve as a last resort
          // rather than hang forever.
          finish();
          return;
        }
        if (process.platform === "win32") {
          const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
            stdio: "ignore",
            windowsHide: true,
          });
          // taskkill reaping the tree fires the child's `exit` (→ finish);
          // resolve on the killer's own settle too so a taskkill failure cannot
          // leave kill() hanging.
          killer.once("close", finish);
          killer.once("error", finish);
        } else {
          try {
            process.kill(-pid, "SIGKILL"); // negative pid → process group
          } catch {
            try {
              process.kill(pid, "SIGKILL");
            } catch {
              // Already gone: `exit` has fired or will, but finish here too so a
              // vanished child that emits no `exit` cannot hang kill().
              finish();
            }
          }
        }
      }, 2000);
      forceTimer.unref();

      // Normal teardown and post-force-kill both surface as `exit`; a failed
      // spawn surfaces only as `error` (no `exit`). Resolve on either so kill()
      // never waits for an event that cannot arrive.
      this.process.once("exit", finish);
      this.process.once("error", finish);

      try {
        this.process.kill("SIGTERM");
      } catch {
        /* already gone — a listener resolves */
      }
    });
  }
}
