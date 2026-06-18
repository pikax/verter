/**
 * Client for the `verter_dx_baseline` newline-delimited JSON bridge.
 *
 * This is a small stdio newline-JSON client for C's bridge protocol
 * (`crates/verter_dx_baseline/src/protocol.rs`) — NOT the LSP client, and NOT a
 * second provider stack. C owns provider discovery/spawn, the versioned artifact
 * overlay, and normalized provider output; B only frames requests and decodes
 * responses. The wire is a strict request→one-response sequence over stdio with
 * NO response ids, so responses are correlated to requests in FIFO order. Because
 * the protocol carries no ids, a per-request timeout FAILS the whole session
 * (rejecting every waiter and hard-killing the child): a late reply to a
 * timed-out request could otherwise be misattributed to the next request, so the
 * session cannot safely continue past a missed reply.
 *
 * Refusals (stale artifact, map-absent, tool-root mismatch, …) are EXPECTED
 * outcomes of the differential, so they surface as typed `error` response frames
 * rather than thrown exceptions; only transport faults (spawn error, premature
 * exit) reject.
 */

import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";

import { StderrBuffer } from "@verter/lsp-test-client";

// ── wire types (mirror protocol.rs under serde rename_all = "camelCase") ──────

export type ProviderName = "tsgo" | "tsserver";
export type FileRole = "entry" | "api" | "support";
export type QueryMethod = "completion" | "hover" | "definition" | "typeDefinition" | "references";

/**
 * Provider-pure completion resolve handle — mirrors
 * `verter_type_runtime::protocol::CompletionResolveData` (`#[serde(tag = "kind",
 * rename_all = "snake_case")]`). Carried on a completion item as `resolveData`
 * and sent back via `resolveCompletion` to recover the auto-import edits. The
 * exact wire shape is pinned Rust-side and asserted against this mirror by the
 * dx-harness serde fixture test.
 */
export type CompletionResolveData =
  | { kind: "lsp"; label: string; data: unknown }
  | { kind: "tsserver_entry"; name: string; source?: string; data?: unknown; offset: number };

/** The pinned TypeScript tool root passed in `hello` (`ToolRoot`). */
export interface ToolRootWire {
  tsserverTsdk?: string;
  expectedTsserverJs?: string;
  tsserverVersion?: string;
  tsgoBin?: string;
}

/** A materialized file pushed to the provider (`BaselineFile`). */
export interface BaselineFile {
  path: string;
  content: string;
  role: FileRole;
  sourceMapIdentity?: string;
}

/** A `.vue.ts` twin refreshed by an edit, carrying its OWN authored version. */
export interface ChangedTwin {
  path: string;
  version: number;
}

export interface HelloRequest {
  type: "hello";
  workspaceRoot: string;
  repoRoot: string;
  provider: ProviderName;
  strictCi: boolean;
  toolRoot: ToolRootWire;
}
export interface OpenRequest {
  type: "open";
  files: BaselineFile[];
  version: number;
}
export interface SyncArtifactsRequest {
  type: "syncArtifacts";
  uri: string;
  version: number;
  files: BaselineFile[];
  sourceMapIdentity?: string;
  changedPublicApiTwins?: ChangedTwin[];
}
export interface QueryRequest {
  type: "query";
  method: QueryMethod;
  uri: string;
  path: string;
  offset: number;
  version: number;
  triggerCharacter?: string;
  requiresSourceMap?: boolean;
}
export interface ResolveCompletionRequest {
  type: "resolveCompletion";
  uri: string;
  path: string;
  version: number;
  data: CompletionResolveData;
}
export interface DiagnosticsRequest {
  type: "diagnostics";
  uri: string;
  path: string;
  version: number;
  requiresSourceMap?: boolean;
}
export interface ShutdownRequest {
  type: "shutdown";
}

export type BridgeRequest =
  | HelloRequest
  | OpenRequest
  | SyncArtifactsRequest
  | QueryRequest
  | ResolveCompletionRequest
  | DiagnosticsRequest
  | ShutdownRequest;

export interface ProviderCapabilities {
  provider: ProviderName;
  positionEncoding: string;
  diagnosticsPush: boolean;
  completionResolve: boolean;
}
export interface HelloResponse {
  type: "hello";
  ok: boolean;
  provider: ProviderName;
  skipped: boolean;
  skipReason?: string;
  baselineToolRootUsed?: string | null;
  capabilities?: ProviderCapabilities;
}
export interface OpenResponse {
  type: "open";
  ok: boolean;
  opened: string[];
  version: number;
}
export type SyncAction = "opened" | "loaded" | "updated";
export interface AppliedSync {
  path: string;
  action: SyncAction;
}
export interface SyncArtifactsResponse {
  type: "syncArtifacts";
  ok: boolean;
  uri: string;
  version: number;
  applied: AppliedSync[];
}
export interface NormalizedHover {
  contents: string;
  rangeStart?: number;
  rangeEnd?: number;
}
export interface NormalizedLocation {
  path: string;
  start: number;
  end: number;
}
export interface NormalizedCompletionItem {
  label: string;
  kind?: string;
  detail?: string;
  insertText?: string;
  sortText?: string;
  /** The provider-pure resolve handle, present on items that carry one. */
  resolveData?: CompletionResolveData;
}
export interface NormalizedResolvedTextEdit {
  start: number;
  end: number;
  newText: string;
}
export type QueryResult =
  | { kind: "hover"; hover: NormalizedHover | null }
  | { kind: "completion"; items: NormalizedCompletionItem[]; isIncomplete: boolean }
  | { kind: "definition"; locations: NormalizedLocation[] };
export interface QueryResponse {
  type: "query";
  method: QueryMethod;
  uri: string;
  version: number;
  result: QueryResult;
  capabilities: ProviderCapabilities;
}
export interface ResolveCompletionResponse {
  type: "resolveCompletion";
  uri: string;
  version: number;
  additionalTextEdits: NormalizedResolvedTextEdit[];
  detail?: string;
  documentation?: string;
  capabilities: ProviderCapabilities;
}
export interface NormalizedDiagnostic {
  message: string;
  severity: string;
  start: number;
  end: number;
  code?: string;
}
export interface DiagnosticsResponse {
  type: "diagnostics";
  uri: string;
  version: number;
  diagnostics: NormalizedDiagnostic[];
  capabilities: ProviderCapabilities;
}
export interface ShutdownResponse {
  type: "shutdown";
  ok: boolean;
  baselineRan: number;
}
export type ErrorKind =
  | "baseline_artifact_stale"
  | "baseline_tool_root_mismatch"
  | "baseline_tool_root_missing"
  | "compiled_code_map_absent"
  | "provider_error"
  | "not_initialized"
  | "invalid_request";
export interface ErrorResponse {
  type: "error";
  kind: ErrorKind;
  message: string;
  uri?: string;
  requestedVersion?: number;
  haveVersion?: number;
}
export type BridgeResponse =
  | HelloResponse
  | OpenResponse
  | SyncArtifactsResponse
  | QueryResponse
  | ResolveCompletionResponse
  | DiagnosticsResponse
  | ShutdownResponse
  | ErrorResponse;

// ── frame builders (omit defaults to mirror C's skip_serializing_if/default) ──

/** Build a `hello` request. */
export function helloFrame(input: Omit<HelloRequest, "type">): HelloRequest {
  return { type: "hello", ...input };
}

/** Build an `open` request. */
export function openFrame(files: BaselineFile[], version: number): OpenRequest {
  return { type: "open", files, version };
}

/** Inputs to {@link syncArtifactsFrame}. */
export interface SyncArtifactsInput {
  uri: string;
  version: number;
  files: BaselineFile[];
  sourceMapIdentity?: string;
  changedPublicApiTwins?: ChangedTwin[];
}

/** Build a `syncArtifacts` request, omitting an absent map identity / empty twins. */
export function syncArtifactsFrame(input: SyncArtifactsInput): SyncArtifactsRequest {
  const frame: SyncArtifactsRequest = {
    type: "syncArtifacts",
    uri: input.uri,
    version: input.version,
    files: input.files,
  };
  if (input.sourceMapIdentity !== undefined) frame.sourceMapIdentity = input.sourceMapIdentity;
  if (input.changedPublicApiTwins && input.changedPublicApiTwins.length > 0) {
    frame.changedPublicApiTwins = input.changedPublicApiTwins;
  }
  return frame;
}

/** Inputs to {@link queryFrame}. */
export interface QueryInput {
  method: QueryMethod;
  uri: string;
  path: string;
  offset: number;
  version: number;
  triggerCharacter?: string;
  requiresSourceMap?: boolean;
}

/** Build a `query` request, omitting an absent trigger char / false map flag. */
export function queryFrame(input: QueryInput): QueryRequest {
  const frame: QueryRequest = {
    type: "query",
    method: input.method,
    uri: input.uri,
    path: input.path,
    offset: input.offset,
    version: input.version,
  };
  if (input.triggerCharacter !== undefined) frame.triggerCharacter = input.triggerCharacter;
  if (input.requiresSourceMap) frame.requiresSourceMap = true;
  return frame;
}

/** Inputs to {@link resolveCompletionFrame}. */
export interface ResolveCompletionInput {
  uri: string;
  path: string;
  version: number;
  data: CompletionResolveData;
}

/** Build a `resolveCompletion` request. */
export function resolveCompletionFrame(input: ResolveCompletionInput): ResolveCompletionRequest {
  return { type: "resolveCompletion", ...input };
}

/** Inputs to {@link diagnosticsFrame}. */
export interface DiagnosticsInput {
  uri: string;
  path: string;
  version: number;
  requiresSourceMap?: boolean;
}

/** Build a `diagnostics` request, omitting a false map flag. */
export function diagnosticsFrame(input: DiagnosticsInput): DiagnosticsRequest {
  const frame: DiagnosticsRequest = {
    type: "diagnostics",
    uri: input.uri,
    path: input.path,
    version: input.version,
  };
  if (input.requiresSourceMap) frame.requiresSourceMap = true;
  return frame;
}

/** Build a `shutdown` request. */
export function shutdownFrame(): ShutdownRequest {
  return { type: "shutdown" };
}

/** Serialise a request to one newline-terminated wire line. */
export function encodeRequest(req: BridgeRequest): string {
  return JSON.stringify(req) + "\n";
}

/** Decode one wire line into a typed response, rejecting an untagged frame. */
export function decodeResponse(line: string): BridgeResponse {
  const raw: unknown = JSON.parse(line);
  if (
    raw === null ||
    typeof raw !== "object" ||
    typeof (raw as { type?: unknown }).type !== "string"
  ) {
    throw new Error(`bridge response is not a tagged frame: ${line}`);
  }
  return raw as BridgeResponse;
}

/** Splits a byte stream into complete newline-delimited frames. */
export class NewlineFramer {
  private partial = "";

  /** Feed a chunk; return every complete, non-blank line it produced. */
  push(chunk: string): string[] {
    this.partial += chunk;
    const parts = this.partial.split("\n");
    this.partial = parts.pop() ?? "";
    const lines: string[] = [];
    for (const part of parts) {
      const line = part.endsWith("\r") ? part.slice(0, -1) : part;
      if (line.trim().length > 0) lines.push(line);
    }
    return lines;
  }
}

// ── client ────────────────────────────────────────────────────────────────

/** Options for {@link BridgeClient}. */
export interface BridgeClientOptions {
  /** Args placed before the (no-op) bridge subcommand — e.g. a fake-binary path. */
  extraArgs?: string[];
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  /** Per-request response timeout (ms). */
  requestTimeoutMs?: number;
}

interface Pending {
  resolve: (r: BridgeResponse) => void;
  reject: (e: Error) => void;
  timer?: ReturnType<typeof setTimeout>;
}

/**
 * A live bridge session. Spawns the bridge in its default (no-arg) stdio loop
 * mode and drives the request/response protocol; responses correlate to requests
 * in FIFO order, matching the bridge's strictly-sequential dispatch loop. A
 * per-request timeout poisons the session (see {@link failTimeout}) because the
 * protocol has no response ids to re-correlate a late reply.
 */
export class BridgeClient {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly framer = new NewlineFramer();
  private readonly pending: Pending[] = [];
  private readonly requestTimeoutMs: number;
  /** Buffered child stderr (the bridge logs there; stdout is the protocol). */
  readonly stderr = new StderrBuffer();
  private exited = false;
  private exitError?: Error;

  constructor(bin: string, opts: BridgeClientOptions = {}) {
    this.requestTimeoutMs = opts.requestTimeoutMs ?? 30_000;
    this.child = spawn(bin, opts.extraArgs ?? [], {
      cwd: opts.cwd,
      env: opts.env,
      stdio: ["pipe", "pipe", "pipe"],
    }) as ChildProcessWithoutNullStreams;

    this.child.stdout.setEncoding("utf-8");
    this.child.stdout.on("data", (chunk: string) => {
      for (const line of this.framer.push(chunk)) this.onLine(line);
    });
    this.child.stderr.on("data", (chunk: Buffer) => this.stderr.append(chunk));
    this.child.on("error", (err) => this.fail(err));
    this.child.on("close", (code, signal) => {
      this.exited = true;
      const reason =
        this.exitError ??
        new Error(`bridge exited before responding (code ${code}, signal ${signal})`);
      // Any request still awaiting a response can never be answered now.
      while (this.pending.length > 0) this.rejectNext(reason);
    });
  }

  private onLine(line: string): void {
    let response: BridgeResponse;
    try {
      response = decodeResponse(line);
    } catch (err) {
      this.fail(err instanceof Error ? err : new Error(String(err)));
      return;
    }
    const waiter = this.pending.shift();
    if (!waiter) return; // an unsolicited frame; nothing is awaiting it
    if (waiter.timer) clearTimeout(waiter.timer);
    waiter.resolve(response);
  }

  private fail(err: Error): void {
    this.exitError = err;
    while (this.pending.length > 0) this.rejectNext(err);
  }

  /**
   * Time out a request by FAILING the whole session. C's bridge dispatches one
   * response per request in strict order with no response ids, so a late reply to
   * a timed-out request would otherwise be misattributed (via the FIFO
   * {@link onLine} `shift`) to whatever request is next in line. Rejecting every
   * waiter and hard-killing the child makes that impossible — once a request
   * times out the session is unusable and every later `send` rejects.
   */
  private failTimeout(req: BridgeRequest): void {
    if (this.exited || this.exitError) return;
    this.fail(
      new Error(
        `bridge request "${req.type}" timed out after ${this.requestTimeoutMs}ms; ` +
          `the session is now failed (the bridge has no response ids, so it cannot ` +
          `safely continue after a missed reply)`,
      ),
    );
    try {
      this.child.kill("SIGKILL");
    } catch {
      // The child may already be gone.
    }
  }

  private rejectNext(err: Error): void {
    const waiter = this.pending.shift();
    if (!waiter) return;
    if (waiter.timer) clearTimeout(waiter.timer);
    waiter.reject(err);
  }

  /** Send one request and await its correlated response (or an `error` frame). */
  send(req: BridgeRequest): Promise<BridgeResponse> {
    // A failed (timed-out / faulted) session is unusable: never enqueue a waiter
    // a future reply could be misattributed to.
    if (this.exited || this.exitError) {
      return Promise.reject(this.exitError ?? new Error("bridge has exited"));
    }
    return new Promise<BridgeResponse>((resolve, reject) => {
      const timer = setTimeout(() => this.failTimeout(req), this.requestTimeoutMs);
      timer.unref();
      this.pending.push({ resolve, reject, timer });
      this.child.stdin.write(encodeRequest(req), (err) => {
        if (err) this.fail(err);
      });
    });
  }

  hello(input: Omit<HelloRequest, "type">): Promise<HelloResponse | ErrorResponse> {
    return this.send(helloFrame(input)) as Promise<HelloResponse | ErrorResponse>;
  }
  open(files: BaselineFile[], version: number): Promise<OpenResponse | ErrorResponse> {
    return this.send(openFrame(files, version)) as Promise<OpenResponse | ErrorResponse>;
  }
  syncArtifacts(input: SyncArtifactsInput): Promise<SyncArtifactsResponse | ErrorResponse> {
    return this.send(syncArtifactsFrame(input)) as Promise<SyncArtifactsResponse | ErrorResponse>;
  }
  query(input: QueryInput): Promise<QueryResponse | ErrorResponse> {
    return this.send(queryFrame(input)) as Promise<QueryResponse | ErrorResponse>;
  }
  resolveCompletion(
    input: ResolveCompletionInput,
  ): Promise<ResolveCompletionResponse | ErrorResponse> {
    return this.send(resolveCompletionFrame(input)) as Promise<
      ResolveCompletionResponse | ErrorResponse
    >;
  }
  diagnostics(input: DiagnosticsInput): Promise<DiagnosticsResponse | ErrorResponse> {
    return this.send(diagnosticsFrame(input)) as Promise<DiagnosticsResponse | ErrorResponse>;
  }

  /** Request a clean shutdown and await the final probe-count report. */
  async shutdown(): Promise<ShutdownResponse | ErrorResponse> {
    if (this.exited || this.exitError) {
      throw this.exitError ?? new Error("bridge has already exited");
    }
    return (await this.send(shutdownFrame())) as ShutdownResponse | ErrorResponse;
  }

  /** Tear the session down: close stdin, then ensure the child is gone. */
  async dispose(): Promise<void> {
    if (this.exited) return;
    try {
      this.child.stdin.end();
    } catch {
      // stdin may already be closed.
    }
    await new Promise<void>((resolve) => {
      const done = (): void => {
        clearTimeout(killTimer);
        resolve();
      };
      const killTimer = setTimeout(() => {
        this.child.kill("SIGKILL");
      }, 2000);
      killTimer.unref();
      if (this.exited) {
        done();
        return;
      }
      this.child.once("close", done);
      this.child.kill();
    });
  }
}
