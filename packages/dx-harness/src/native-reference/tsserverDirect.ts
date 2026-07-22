/**
 * Direct tsserver session: the native reference for the `tsserver` engine.
 *
 * Spawns the EXACT process shape Verter's `TsserverTypeProvider::spawn` uses —
 * `node <tsserver.js> --useSyntaxServer=false
 * --disableAutomaticTypingAcquisition` — but WITHOUT `@verter/typescript-plugin`
 * (the reference is a plain-TypeScript editor, no Verter anywhere). The
 * tsserver.js resolution mirrors `verter_lsp::tsserver::find_tsserver`:
 * workspace `node_modules/typescript/lib/tsserver.js` walking up to 10 parent
 * directories first, then the tsdk (this repo's
 * `packages/typescript-plugin/node_modules/typescript/lib`, the same tsdk the
 * corpus gate passes), with the TS7+ native-family gate applied the same way.
 *
 * Protocol: requests are newline-delimited JSON
 * (`{"seq":N,"type":"request","command":…,"arguments":…}`); responses and
 * events arrive Content-Length framed. Commands match Verter's vocabulary:
 * `open` (fire-and-forget), `quickinfo`, `definition`, `references`,
 * `completionInfo` — with 1-based line/offset positions.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { summarizeKinds } from "../corpus-gate/metrics.js";
import type { CorpusProbe } from "../corpus-gate/probes.js";
import { REPO_ROOT } from "../corpus-gate/spawn.js";
import type { CorpusRequestObservation } from "../corpus-gate/types.js";
import { mineNativeTsProbes } from "./probes.js";
import { NativeTraceWriter } from "./trace.js";
import type { NativeAccounting, NativeEngineReport, NativeReferenceConfig } from "./types.js";

const HEADER_SEPARATOR = "\r\n\r\n";

interface ResolvedTsserver {
  readonly tsserverJs: string;
  readonly provenance: string;
  readonly tsVersion: string | null;
}

function tsVersionNear(tsserverJs: string): string | null {
  try {
    const packageJson = path.join(path.dirname(path.dirname(tsserverJs)), "package.json");
    const parsed = JSON.parse(readFileSync(packageJson, "utf8")) as { version?: string };
    return typeof parsed.version === "string" ? parsed.version : null;
  } catch {
    return null;
  }
}

function isNativeFamily(version: string | null): boolean {
  if (version === null) return false;
  const major = Number.parseInt(version.split(".")[0] ?? "", 10);
  return Number.isFinite(major) && major >= 7;
}

/**
 * Mirror of `find_tsserver` (workspace walk-up first, then tsdk) plus the
 * TS7+ native-family rejection `try_spawn_tsserver` applies.
 */
function resolveTsserver(corpusDir: string, explicitTsdk: string | null): ResolvedTsserver {
  let dir = corpusDir;
  for (let i = 0; i < 10; i += 1) {
    const candidate = path.join(dir, "node_modules", "typescript", "lib", "tsserver.js");
    if (existsSync(candidate)) {
      const version = tsVersionNear(candidate);
      if (!isNativeFamily(version)) {
        return { tsserverJs: candidate, provenance: "workspace-walkup", tsVersion: version };
      }
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  const tsdk =
    explicitTsdk ??
    path.join(REPO_ROOT, "packages", "typescript-plugin", "node_modules", "typescript", "lib");
  const candidate = path.join(tsdk, "tsserver.js");
  if (!existsSync(candidate)) {
    throw new Error(`tsserver.js not found in workspace walk-up nor tsdk: ${candidate}`);
  }
  const version = tsVersionNear(candidate);
  if (isNativeFamily(version)) {
    throw new Error(`resolved tsserver is TS7+ native family (${version}) — not a tsserver`);
  }
  return {
    tsserverJs: candidate,
    provenance: explicitTsdk ? "explicit-tsdk" : "repo-plugin-tsdk",
    tsVersion: version,
  };
}

/** Strip anything path-shaped from a tsserver message before tallying it. */
function scrubMessage(message: string): string {
  return message
    .replace(/([A-Za-z]:)?[\\/][^\s'"]+/g, "<path>")
    .replace(/'[^']*'/g, "'<x>'")
    .slice(0, 80);
}

/** Minimal tsserver protocol client (newline requests, framed responses). */
class TsserverDirectClient {
  readonly process: ChildProcess;
  private stdoutBuf: Buffer = Buffer.alloc(0);
  private nextSeq = 1;
  private readonly pending = new Map<
    number,
    { resolve: (value: TsserverResponse) => void; timer: ReturnType<typeof setTimeout> }
  >();
  private terminated = false;
  bytesSentApprox = 0;
  bytesReceivedApprox = 0;
  requestCount = 0;
  notificationCount = 0;

  constructor(
    nodeBin: string,
    tsserverJs: string,
    cwd: string,
    private readonly trace: NativeTraceWriter,
  ) {
    this.process = spawn(
      nodeBin,
      [tsserverJs, "--useSyntaxServer=false", "--disableAutomaticTypingAcquisition"],
      { stdio: ["pipe", "pipe", "pipe"], cwd, windowsHide: true },
    );
    this.process.stdout!.on("data", (chunk: Buffer) => {
      this.stdoutBuf = this.stdoutBuf.length === 0 ? chunk : Buffer.concat([this.stdoutBuf, chunk]);
      this.drain();
    });
    this.process.on("exit", () => {
      this.terminated = true;
      for (const [, entry] of this.pending) {
        clearTimeout(entry.timer);
        entry.resolve({ kind: "error", message: "tsserver exited" });
      }
      this.pending.clear();
    });
    this.process.on("error", () => {
      this.terminated = true;
    });
  }

  isAlive(): boolean {
    return !this.terminated && this.process.exitCode === null;
  }

  private drain(): void {
    for (;;) {
      const headerEnd = this.stdoutBuf.indexOf(HEADER_SEPARATOR);
      if (headerEnd === -1) break;
      const header = this.stdoutBuf.subarray(0, headerEnd).toString("utf8");
      const match = header.match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        this.stdoutBuf = this.stdoutBuf.subarray(headerEnd + HEADER_SEPARATOR.length);
        continue;
      }
      const contentLength = Number.parseInt(match[1], 10);
      const bodyStart = headerEnd + HEADER_SEPARATOR.length;
      if (this.stdoutBuf.length < bodyStart + contentLength) break;
      const body = this.stdoutBuf.subarray(bodyStart, bodyStart + contentLength).toString("utf8");
      this.stdoutBuf = this.stdoutBuf.subarray(bodyStart + contentLength);
      this.bytesReceivedApprox += body.length;
      let msg: {
        type?: string;
        event?: string;
        request_seq?: number;
        success?: boolean;
        message?: string;
        body?: unknown;
      };
      try {
        msg = JSON.parse(body);
      } catch {
        continue;
      }
      if (msg.type === "event" && typeof msg.event === "string") {
        this.trace.tally(`event:${msg.event}`);
        this.trace.line({
          t: Date.now(),
          ev: "server-event",
          event: msg.event,
          bytes: body.length,
        });
        continue;
      }
      if (msg.type === "response" && typeof msg.request_seq === "number") {
        const entry = this.pending.get(msg.request_seq);
        if (!entry) continue;
        this.pending.delete(msg.request_seq);
        clearTimeout(entry.timer);
        if (msg.success === false) {
          entry.resolve({ kind: "failure", message: String(msg.message ?? "unknown") });
        } else {
          entry.resolve({ kind: "ok", body: msg.body, bytes: body.length });
        }
      }
    }
  }

  private write(payload: unknown): void {
    const line = `${JSON.stringify(payload)}\n`;
    this.bytesSentApprox += line.length;
    this.process.stdin?.write(line);
  }

  /** Fire-and-forget command (tsserver `open` has no response). */
  command(command: string, args: unknown): void {
    this.notificationCount += 1;
    this.write({ seq: this.nextSeq++, type: "request", command, arguments: args });
  }

  request(command: string, args: unknown, timeoutMs: number): Promise<TsserverResponse> {
    if (this.terminated) {
      return Promise.resolve({ kind: "error", message: "tsserver not running" });
    }
    const seq = this.nextSeq++;
    this.requestCount += 1;
    return new Promise<TsserverResponse>((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(seq);
        resolve({ kind: "timeout" });
      }, timeoutMs);
      timer.unref?.();
      this.pending.set(seq, { resolve, timer });
      this.write({ seq, type: "request", command, arguments: args });
    });
  }

  kill(): void {
    try {
      this.process.kill();
    } catch {
      /* already gone */
    }
  }
}

type TsserverResponse =
  | { kind: "ok"; body: unknown; bytes: number }
  | { kind: "failure"; message: string }
  | { kind: "timeout" }
  | { kind: "error"; message: string };

/**
 * tsserver reports "no content" for several commands as `success:false` with a
 * stock message; that is the EMPTY class in LSP terms, not an error.
 */
function failureIsEmpty(message: string): boolean {
  return /no content available|no definition|could not find (?:references|definition)|no quick info/i.test(
    message,
  );
}

function scriptKindFor(relativePath: string): string {
  return relativePath.endsWith(".tsx") ? "TSX" : "TS";
}

function toForwardSlashes(absolute: string): string {
  return absolute.replaceAll("\\", "/");
}

/** Run the native tsserver reference session over the sampled files. */
export async function runTsserverDirectSession(
  config: NativeReferenceConfig,
  workspaceRoot: string,
  sampleRelativePaths: readonly string[],
  log: (message: string) => void,
): Promise<NativeEngineReport> {
  const startedAt = Date.now();
  const trace = new NativeTraceWriter(config.traceDir, "tsserver");
  const observations: CorpusRequestObservation[] = [];
  const accounting: NativeAccounting = {
    requestsSent: 0,
    requestsAnswered: 0,
    requestsEmpty: 0,
    requestsTimedOut: 0,
    requestsErrored: 0,
    filesOpened: 0,
    filesSkipped: 0,
    probesMined: 0,
  };
  const perFileFirstRequestMs: number[] = [];
  let fatalError: string | null = null;
  let warmup: { ms: number; verdict: string } | null = null;

  const resolved = resolveTsserver(workspaceRoot, config.tsdk);
  const client = new TsserverDirectClient(
    process.execPath,
    resolved.tsserverJs,
    workspaceRoot,
    trace,
  );
  const spawnStart = Date.now();
  trace.line({
    t: spawnStart,
    ev: "spawn",
    provenance: resolved.provenance,
    tsVersion: resolved.tsVersion,
  });
  client.command("configure", { hostInfo: "verter-native-reference" });

  const fire = async (
    kind: CorpusRequestObservation["kind"],
    category: string,
    file: string,
    probe: { line: number; character: number },
    timeoutMs: number,
  ): Promise<CorpusRequestObservation> => {
    const command =
      kind === "hover"
        ? "quickinfo"
        : kind === "definition"
          ? "definition"
          : kind === "completion"
            ? "completionInfo"
            : "references";
    const base = { file, line: probe.line + 1, offset: probe.character + 1 };
    const args =
      kind === "completion"
        ? {
            ...base,
            includeExternalModuleExports: true,
            includeInsertTextCompletions: true,
            triggerCharacter: ".",
          }
        : base;
    accounting.requestsSent += 1;
    const start = Date.now();
    trace.line({ t: start, ev: "request", command, category, kind });
    const response = await client.request(command, args, timeoutMs);
    const ms = Date.now() - start;
    let verdict: CorpusRequestObservation["verdict"];
    if (response.kind === "ok") {
      const body = response.body as
        | { displayString?: string; entries?: unknown[]; refs?: unknown[] }
        | unknown[]
        | null
        | undefined;
      const empty =
        body == null ||
        (Array.isArray(body)
          ? body.length === 0
          : kind === "hover"
            ? String(body.displayString ?? "").trim().length === 0
            : kind === "completion"
              ? (body.entries ?? []).length === 0
              : kind === "references"
                ? (body.refs ?? []).length === 0
                : false);
      verdict = empty ? "empty" : "ok";
      accounting.requestsAnswered += 1;
      if (empty) accounting.requestsEmpty += 1;
    } else if (response.kind === "failure") {
      if (failureIsEmpty(response.message)) {
        verdict = "empty";
        accounting.requestsAnswered += 1;
        accounting.requestsEmpty += 1;
      } else {
        verdict = "error";
        accounting.requestsErrored += 1;
        trace.tally(`failure:${scrubMessage(response.message)}`);
      }
    } else if (response.kind === "timeout") {
      verdict = "timeout";
      accounting.requestsTimedOut += 1;
    } else {
      verdict = "error";
      accounting.requestsErrored += 1;
    }
    trace.line({ t: Date.now(), ev: "response", command, category, kind, ms, verdict });
    return { kind, category, ms, verdict, unexpectedEmpty: verdict === "empty" };
  };

  try {
    for (const [index, relativePath] of sampleRelativePaths.entries()) {
      const absolute = path.join(workspaceRoot, relativePath);
      let text: string;
      try {
        text = readFileSync(absolute, "utf8");
      } catch {
        accounting.filesSkipped += 1;
        continue;
      }
      const file = toForwardSlashes(absolute);
      client.command("open", {
        file,
        fileContent: text,
        scriptKindName: scriptKindFor(relativePath),
        projectRootPath: toForwardSlashes(workspaceRoot),
      });
      accounting.filesOpened += 1;
      trace.line({
        t: Date.now(),
        ev: "open",
        fileIndex: index,
        ...(config.includeFileDetail ? { relativePath } : {}),
        bytes: text.length,
      });

      if (index === 0) {
        const warmupStart = Date.now();
        const response = await client.request(
          "quickinfo",
          { file, line: 1, offset: 1 },
          config.warmupTimeoutMs,
        );
        warmup = {
          ms: Date.now() - warmupStart,
          verdict: response.kind === "ok" || response.kind === "failure" ? "ok" : response.kind,
        };
        trace.line({ t: Date.now(), ev: "warmup", ms: warmup.ms, verdict: warmup.verdict });
      }

      const probes: CorpusProbe[] = mineNativeTsProbes(text, config.maxProbesPerFile);
      accounting.probesMined += probes.length;
      let firstOfFile = true;
      for (const probe of probes) {
        for (const kind of probe.kinds) {
          if (!client.isAlive()) {
            fatalError = `tsserver died mid-session (before ${kind} @ file ${accounting.filesOpened})`;
            log(`[native-ref:tsserver] ${fatalError}`);
            break;
          }
          const observation = await fire(
            kind,
            probe.category,
            file,
            { line: probe.line, character: probe.character },
            config.requestTimeoutMs,
          );
          observations.push(observation);
          if (firstOfFile) {
            perFileFirstRequestMs.push(observation.ms);
            firstOfFile = false;
          }
        }
        if (fatalError !== null) break;
      }
      if (fatalError !== null) break;
      log(
        `[native-ref:tsserver] ${accounting.filesOpened}/${sampleRelativePaths.length} files, ` +
          `${accounting.requestsSent} requests`,
      );
    }
  } catch (error) {
    fatalError = String((error as Error)?.message ?? error).slice(0, 500);
    log(`[native-ref:tsserver] fatal: ${fatalError}`);
  } finally {
    client.kill();
  }

  return {
    engine: "tsserver",
    fatalError,
    provenance: `${resolved.provenance} (typescript ${resolved.tsVersion ?? "unknown"})`,
    startup: {
      spawnToInitializeMs: 0,
      serverName: "tsserver",
      serverVersion: resolved.tsVersion,
      warmup,
    },
    accounting,
    kinds: summarizeKinds(observations),
    perFileFirstRequestMs,
    serverMessageTallies: trace.talliesSnapshot(),
    clientRequestCount: client.requestCount,
    clientNotificationCount: client.notificationCount,
    bytesSentApprox: client.bytesSentApprox,
    bytesReceivedApprox: client.bytesReceivedApprox,
    wallClockMs: Date.now() - startedAt,
  };
}
