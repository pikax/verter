/**
 * Direct tsgo session: the native reference for the `tsgo` engine.
 *
 * Spawns the EXACT process shape Verter's `TsgoTypeProvider::spawn` uses —
 * `<tsgo> --lsp --stdio` with the workspace root as `rootUri`/workspace folder
 * — resolved from the same 2-tier front of Verter's toolchain order
 * (`VERTER_TSGO_BIN`, else the repo TypeScript-7 package's platform resolver,
 * exactly like `corpus-gate/spawn.ts`). The client capabilities mirror the
 * completion/diagnostic capability surface `build_client_capabilities()`
 * advertises in `crates/verter_type_runtime/src/tsgo/ipc.rs`, so tsgo behaves
 * the same as it does under Verter. No Verter process is anywhere in the loop:
 * every request goes straight from this driver to tsgo over stdio.
 */
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";

import { summarizeKinds } from "../corpus-gate/metrics.js";
import type { CorpusProbe } from "../corpus-gate/probes.js";
import { sampleManifestHash } from "../corpus-gate/sample.js";
import { REPO_ROOT } from "../corpus-gate/spawn.js";
import type { CorpusRequestObservation } from "../corpus-gate/types.js";
import {
  completionIsEmpty,
  definitionIsEmpty,
  hoverIsEmpty,
  referencesIsEmpty,
} from "../corpus-gate/verdicts.js";
import { mineNativeTsProbes } from "./probes.js";
import { NativeTraceWriter } from "./trace.js";
import type { NativeAccounting, NativeEngineReport, NativeReferenceConfig } from "./types.js";

/**
 * The completion + diagnostic capability surface Verter advertises to tsgo
 * (`build_client_capabilities()` in `verter_type_runtime/src/tsgo/ipc.rs`),
 * mirrored so tsgo gates the same optional features on both sides.
 */
function verterEquivalentTsgoCapabilities(): Record<string, unknown> {
  return {
    textDocument: {
      publishDiagnostics: {
        tagSupport: { valueSet: [1, 2] },
        relatedInformation: true,
      },
      diagnostic: {
        tagSupport: { valueSet: [1, 2] },
        relatedInformation: true,
      },
      completion: {
        contextSupport: true,
        completionItemKind: {
          valueSet: [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25,
          ],
        },
        completionItem: {
          snippetSupport: true,
          commitCharactersSupport: true,
          preselectSupport: true,
          labelDetailsSupport: true,
          resolveSupport: {
            properties: ["documentation", "detail", "additionalTextEdits", "labelDetails"],
          },
        },
      },
      definition: { linkSupport: true },
      hover: { contentFormat: ["markdown", "plaintext"] },
    },
    workspace: { workspaceFolders: true },
  };
}

interface ResolvedTsgo {
  readonly bin: string;
  readonly provenance: string;
}

function requireFile(label: string, candidate: string): string {
  const resolved = path.resolve(candidate);
  if (!existsSync(resolved) || !statSync(resolved).isFile()) {
    throw new Error(`${label} is required and must be a file: ${resolved}`);
  }
  return resolved;
}

/** Mirrors `resolveTsgoBinary` in `corpus-gate/spawn.ts` (same order). */
async function resolveTsgoBinary(explicit: string | null): Promise<ResolvedTsgo> {
  if (explicit) return { bin: requireFile("tsgo binary", explicit), provenance: "explicit" };
  if (process.env.VERTER_TSGO_BIN) {
    return {
      bin: requireFile("VERTER_TSGO_BIN", process.env.VERTER_TSGO_BIN),
      provenance: "VERTER_TSGO_BIN",
    };
  }
  const resolver = path.join(REPO_ROOT, "node_modules", "typescript", "lib", "getExePath.js");
  requireFile("TypeScript executable resolver", resolver);
  const module = (await import(pathToFileURL(resolver).href)) as { default?: () => string };
  if (typeof module.default !== "function") {
    throw new Error(`TypeScript executable resolver has no default function: ${resolver}`);
  }
  return {
    bin: requireFile("TypeScript 7 native executable", module.default()),
    provenance: "repo-typescript-package",
  };
}

function languageIdFor(relativePath: string): string {
  return relativePath.endsWith(".tsx") ? "typescriptreact" : "typescript";
}

/**
 * Run the native tsgo reference session over `sampleRelativePaths` inside
 * `workspaceRoot` (the mirror workspace with the derived analogues).
 */
export async function runTsgoDirectSession(
  config: NativeReferenceConfig,
  workspaceRoot: string,
  sampleRelativePaths: readonly string[],
  log: (message: string) => void,
): Promise<NativeEngineReport> {
  const startedAt = Date.now();
  const trace = new NativeTraceWriter(config.traceDir, "tsgo");
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
  let bytesSentApprox = 0;
  let bytesReceivedApprox = 0;
  let clientRequestCount = 0;
  let clientNotificationCount = 0;
  let fatalError: string | null = null;
  let serverName: string | null = null;
  let serverVersion: string | null = null;
  let spawnToInitializeMs = 0;
  let warmup: { ms: number; verdict: string } | null = null;

  const { bin, provenance } = await resolveTsgoBinary(config.tsgoBin);
  const rootUri = pathToFileURL(workspaceRoot).href;

  const client = new LspClient("native-tsgo", bin, ["--lsp", "--stdio"], workspaceRoot, {
    defaultTimeout: config.requestTimeoutMs,
    stderr: { maxBytes: 4 * 1024 * 1024 },
    onAnyNotification(method, params) {
      trace.tally(method);
      trace.line({
        t: Date.now(),
        ev: "server-notification",
        method,
        bytes: JSON.stringify(params ?? null).length,
      });
    },
  });

  /** Send one counted request and classify the settlement like the corpus gate. */
  const fire = async (
    kind: CorpusRequestObservation["kind"],
    category: string,
    uri: string,
    position: { line: number; character: number },
  ): Promise<CorpusRequestObservation> => {
    const method =
      kind === "hover"
        ? "textDocument/hover"
        : kind === "definition"
          ? "textDocument/definition"
          : kind === "completion"
            ? "textDocument/completion"
            : "textDocument/references";
    const params =
      kind === "completion"
        ? {
            textDocument: { uri },
            position,
            context: { triggerKind: 2, triggerCharacter: "." },
          }
        : kind === "references"
          ? { textDocument: { uri }, position, context: { includeDeclaration: true } }
          : { textDocument: { uri }, position };
    const requestBytes = JSON.stringify({ method, params }).length;
    bytesSentApprox += requestBytes;
    clientRequestCount += 1;
    accounting.requestsSent += 1;
    const start = Date.now();
    trace.line({ t: start, ev: "request", method, category, kind, bytes: requestBytes });
    let verdict: CorpusRequestObservation["verdict"];
    let responseBytes = 0;
    try {
      const result = await client.sendRequest(method, params, config.requestTimeoutMs);
      responseBytes = JSON.stringify(result ?? null).length;
      bytesReceivedApprox += responseBytes;
      const empty =
        kind === "hover"
          ? hoverIsEmpty(result)
          : kind === "definition"
            ? definitionIsEmpty(result)
            : kind === "completion"
              ? completionIsEmpty(result)
              : referencesIsEmpty(result);
      verdict = empty ? "empty" : "ok";
      accounting.requestsAnswered += 1;
      if (empty) accounting.requestsEmpty += 1;
    } catch (error) {
      const message = String((error as Error)?.message ?? error);
      verdict = /timed out|timeout/i.test(message) ? "timeout" : "error";
      if (verdict === "timeout") accounting.requestsTimedOut += 1;
      else accounting.requestsErrored += 1;
    }
    const ms = Date.now() - start;
    trace.line({
      t: Date.now(),
      ev: "response",
      method,
      category,
      kind,
      ms,
      verdict,
      bytes: responseBytes,
    });
    return { kind, category, ms, verdict, unexpectedEmpty: verdict === "empty" };
  };

  try {
    const initializeStart = Date.now();
    trace.line({ t: initializeStart, ev: "spawn", bin: path.basename(bin), provenance });
    const initialized = (await client.initialize(
      {
        processId: process.pid,
        rootUri,
        capabilities: verterEquivalentTsgoCapabilities(),
        workspaceFolders: [{ uri: rootUri, name: "workspace" }],
      },
      60_000,
    )) as { serverInfo?: { name?: string; version?: string }; capabilities?: unknown };
    spawnToInitializeMs = Date.now() - initializeStart;
    serverName = initialized.serverInfo?.name ?? null;
    serverVersion = initialized.serverInfo?.version ?? null;
    client.sendNotification("initialized", {});
    clientNotificationCount += 1;
    // The full advertised server capability surface — the "what does tsgo
    // offer" inventory — goes to the trace for the engine-capability analysis.
    trace.line({
      t: Date.now(),
      ev: "initialize-result",
      ms: spawnToInitializeMs,
      serverName,
      serverVersion,
      capabilities: initialized.capabilities ?? null,
    });

    for (const [index, relativePath] of sampleRelativePaths.entries()) {
      const absolute = path.join(workspaceRoot, relativePath);
      let text: string;
      try {
        text = readFileSync(absolute, "utf8");
      } catch {
        accounting.filesSkipped += 1;
        continue;
      }
      const uri = pathToFileURL(absolute).href;
      const didOpenBytes = text.length;
      client.sendNotification("textDocument/didOpen", {
        textDocument: { uri, languageId: languageIdFor(relativePath), version: 1, text },
      });
      clientNotificationCount += 1;
      bytesSentApprox += didOpenBytes;
      accounting.filesOpened += 1;
      trace.line({
        t: Date.now(),
        ev: "open",
        fileIndex: index,
        ...(config.includeFileDetail ? { relativePath } : {}),
        bytes: didOpenBytes,
      });

      // The single uncounted warmup probe on the FIRST file: the provider's
      // project-load cost, surfaced explicitly (the corpus gate absorbs the
      // equivalent cost in its bounded startup settle before counting probes).
      if (index === 0) {
        const warmupStart = Date.now();
        let warmupVerdict = "ok";
        try {
          await client.sendRequest(
            "textDocument/hover",
            { textDocument: { uri }, position: { line: 0, character: 0 } },
            config.warmupTimeoutMs,
          );
        } catch (error) {
          warmupVerdict = /timed out|timeout/i.test(String((error as Error)?.message ?? error))
            ? "timeout"
            : "error";
        }
        warmup = { ms: Date.now() - warmupStart, verdict: warmupVerdict };
        trace.line({ t: Date.now(), ev: "warmup", ms: warmup.ms, verdict: warmupVerdict });
      }

      const probes: CorpusProbe[] = mineNativeTsProbes(text, config.maxProbesPerFile);
      accounting.probesMined += probes.length;
      let firstOfFile = true;
      for (const probe of probes) {
        for (const kind of probe.kinds) {
          if (!client.isAlive()) {
            fatalError = `tsgo died mid-session (before ${kind} @ file ${accounting.filesOpened})`;
            log(`[native-ref:tsgo] ${fatalError}`);
            break;
          }
          const observation = await fire(kind, probe.category, uri, {
            line: probe.line,
            character: probe.character,
          });
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
        `[native-ref:tsgo] ${accounting.filesOpened}/${sampleRelativePaths.length} files, ` +
          `${accounting.requestsSent} requests`,
      );
    }
  } catch (error) {
    fatalError = String((error as Error)?.message ?? error).slice(0, 500);
    log(`[native-ref:tsgo] fatal: ${fatalError}`);
  } finally {
    await client.kill().catch(() => {});
  }

  return {
    engine: "tsgo",
    fatalError,
    provenance,
    startup: { spawnToInitializeMs, serverName, serverVersion, warmup },
    accounting,
    kinds: summarizeKinds(observations),
    perFileFirstRequestMs,
    serverMessageTallies: trace.talliesSnapshot(),
    clientRequestCount,
    clientNotificationCount,
    bytesSentApprox,
    bytesReceivedApprox,
    wallClockMs: Date.now() - startedAt,
  };
}

/** Stable manifest hash re-export so the lane proves its sample identity. */
export { sampleManifestHash };
