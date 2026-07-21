/**
 * `EnduranceSession` — the instrumented wrapper every scenario drives.
 *
 * It owns:
 *  - the document overlay (didOpen/didChange, full-document sync, per-file
 *    version counters) so probe positions are ALWAYS computed from the
 *    current in-session text (edit churn can shift line numbers);
 *  - request accounting: every LSP request increments `RequestTracker.sent`
 *    and exactly one settle bucket (answered/cancelled/errored/unanswered),
 *    with its latency recorded — a silent server drop surfaces as an
 *    `unanswered` timeout and fails the run;
 *  - a bounded in-flight pool so the harness itself never queues unboundedly;
 *  - needle-based probe execution (hover/completion/definition) with result
 *    normalizers and per-probe content validation.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import type { LspClient, LspPosition } from "@verter/lsp-test-client";

import {
  classifyRequestError,
  ConcurrencyPool,
  LatencyRecorder,
  RequestTracker,
  TypeQualityRecorder,
} from "./metrics.js";
import type { EnduranceConfig, RequestClassification } from "./types.js";

export interface OpenedDocument {
  readonly relativePath: string;
  readonly uri: string;
  readonly languageId: string;
  text: string;
  version: number;
}

/** LSP language identifier inferred from a document path. */
export function languageIdForPath(relativePath: string): string {
  const extension = path.posix.extname(relativePath.replaceAll("\\", "/")).toLowerCase();
  switch (extension) {
    case ".vue":
      return "vue";
    case ".svelte":
      return "svelte";
    case ".ts":
    case ".mts":
    case ".cts":
      return "typescript";
    case ".tsx":
      return "typescriptreact";
    case ".js":
    case ".mjs":
    case ".cjs":
      return "javascript";
    case ".jsx":
      return "javascriptreact";
    case ".json":
      return "json";
    default:
      return "plaintext";
  }
}

export interface SettledOutcome<T = unknown> {
  readonly classification: RequestClassification;
  readonly latencyMs: number;
  readonly result?: T;
  readonly error?: Error;
}

/** Extract every human-readable string from an LSP hover result. */
export function hoverText(result: unknown): string {
  if (result === null || result === undefined) return "";
  const contents = (result as { contents?: unknown }).contents;
  if (contents === null || contents === undefined) return "";
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) {
    return contents
      .map((marked) =>
        typeof marked === "string" ? marked : ((marked as { value?: string })?.value ?? ""),
      )
      .join("\n");
  }
  return (contents as { value?: string }).value ?? "";
}

/** Extract completion item labels from a CompletionItem[] | CompletionList | null. */
export function completionLabels(result: unknown): string[] {
  if (result === null || result === undefined) return [];
  const items = Array.isArray(result) ? result : ((result as { items?: unknown[] }).items ?? []);
  return items
    .map((item) => (item as { label?: unknown })?.label)
    .filter((label): label is string => typeof label === "string");
}

/**
 * Template-idiomatic (kebab-case) form of a camelCase prop name — the form
 * Verter's native component-attr completion offers in templates.
 */
export function camelToKebab(name: string): string {
  return name.replace(/[A-Z]/g, (char) => `-${char.toLowerCase()}`);
}

export interface DefinitionTarget {
  readonly uri: string;
  readonly line: number;
}

/** Normalize Location | Location[] | LocationLink[] | null to {uri, line} targets. */
export function definitionTargets(result: unknown): DefinitionTarget[] {
  if (result === null || result === undefined) return [];
  const list = Array.isArray(result) ? result : [result];
  const targets: DefinitionTarget[] = [];
  for (const entry of list as any[]) {
    if (!entry || typeof entry !== "object") continue;
    if (typeof entry.targetUri === "string") {
      const range = entry.targetSelectionRange ?? entry.targetRange;
      targets.push({ uri: entry.targetUri, line: range?.start?.line ?? -1 });
    } else if (typeof entry.uri === "string") {
      targets.push({ uri: entry.uri, line: entry.range?.start?.line ?? -1 });
    }
  }
  return targets;
}

// ── Probes ────────────────────────────────────────────────────────────────

interface ProbeBase {
  /** Document (workspace-relative posix path) the probe targets. */
  readonly relativePath: string;
  /**
   * Literal text locating the probe in the CURRENT document text; the cursor
   * sits at `indexOf(needle, occurrence) + cursorOffset`.
   */
  readonly needle: string;
  readonly occurrence?: number;
  readonly cursorOffset?: number;
  /** Human label for failure messages. */
  readonly label: string;
  /**
   * When true the probe is INFORMATIONAL: it targets documented type-quality
   * gap territory (provider member-access results — e.g. script `props.`
   * completion, `props.<member>` hover). Only SETTLING is asserted (an
   * unanswered/errored request still fails); the content expectation is not
   * checked and the observed quality feeds the receipt's `typeQuality` data.
   */
}

interface HoverProbeFields extends ProbeBase {
  readonly kind: "hover";
  readonly expectIncludes: readonly string[];
  /**
   * Assert the hover ANSWERS with non-empty content (positions where Verter
   * owns a native answer — template attr names, component tags, Vue binding
   * hovers) without asserting anything about the provider's type text.
   */
  readonly requireNonEmpty?: boolean;
}

/** Hard typed hovers must explicitly reject `any`; informational observations may omit it. */
export type HoverProbe = HoverProbeFields &
  (
    | { readonly informational: true; readonly forbidIncludes?: readonly string[] }
    | { readonly informational?: false; readonly forbidIncludes: readonly string[] }
  );

export interface CompletionProbe extends ProbeBase {
  readonly kind: "completion";
  readonly expectLabels: readonly string[];
  readonly forbidLabels?: readonly string[];
  readonly informational?: boolean;
}

export interface DefinitionProbe extends ProbeBase {
  readonly kind: "definition";
  /** Expected target document — defaults to the probe's own file. */
  readonly expectUriSuffix?: string;
  /**
   * When set, the definition must land on the line containing this needle in
   * the CURRENT text of the target file (robust to line shifts from edits).
   */
  readonly expectLineNeedle?: string;
  readonly informational?: boolean;
}

export type EnduranceProbe = HoverProbe | CompletionProbe | DefinitionProbe;

export interface ProbeOutcome {
  readonly classification: RequestClassification;
  readonly latencyMs: number;
  /** null when the response content matches every expectation. */
  readonly mismatch: string | null;
  readonly result: unknown;
}

export interface EnduranceSessionOptions {
  readonly config: EnduranceConfig;
  readonly recorder: LatencyRecorder;
  readonly tracker: RequestTracker;
  /** Pool override (defaults to config.maxInFlight / config.requestTimeoutMs). */
  readonly pool?: ConcurrencyPool;
}

export class EnduranceSession {
  readonly tracker: RequestTracker;
  readonly recorder: LatencyRecorder;
  /** INFORMATIONAL type-quality observations (never asserted). */
  readonly typeQuality = new TypeQualityRecorder();

  private readonly documents = new Map<string, OpenedDocument>();
  private readonly pool: ConcurrencyPool;
  private readonly config: EnduranceConfig;

  constructor(
    readonly client: LspClient,
    readonly workspaceRoot: string,
    options: EnduranceSessionOptions,
  ) {
    this.config = options.config;
    this.recorder = options.recorder;
    this.tracker = options.tracker;
    this.pool =
      options.pool ?? new ConcurrencyPool(this.config.maxInFlight, this.config.requestTimeoutMs);
  }

  uriFor(relativePath: string): string {
    return pathToFileURL(path.join(this.workspaceRoot, relativePath)).href;
  }

  /** Current in-session text; falls back to disk for unopened files. */
  textOf(relativePath: string): string {
    const normalized = relativePath.replaceAll("\\", "/");
    const opened = this.documents.get(normalized);
    if (opened) return opened.text;
    return readFileSync(path.join(this.workspaceRoot, normalized), "utf8");
  }

  isOpen(relativePath: string): boolean {
    return this.documents.has(relativePath.replaceAll("\\", "/"));
  }

  /**
   * didOpen a document. `text` defaults to the on-disk content (pass an
   * explicit string — e.g. "" — for a buffer typed from scratch).
   */
  openFile(relativePath: string, text?: string, languageId?: string): OpenedDocument {
    const normalized = relativePath.replaceAll("\\", "/");
    if (this.documents.has(normalized)) {
      throw new Error(`document already open: ${normalized}`);
    }
    const content = text ?? readFileSync(path.join(this.workspaceRoot, normalized), "utf8");
    const resolvedLanguageId = languageId ?? languageIdForPath(normalized);
    const document: OpenedDocument = {
      relativePath: normalized,
      uri: this.uriFor(normalized),
      languageId: resolvedLanguageId,
      text: content,
      version: 1,
    };
    this.documents.set(normalized, document);
    this.client.sendNotification("textDocument/didOpen", {
      textDocument: {
        uri: document.uri,
        languageId: resolvedLanguageId,
        version: document.version,
        text: content,
      },
    });
    return document;
  }

  /** Full-document didChange; returns the new version. */
  changeFile(relativePath: string, text: string): number {
    const normalized = relativePath.replaceAll("\\", "/");
    const document = this.documents.get(normalized);
    if (!document) throw new Error(`document not open: ${normalized}`);
    document.version += 1;
    document.text = text;
    this.tracker.editsSent += 1;
    this.client.sendNotification("textDocument/didChange", {
      textDocument: { uri: document.uri, version: document.version },
      contentChanges: [{ text }],
    });
    return document.version;
  }

  closeFile(relativePath: string): void {
    const normalized = relativePath.replaceAll("\\", "/");
    const document = this.documents.get(normalized);
    if (!document) return;
    this.documents.delete(normalized);
    this.client.sendNotification("textDocument/didClose", {
      textDocument: { uri: document.uri },
    });
  }

  /** LSP position of `indexOf(needle, occurrence) + cursorOffset` in current text. */
  positionOf(relativePath: string, needle: string, occurrence = 0, cursorOffset = 0): LspPosition {
    const text = this.textOf(relativePath);
    let offset = -1;
    let from = 0;
    for (let hit = 0; hit <= occurrence; hit += 1) {
      offset = text.indexOf(needle, from);
      if (offset === -1) {
        throw new Error(
          `needle ${JSON.stringify(needle)} (occurrence ${occurrence}) not found in ${relativePath}`,
        );
      }
      from = offset + 1;
    }
    const target = offset + cursorOffset;
    return this.client
      .documentPositions(text)
      .utf16ToPosition(target, this.client.positionEncoding);
  }

  /** 0-based line of `indexOf(needle, occurrence)` in the current text. */
  lineOf(relativePath: string, needle: string, occurrence = 0): number {
    const position = this.positionOf(relativePath, needle, occurrence);
    return position.line;
  }

  /**
   * Tracked request core. Every call increments `sent` and exactly one settle
   * bucket; latency is always recorded. Rethrows on settle-by-rejection.
   */
  async request<T = unknown>(method: string, params?: unknown, timeoutMs?: number): Promise<T> {
    const timeout = timeoutMs ?? this.config.requestTimeoutMs;
    const startedAt = Date.now();
    this.tracker.sent += 1;
    try {
      const result = await this.pool.run(() => this.client.sendRequest<T>(method, params, timeout));
      this.tracker.settle("answered");
      this.recorder.record(method, Date.now() - startedAt, true);
      return result;
    } catch (error) {
      const classification = classifyRequestError(error);
      this.tracker.settle(classification);
      this.recorder.record(method, Date.now() - startedAt, false);
      throw error;
    }
  }

  /** Tracked request that never throws — the storm/soak worker primitive. */
  async settled<T = unknown>(
    method: string,
    params?: unknown,
    timeoutMs?: number,
  ): Promise<SettledOutcome<T>> {
    const startedAt = Date.now();
    try {
      const result = await this.request<T>(method, params, timeoutMs);
      return { classification: "answered", latencyMs: Date.now() - startedAt, result };
    } catch (error) {
      const classification = classifyRequestError(error);
      return {
        classification,
        latencyMs: Date.now() - startedAt,
        error: error instanceof Error ? error : new Error(String(error)),
      };
    }
  }

  /** Shortcuts for the standard provider requests (tracked, strict). */
  hover(relativePath: string, position: LspPosition, timeoutMs?: number): Promise<unknown> {
    return this.request(
      "textDocument/hover",
      { textDocument: { uri: this.uriFor(relativePath) }, position },
      timeoutMs ?? this.config.probeTimeoutMs,
    );
  }

  completion(relativePath: string, position: LspPosition, timeoutMs?: number): Promise<unknown> {
    return this.request(
      "textDocument/completion",
      { textDocument: { uri: this.uriFor(relativePath) }, position },
      timeoutMs ?? this.config.probeTimeoutMs,
    );
  }

  definition(relativePath: string, position: LspPosition, timeoutMs?: number): Promise<unknown> {
    return this.request(
      "textDocument/definition",
      { textDocument: { uri: this.uriFor(relativePath) }, position },
      timeoutMs ?? this.config.probeTimeoutMs,
    );
  }

  private methodAndParams(probe: EnduranceProbe): { method: string; params: unknown } {
    const position = this.positionOf(
      probe.relativePath,
      probe.needle,
      probe.occurrence ?? 0,
      probe.cursorOffset ?? 0,
    );
    const uri = this.uriFor(probe.relativePath);
    switch (probe.kind) {
      case "hover":
        return { method: "textDocument/hover", params: { textDocument: { uri }, position } };
      case "completion":
        return { method: "textDocument/completion", params: { textDocument: { uri }, position } };
      case "definition":
        return { method: "textDocument/definition", params: { textDocument: { uri }, position } };
    }
  }

  /** Content validation for a probe response; null = matches all expectations. */
  validateProbeResult(probe: EnduranceProbe, result: unknown): string | null {
    switch (probe.kind) {
      case "hover": {
        const text = hoverText(result);
        if (probe.requireNonEmpty && text.trim().length === 0) {
          return `hover ${probe.label}: expected a non-empty native answer, got empty hover`;
        }
        const missing = probe.expectIncludes.filter((fragment) => !text.includes(fragment));
        if (missing.length > 0) {
          return `hover ${probe.label}: expected fragments ${JSON.stringify(missing)} in ${JSON.stringify(text.slice(0, 300))}`;
        }
        const forbidden = (probe.forbidIncludes ?? []).filter((fragment) =>
          text.includes(fragment),
        );
        return forbidden.length > 0
          ? `hover ${probe.label}: forbidden fragments present ${JSON.stringify(forbidden)} in ${JSON.stringify(text.slice(0, 300))}`
          : null;
      }
      case "completion": {
        const labels = completionLabels(result);
        const missing = probe.expectLabels.filter((label) => !labels.includes(label));
        if (missing.length > 0) {
          return `completion ${probe.label}: missing labels ${JSON.stringify(missing)} (got ${labels.length} items, first: ${JSON.stringify(labels.slice(0, 10))})`;
        }
        const forbidden = (probe.forbidLabels ?? []).filter((label) => labels.includes(label));
        return forbidden.length > 0
          ? `completion ${probe.label}: forbidden labels present ${JSON.stringify(forbidden)}`
          : null;
      }
      case "definition": {
        const targets = definitionTargets(result);
        if (targets.length === 0) {
          return `definition ${probe.label}: no targets returned`;
        }
        const uriSuffix = probe.expectUriSuffix ?? `/${probe.relativePath}`;
        const uriMatches = targets.filter((target) => target.uri.endsWith(uriSuffix));
        if (uriMatches.length === 0) {
          return `definition ${probe.label}: no target ending ${JSON.stringify(uriSuffix)} in ${JSON.stringify(targets)}`;
        }
        if (probe.expectLineNeedle !== undefined) {
          const targetFile = probe.expectUriSuffix
            ? probe.expectUriSuffix.replace(/^\//, "")
            : probe.relativePath;
          const expectedLine = this.lineOf(targetFile, probe.expectLineNeedle);
          if (!uriMatches.some((target) => target.line === expectedLine)) {
            return `definition ${probe.label}: expected line ${expectedLine} (needle ${JSON.stringify(probe.expectLineNeedle)}) in ${JSON.stringify(uriMatches)}`;
          }
        }
        return null;
      }
    }
  }

  /**
   * Execute a probe through the tracked-settled path and validate its content.
   * Never throws for server behavior; `mismatch` reports content failures.
   * Every answered hover/completion feeds the INFORMATIONAL type-quality
   * recorder (data only — the documented provider type-quality gaps are
   * observed in the receipt, never asserted here). Informational probes skip
   * content validation entirely: only settling is asserted for them.
   */
  async runProbe(probe: EnduranceProbe, timeoutMs?: number): Promise<ProbeOutcome> {
    const { method, params } = this.methodAndParams(probe);
    const outcome = await this.settled(method, params, timeoutMs ?? this.config.probeTimeoutMs);
    if (outcome.classification === "answered") {
      if (probe.kind === "hover") this.typeQuality.recordHover(hoverText(outcome.result));
      if (probe.kind === "completion") {
        this.typeQuality.recordCompletion(completionLabels(outcome.result));
      }
    }
    const mismatch =
      outcome.classification === "answered" && !probe.informational
        ? this.validateProbeResult(probe, outcome.result)
        : null;
    return {
      classification: outcome.classification,
      latencyMs: outcome.latencyMs,
      mismatch,
      result: outcome.result,
    };
  }

  /** Strict probe: throws unless answered AND content-correct. */
  async requireProbe(probe: EnduranceProbe, timeoutMs?: number): Promise<ProbeOutcome> {
    const outcome = await this.runProbe(probe, timeoutMs);
    if (outcome.classification !== "answered") {
      throw new Error(
        `probe ${probe.label} settled as ${outcome.classification}, expected answered`,
      );
    }
    if (outcome.mismatch) {
      throw new Error(outcome.mismatch);
    }
    return outcome;
  }
}
