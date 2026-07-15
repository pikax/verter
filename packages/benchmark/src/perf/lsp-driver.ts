/**
 * LSP-driven interactive + warm workload samplers (§2.7 axis B).
 *
 * Drives the `verter_lsp` binary through `@verter/lsp-test-client` over
 * JSON-RPC stdio against the materialized perf corpus. Every sampler drives
 * MANY operations against ONE persistent client and returns the PER-OPERATION
 * latency DISTRIBUTION (so the gate computes real p50/p95/p99 over the pooled
 * distribution — never a single latency/run collapsed to a median):
 *  - `editToDiagnosticsLatency` — open the active SFC, apply many in-file edits,
 *    measure each edit→updated-`publishDiagnostics`; also count the distinct
 *    document URIs that re-publish after a single-file edit (the behavioral
 *    diagnostic-publication-locality invariant — a publishDiagnostics-URI proxy,
 *    NOT real invalidation/recheck) and capture the diagnostic SET.
 *  - `ideQueryLatency` — many hovers and many completions, as SEPARATE
 *    distributions (a regression in one path cannot hide behind the other).
 *  - `warmDependencyEditLatency` — the genuinely-warm signal: ONE persistent
 *    client retains the Program across many edits while the dependent SFC is
 *    re-typechecked against an imported type module (a real cross-file resolution
 *    per edit), measuring each edit→updated-`publishDiagnostics`.
 *
 * The verter_lsp version-gate (§2.7) means a query whose snapshot is not yet
 * synced returns no result and retries; these samplers wait for `$/verter/ready`
 * before the active-file interaction so the workspace scan is not counted.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { LspClient, type LspClientOptions } from "@verter/lsp-test-client";
import type { EnsuredCorpus } from "./corpus.js";
import { normalizeDiagMessage, normalizeDiagPath } from "./workloads.js";

const PHASE_TIMEOUT = 120_000;
const SHORT_TIMEOUT = 30_000;
/** Quiescence window after an edit, to let the affected-set burst land. */
const SETTLE_MS = 200;

export interface DriverOptions {
  /** Number of operations to drive (the per-operation distribution size). */
  readonly ops: number;
}

/**
 * The subset of the LSP client the samplers drive. Injecting it (via
 * {@link DriverConnect}) lets a spec exercise the real measured loops — the
 * no-swallow request/wait handling and the non-empty-result gating — without a
 * live `verter_lsp` binary. `LspClient` satisfies this surface.
 */
export interface DriverClient {
  sendNotification(method: string, params?: unknown): void;
  sendRequest<T>(method: string, params?: unknown, timeout?: number): Promise<T>;
  onNotification(method: string, handler: (params: unknown) => void): void;
  offNotification(method: string, handler: (params: unknown) => void): void;
  waitForNotification(
    method: string,
    timeout?: number,
    predicate?: (params: unknown) => boolean,
  ): Promise<unknown>;
  kill(): Promise<void> | void;
}

/** Spawn (or fake) a ready client + its workspace root URI. */
export type DriverConnect = (
  binPath: string,
  corpus: EnsuredCorpus,
  label: string,
) => Promise<{ client: DriverClient; rootUri: string }>;

/**
 * Whether an LSP `Hover.contents` value carries a NON-EMPTY result. Validates
 * every shape the protocol allows:
 *  - `MarkedString` as a plain string (non-empty after trim),
 *  - `MarkupContent` (`{ kind, value }`) and object `MarkedString`
 *    (`{ language, value }`) — the `.value` must be a non-empty string,
 *  - `MarkedString[]` — a hit iff SOME entry is itself non-empty (so a non-empty
 *    array of all-empty entries is NOT a hit).
 * An empty-string / whitespace-only value is treated as no result.
 */
function hoverContentsNonEmpty(contents: unknown): boolean {
  if (typeof contents === "string") return contents.trim() !== "";
  if (Array.isArray(contents)) return contents.some((c) => hoverContentsNonEmpty(c));
  if (contents !== null && typeof contents === "object" && "value" in contents) {
    const value = (contents as { value?: unknown }).value;
    return typeof value === "string" && value.trim() !== "";
  }
  return false;
}

/**
 * A hover result MUST carry NON-EMPTY contents — a null/empty hover (including a
 * `MarkupContent`/`MarkedString` whose `.value` is empty, or an array of only
 * empty entries) is a broken/no-result query: a no-op LSP that "answers" fast
 * with empty rich content must NOT read as a passing sample.
 */
export function assertHoverResult(hover: unknown, label = "hover"): void {
  const contents = (hover as { contents?: unknown } | null | undefined)?.contents;
  if (hover == null || contents == null || !hoverContentsNonEmpty(contents)) {
    throw new Error(`${label}: no hover contents — a broken/no-result LSP query`);
  }
}

/**
 * Whether a completion item carries a NON-BLANK label — real content. A
 * `CompletionItem.label` is either a plain string or a `CompletionItemLabel`
 * `{ label: string }` object (LSP ≥ 3.17). A shell (`{}`), a `null`, or a
 * blank/whitespace-only label is NOT content.
 */
function completionItemHasLabel(item: unknown): boolean {
  if (item === null || typeof item !== "object") return false;
  const label = (item as { label?: unknown }).label;
  if (typeof label === "string") return label.trim() !== "";
  if (label !== null && typeof label === "object") {
    const inner = (label as { label?: unknown }).label;
    return typeof inner === "string" && inner.trim() !== "";
  }
  return false;
}

/**
 * Count completion items that carry real content, or THROW on a no-result query.
 * A non-empty array is NOT automatically a hit: a no-op LSP answering fast with
 * content-less shells (`[{}]` / `[null]`) or all-blank labels must NOT read as a
 * passing sample (mirrors the hover content check above). Returns the count of
 * VALID (non-blank-label) items.
 */
export function completionItemCount(completion: unknown, label = "completion"): number {
  const items = Array.isArray(completion)
    ? completion
    : (completion as { items?: unknown[] } | null | undefined)?.items;
  if (!items || items.length === 0) {
    throw new Error(`${label}: no completion items — a broken/no-result LSP query`);
  }
  const valid = items.filter(completionItemHasLabel).length;
  if (valid === 0) {
    throw new Error(
      `${label}: ${items.length} completion item(s) but none carry a non-blank label — a broken/no-content LSP query`,
    );
  }
  return valid;
}

/**
 * Flatten an LSP `Hover.contents` value to its TEXT (every protocol shape:
 * plain string, `MarkupContent`/object `MarkedString` `.value`, or an array of
 * those). Used to capture the hover CONTENT for candidate-vs-baseline equality —
 * an empty/whitespace hover is already rejected by {@link assertHoverResult}.
 */
export function hoverContentText(contents: unknown): string {
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) return contents.map(hoverContentText).join("\n");
  if (contents !== null && typeof contents === "object" && "value" in contents) {
    const value = (contents as { value?: unknown }).value;
    return typeof value === "string" ? value : "";
  }
  return "";
}

/**
 * Extract every NON-BLANK completion label (plain string label or
 * `CompletionItemLabel` `{ label }`), for capturing the completion CONTENT (the
 * label SET) — distinct from {@link completionItemCount}, which only counts. A
 * content-less item contributes no label.
 */
export function completionLabelTexts(completion: unknown): string[] {
  const items = Array.isArray(completion)
    ? completion
    : (completion as { items?: unknown[] } | null | undefined)?.items;
  if (!items) return [];
  const out: string[] = [];
  for (const item of items) {
    if (item === null || typeof item !== "object") continue;
    const label = (item as { label?: unknown }).label;
    if (typeof label === "string") {
      if (label.trim() !== "") out.push(label.trim());
    } else if (label !== null && typeof label === "object") {
      const inner = (label as { label?: unknown }).label;
      if (typeof inner === "string" && inner.trim() !== "") out.push(inner.trim());
    }
  }
  return out;
}

/**
 * Normalize an IDE-query result string (hover text / completion label) for
 * cross-side equality: collapse the per-side carrier hash + per-run temp dir and
 * relativize the side's root (so the candidate's and baseline's PHYSICALLY-distinct
 * working trees do not false-diverge), then collapse internal whitespace. A
 * LOGICAL content change (a different type, a different label) still registers.
 */
function normalizeQueryText(text: string, rootDir: string): string {
  return normalizeDiagMessage(text, rootDir).replace(/\s+/g, " ").trim();
}

/** The deterministic identifiers the IDE-query content probe targets, in order. */
const QUERY_PROBE_IDENTIFIERS = ["props", "defineProps", "recompute", "emit"] as const;

/**
 * A FIXED, deterministic set of probe positions for the IDE-query content
 * correctness pass: the first occurrence of each {@link QUERY_PROBE_IDENTIFIERS}
 * that actually appears in the active SFC (deduped by position). Always ≥ 1 entry.
 */
export function deterministicQueryPositions(text: string): { line: number; character: number }[] {
  const positions: { line: number; character: number }[] = [];
  const seen = new Set<string>();
  for (const ident of QUERY_PROBE_IDENTIFIERS) {
    if (!text.includes(ident)) continue;
    const p = findIdentifierPosition(text, ident);
    const key = `${p.line}:${p.character}`;
    if (!seen.has(key)) {
      positions.push(p);
      seen.add(key);
    }
  }
  if (positions.length === 0) positions.push({ line: 0, character: 0 });
  return positions;
}

/** Corpus-relative path for a `file://` URI (via fileURLToPath, NOT hand-strip). */
export function relativizeUri(uri: string, rootDir: string): string {
  try {
    return relative(rootDir, fileURLToPath(uri)).split(/[\\/]/).join("/");
  } catch {
    return uri;
  }
}

function clientOptions(name: string): LspClientOptions {
  return {
    onError: (err: Error) => {
      if (process.env.PERF_LSP_DEBUG) console.error(`[${name}] process error:`, err.message);
    },
  };
}

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/** Pick a deterministic active SFC deep in the app project (rich import set). */
function activeSfc(corpus: EnsuredCorpus): string {
  const appComponents = join(corpus.dir, "app");
  const found = firstVue(appComponents);
  if (!found) throw new Error("no app SFC found for the interactive workload");
  return found;
}

function firstVue(dir: string): string | null {
  let result: string | null = null;
  const entries = (() => {
    try {
      return readdirSync(dir, { withFileTypes: true, encoding: "utf-8" });
    } catch {
      return [] as import("node:fs").Dirent<string>[];
    }
  })();
  const dirs = entries.filter((e) => e.isDirectory()).sort((a, b) => (a.name < b.name ? -1 : 1));
  const files = entries
    .filter((e) => !e.isDirectory() && e.name.endsWith(".vue"))
    .sort((a, b) => (a.name < b.name ? -1 : 1));
  if (files.length > 0) return join(dir, files[0].name);
  for (const d of dirs) {
    if (d.name.startsWith(".")) continue;
    result = firstVue(join(dir, d.name));
    if (result) return result;
  }
  return result;
}

/** The sibling type module the active SFC imports (`./types`). The warm edit target. */
function siblingTypeModule(sfcPath: string): string {
  return join(sfcPath, "..", "types.ts");
}

interface PublishedDiag {
  readonly line: number;
  readonly character: number;
  readonly code: string;
  /** LSP `DiagnosticSeverity` mapped to the tsc vocabulary (`error`/`warning`/…). */
  readonly severity: string;
  readonly message: string;
}

/**
 * Map an LSP `DiagnosticSeverity` (1=Error, 2=Warning, 3=Information, 4=Hint) to
 * the SAME vocabulary `parseDiagnosticSet` captures from `verter-tsc` output
 * (`error`/`warning`/…), so the LSP correctness set and the tsc correctness set
 * are comparable. An omitted severity defaults to `error` (the LSP client default
 * for a diagnostic with no explicit severity, and what a `verter-tsc` diagnostic
 * almost always is) — never silently dropped.
 */
function normalizeDiagnosticSeverity(severity: unknown): string {
  switch (severity) {
    case 2:
      return "warning";
    case 3:
      return "information";
    case 4:
      return "hint";
    default:
      return "error";
  }
}

/**
 * Buffers every `textDocument/publishDiagnostics` so a workload can read both
 * the distinct affected-URI set after an edit and the full diagnostic SET.
 */
export class DiagnosticsBus {
  private readonly byUri = new Map<string, PublishedDiag[]>();
  private readonly handler: (params: unknown) => void;

  constructor(
    private readonly client: DriverClient,
    private readonly rootDir: string,
  ) {
    this.handler = (params): void => {
      const p = params as { uri?: string; diagnostics?: unknown[] };
      if (!p?.uri) return;
      const diags = (p.diagnostics ?? []).map((d): PublishedDiag => {
        const dd = d as {
          range?: { start?: { line?: number; character?: number } };
          code?: unknown;
          severity?: unknown;
          message?: unknown;
        };
        return {
          line: dd.range?.start?.line ?? 0,
          character: dd.range?.start?.character ?? 0,
          code: String(dd.code ?? ""),
          severity: normalizeDiagnosticSeverity(dd.severity),
          message: String(dd.message ?? ""),
        };
      });
      this.byUri.set(p.uri, diags);
    };
    this.client.onNotification("textDocument/publishDiagnostics", this.handler);
  }

  reset(): void {
    this.byUri.clear();
  }

  /** Distinct document URIs seen since the last `reset()`. */
  affectedUriCount(): number {
    return this.byUri.size;
  }

  /**
   * A normalized, sorted diagnostic SET (corpus-relative) for cross-side
   * correctness equality. The key carries the FULL diagnostic identity —
   * `path:line:char:code:severity:message` — the SAME shape `parseDiagnosticSet`
   * emits from `verter-tsc` output: two diagnostics that agree on path/line/char/
   * code but differ in SEVERITY (an error↔warning flip) or in the diagnostic TEXT
   * are DISTINCT, so a candidate that changes the actual error reported at a site
   * cannot pass diagnostic-SET equality. The path + message reuse
   * `normalizeDiagPath`/`normalizeDiagMessage`, so a per-side carrier hash / temp
   * dir collapses and the equality stays logical, not byte-physical.
   */
  diagnosticSet(): string[] {
    const out: string[] = [];
    for (const [uri, diags] of this.byUri) {
      const rel = normalizeDiagPath(this.relUri(uri), this.rootDir);
      for (const d of diags) {
        out.push(
          `${rel}:${d.line}:${d.character}:${d.code}:${d.severity}:${normalizeDiagMessage(d.message, this.rootDir)}`,
        );
      }
    }
    out.sort();
    return out;
  }

  /**
   * The CURRENTLY-settled diagnostic fingerprints (`code:line:char:message`) for
   * one document — the pre-edit baseline a measured edit's transition is compared
   * against (an unversioned publish equal to this is a stale republish, not the
   * edit's effect).
   */
  fingerprintsFor(uri: string): string[] {
    const diags = this.byUri.get(uri) ?? [];
    return diags.map((d) => fingerprintOf(d.code, d.line, d.character, d.message));
  }

  private relUri(uri: string): string {
    return relativizeUri(uri, this.rootDir);
  }

  dispose(): void {
    this.client.offNotification("textDocument/publishDiagnostics", this.handler);
  }
}

async function spawnReadyClient(
  binPath: string,
  corpus: EnsuredCorpus,
  label: string,
): Promise<{ client: DriverClient; rootUri: string }> {
  const args = [corpus.dir, "--type-provider=tsgo"];
  const client = new LspClient(label, binPath, args, undefined, clientOptions(label));
  const rootUri = pathToFileURL(corpus.dir).toString();
  await client.initialize(
    {
      processId: process.pid,
      capabilities: {
        textDocument: {
          publishDiagnostics: { relatedInformation: true },
          hover: { contentFormat: ["markdown", "plaintext"] },
          completion: { completionItem: { snippetSupport: false } },
        },
        workspace: { workspaceFolders: true },
      },
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: "perf" }],
    },
    SHORT_TIMEOUT,
  );
  // Begin capturing the type-provider status BEFORE `initialized` (the server emits
  // it once during `initialized`, ahead of `$/verter/ready`), so it is not missed.
  const providerStatus = captureTypeProviderStatus(client);
  client.sendNotification("initialized", {});
  // Readiness is REQUIRED on the default (CI) path — a missing $/verter/ready
  // would let the cold workspace scan leak into the warm/interactive samples.
  await waitForReady(client, PHASE_TIMEOUT, true);
  providerStatus.dispose();
  // A measured run REQUIRES the tsgo type engine — fail loud (with the server's
  // reason) rather than silently measure a verter-only/tsserver fallback that
  // exercises no tsgo type checking.
  assertTypeProviderTsgo(providerStatus.current(), true);
  return { client, rootUri };
}

/** The production connect: spawn a real `verter_lsp` client and await readiness. */
const defaultConnect: DriverConnect = (binPath, corpus, label) =>
  spawnReadyClient(binPath, corpus, label);

function waitForUriDiagnostics(client: DriverClient, uri: string, timeout: number): Promise<void> {
  return client
    .waitForNotification(
      "textDocument/publishDiagnostics",
      timeout,
      (params) => (params as { uri?: string } | null)?.uri === uri,
    )
    .then(() => undefined);
}

/**
 * Wait for a `publishDiagnostics` for `uri` whose diagnostics array satisfies
 * `predicate` (e.g. a NON-EMPTY settled set). Used to settle a real, non-empty
 * pre-edit baseline before the first measured dependency edit. Rejects on timeout
 * (a hard workload failure — never a fast pass on an empty/queued publish).
 */
function waitForUriDiagnosticsMatching(
  client: DriverClient,
  uri: string,
  predicate: (diagnostics: unknown[]) => boolean,
  timeout: number,
): Promise<void> {
  return client
    .waitForNotification("textDocument/publishDiagnostics", timeout, (params) => {
      const p = params as { uri?: string; diagnostics?: unknown[] } | null;
      if (p?.uri !== uri) return false;
      return predicate(Array.isArray(p.diagnostics) ? p.diagnostics : []);
    })
    .then(() => undefined);
}

/**
 * The TS diagnostic code the in-file single-file edit produces. Each measured edit
 * injects `const __perfTypeErrN: "perf-N" = 0`, a TS2322 ("Type '0' is not assignable
 * to type '\"perf-N\"'") whose message ECHOES the injected `perf-N` literal — the
 * unique per-edit fingerprint the wait binds to. The token MUST sit in TYPE position:
 * a string-literal VALUE (`= "perf-N"`) WIDENS to `string`, so its TS2322 message
 * ("Type 'string' is not assignable to type 'number'.") is token-free and could never
 * be matched; a string-literal TYPE annotation is preserved verbatim in the message.
 * Matched flexibly: an LSP `Diagnostic.code` may arrive as the number 2322, the string
 * "2322", or "TS2322".
 */
const EDIT_DIAGNOSTIC_CODE = "2322";

/**
 * The TS diagnostic code the warm cross-file edit produces. Each measured edit re-points
 * the dependent SFC's inline `import("./types").WarmDepProbeN` annotation at the NEXT
 * stable imported string-literal alias (`WarmDepProbeN = "perfDepN"` in the sibling type
 * module), so the warm re-typecheck re-resolves the cross-file import and errors TS2322
 * ("Type '0' is not assignable to type '\"perfDepN\"'"), whose message ECHOES that alias's
 * literal — the unique per-edit fingerprint binding the measured wait to THIS edit (not
 * merely "some diagnostic carrying the edit code"). The erroring TYPE lives in the imported
 * module (genuinely cross-file); the dependent's own buffer is what changes per edit, which
 * is what reliably re-publishes an open dependent on the persistent client.
 */
const DEPENDENCY_EDIT_DIAGNOSTIC_CODE = "2322";

/** Normalize an LSP diagnostic code to its bare TS number (`TS2322`/`2322`/2322 → `2322`). */
function normalizeDiagnosticCode(code: unknown): string {
  return String(code ?? "")
    .trim()
    .replace(/^TS/i, "");
}

/**
 * Normalize a diagnostic message for fingerprinting: trim + collapse internal
 * whitespace runs. Message differences MUST register (a real per-edit transition
 * has to be distinguishable from a stale republish of the same state), so this
 * deliberately does NOT lowercase or strip content.
 */
function normalizeTransitionMessage(msg: unknown): string {
  return String(msg ?? "")
    .trim()
    .replace(/\s+/g, " ");
}

/**
 * A run-stable fingerprint of one diagnostic: `code:line:char:message`.
 *
 * Severity is INTENTIONALLY excluded here (unlike `DiagnosticsBus.diagnosticSet`,
 * which carries it): this fingerprint detects a per-edit TRANSITION within ONE
 * document on ONE side (pre-edit settled set vs post-edit publish) — the measured
 * edit toggles a specific error diagnostic present↔absent, and severity does not
 * independently vary at a fixed code+location+message across that pre/post pair,
 * so adding it would not sharpen transition detection. The cross-SIDE correctness
 * equality — where a severity flip at the same site must register — is owned by
 * `diagnosticSet()`, which does carry severity.
 */
function fingerprintOf(code: unknown, line: number, character: number, message: unknown): string {
  return `${normalizeDiagnosticCode(code)}:${line}:${character}:${normalizeTransitionMessage(message)}`;
}

/** Fingerprint a raw LSP `Diagnostic` from a `publishDiagnostics` payload. */
function rawDiagnosticFingerprint(d: unknown): string {
  const dd = d as {
    range?: { start?: { line?: number; character?: number } };
    code?: unknown;
    message?: unknown;
  };
  return fingerprintOf(
    dd.code,
    dd.range?.start?.line ?? 0,
    dd.range?.start?.character ?? 0,
    dd.message,
  );
}

/** Order-independent equality of two diagnostic fingerprint sets. */
function fingerprintSetsEqual(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const sa = [...a].sort();
  const sb = [...b].sort();
  return sa.every((v, i) => v === sb[i]);
}

/**
 * The unique diagnostic a measured edit deterministically toggles. Matched on the
 * bare TS code and, when the harness can predict it, a unique message substring
 * (e.g. the per-iteration `perf-3` literal the single-file edit injects, which the
 * resulting TS2322 message echoes) — so the wait binds to THIS edit's specific
 * diagnostic, not merely "some diagnostic carrying this code".
 */
export interface EditFingerprint {
  readonly code: string;
  readonly messageIncludes?: string;
}

/** Whether a raw LSP diagnostic matches an {@link EditFingerprint}. */
function matchesEditFingerprint(d: unknown, fp: EditFingerprint): boolean {
  const dd = d as { code?: unknown; message?: unknown };
  if (normalizeDiagnosticCode(dd.code) !== fp.code) return false;
  if (fp.messageIncludes == null) return true;
  return normalizeTransitionMessage(dd.message).includes(fp.messageIncludes);
}

export interface DiagnosticTransition {
  /** The diagnostic code this transition concerns (the same code carried by `editFingerprint.code`). */
  readonly code: string;
  /** true ⇒ the edit ADDS the fingerprint (present after); false ⇒ REMOVES it. */
  readonly expectPresent: boolean;
  /**
   * Bind a VERSIONED publish to the edit: reject any publish whose echoed document
   * version is below this floor (a stale, pre-edit publish for an earlier version
   * cannot satisfy). Only the active edited document advances a version — omit it
   * for a DEPENDENT document the edit does not itself version. verter_lsp currently
   * publishes diagnostics with version `None`, so this is an ADDITIVE guard; the
   * unversioned envelope below is the real binding for those.
   */
  readonly minVersion?: number;
  /**
   * The diagnostic fingerprints settled for this document BEFORE the edit. A publish
   * whose fingerprint set EQUALS this is a stale/no-op republish of the pre-edit state
   * and does NOT satisfy (no genuine transition). REQUIRED on every measured transition.
   */
  readonly preEditFingerprints: readonly string[];
  /**
   * The unique diagnostic this edit deterministically toggles. A satisfying publish
   * must CARRY it (when the edit ADDS) or LACK it (when the edit REMOVES). REQUIRED on
   * every measured transition.
   */
  readonly editFingerprint: EditFingerprint;
}

/**
 * Whether a `publishDiagnostics` payload satisfies the expected per-edit transition.
 * Every measured transition MUST carry the full envelope (preEditFingerprints + the
 * unique editFingerprint); a transition lacking it THROWS (a code-only/version-only
 * match is not a sound per-edit binding). The binding rails then compose:
 *  - VERSION FLOOR (necessary on a VERSIONED publish): reject any publish whose
 *    echoed `version < minVersion` (a stale, pre-edit publish for an earlier version
 *    cannot satisfy). Vacuous on an unversioned publish (verter_lsp publishes
 *    `version: None`).
 *  - TRANSITION ENVELOPE: a new document version does NOT prove the diagnostics
 *    actually transitioned, because the toggled fingerprint could already have been
 *    present/absent before the edit. So the published fingerprint set must DIFFER from
 *    the pre-edit settled set (a publish equal to the pre-edit state is a stale
 *    republish) AND the unique edit fingerprint must be on the EXPECTED side of the
 *    toggle (present when added, absent when removed).
 */
export function publishMatchesTransition(
  params: unknown,
  uri: string,
  t: DiagnosticTransition,
): boolean {
  const p = params as { uri?: string; version?: number; diagnostics?: unknown[] } | null;
  if (!p || p.uri !== uri) return false;

  // A measured transition MUST carry the full envelope (preEditFingerprints + the
  // unique editFingerprint). There is NO code-only / version-only acceptance path: a
  // version bump or a coarse code presence does not prove the diagnostics actually
  // transitioned for THIS edit, so a transition lacking the envelope is a harness
  // defect, not a satisfiable publish.
  if (t.preEditFingerprints == null || t.editFingerprint == null) {
    throw new Error(
      "publishMatchesTransition: a measured diagnostic transition requires the full envelope " +
        "(preEditFingerprints + a unique editFingerprint); a code-only/version-only match is not " +
        "a sound per-edit binding",
    );
  }

  const diags = Array.isArray(p.diagnostics) ? p.diagnostics : [];

  // Version floor — a NECESSARY (never sufficient) condition on a versioned publish.
  if (typeof p.version === "number" && t.minVersion != null && p.version < t.minVersion) {
    return false;
  }

  // Transition envelope: a genuine set transition (differs from the pre-edit settled
  // set) AND the unique edit fingerprint on the expected side; a republish of the
  // unchanged pre-edit state does NOT satisfy.
  const fps = diags.map(rawDiagnosticFingerprint);
  if (fingerprintSetsEqual(fps, t.preEditFingerprints)) return false; // stale republish
  const present = diags.some((d) => matchesEditFingerprint(d, t.editFingerprint));
  return present === t.expectPresent;
}

/**
 * Wait for a `publishDiagnostics` for `uri` that satisfies the expected transition
 * (the specific toggled code present/absent, optionally version-bound). Because the
 * measured edit ALTERNATES the error state, a stale / no-op republish (the opposite
 * or unchanged state, an unrelated diagnostic code, or an earlier document version)
 * does NOT satisfy — so the measured latency reflects the edit's actual effect, not
 * the initial / position-adjusted / queued republish. Rejects on timeout (a hard
 * workload failure).
 */
function waitForDiagnosticTransition(
  client: DriverClient,
  uri: string,
  transition: DiagnosticTransition,
  timeout: number,
): Promise<void> {
  return client
    .waitForNotification("textDocument/publishDiagnostics", timeout, (params) =>
      publishMatchesTransition(params, uri, transition),
    )
    .then(() => undefined);
}

/**
 * Wait for `$/verter/ready`. On the default (CI) path readiness is REQUIRED: a
 * missing ready means the cold workspace scan could leak into the warm /
 * interactive samples, so it is a HARD failure (surfacing as a workload failure
 * on a full run). `required: false` is an explicit, separately-labeled fallback
 * mode that CI does not use.
 */
export async function waitForReady(
  client: DriverClient,
  timeout: number,
  required: boolean,
): Promise<void> {
  try {
    await client.waitForNotification("$/verter/ready", timeout);
  } catch {
    if (required) {
      throw new Error(
        "verter_lsp did not emit $/verter/ready within the readiness window — refusing to " +
          "measure interactive/warm latency that may include the cold workspace scan",
      );
    }
  }
}

/** A live capture of the server's `$/verter/typeProviderStatus` notification. */
export interface TypeProviderStatusCapture {
  /** The most recently-received status payload, or `undefined` if none arrived. */
  current(): unknown;
  /** Detach the capture handler from the client. */
  dispose(): void;
}

/**
 * Begin capturing the server's `$/verter/typeProviderStatus` notification. The
 * server emits it ONCE during `initialized` (before `$/verter/ready` — it is sent
 * synchronously, while `ready` follows the async workspace scan), so the handler
 * MUST be registered before `initialized` is sent or the notification is missed
 * (it is not replayed). The captured payload is read after readiness and handed to
 * {@link assertTypeProviderTsgo}.
 */
export function captureTypeProviderStatus(client: DriverClient): TypeProviderStatusCapture {
  let latest: unknown;
  const handler = (params: unknown): void => {
    latest = params;
  };
  client.onNotification("$/verter/typeProviderStatus", handler);
  return {
    current: () => latest,
    dispose: () => client.offNotification("$/verter/typeProviderStatus", handler),
  };
}

/**
 * Assert the server's active type provider is **tsgo** before a MEASURED run. A
 * measured LSP workload only exercises the TypeScript type engine when tsgo is
 * active; a server that fell back to verter-only mode (`kind: "none"`) — or to
 * tsserver — publishes no tsgo diagnostics / empty hovers, which the harness would
 * otherwise read as a fast (degenerate) sample and SKIP rather than fail. So on the
 * required (CI/measured) path a non-`tsgo` status (or a missing one) is a HARD
 * failure, surfacing the server-provided `reason` when present.
 *
 * `required: false` is the explicit smoke/self-check fallback (mirrors
 * {@link waitForReady}'s `required` parameter): it tolerates a non-tsgo provider so
 * a self-check on a box without the pinned tsgo can run without a hard failure.
 */
export function assertTypeProviderTsgo(status: unknown, required: boolean): void {
  const kind =
    status !== null &&
    typeof status === "object" &&
    typeof (status as { kind?: unknown }).kind === "string"
      ? (status as { kind: string }).kind
      : undefined;
  if (kind === "tsgo") return;
  if (!required) return;
  const reason =
    status !== null &&
    typeof status === "object" &&
    typeof (status as { reason?: unknown }).reason === "string"
      ? (status as { reason: string }).reason
      : undefined;
  const which = kind ?? "(no $/verter/typeProviderStatus received)";
  throw new Error(
    `verter_lsp active type provider is '${which}', not 'tsgo'` +
      (reason ? ` — reason: ${reason}` : "") +
      " — refusing to measure LSP workloads without the tsgo type engine (a verter-only / tsserver " +
      "fallback exercises no tsgo type checking and would silently skip the measurement).",
  );
}

export interface EditLatencySample {
  /** Per-edit latency distribution (ms) — edit → updated diagnostics. */
  readonly latencies: number[];
  /** Max distinct document URIs that re-published after a SINGLE-file edit. */
  readonly affectedUrisMax: number;
  /** Total project SFCs (the locality-fraction denominator). */
  readonly totalUris: number;
  /** The settled diagnostic SET (corpus-relative, sorted) for correctness. */
  readonly diagnosticSet: string[];
}

/** Workload 3: many in-file edits on the active SFC, per-edit latency distribution. */
export async function editToDiagnosticsLatency(
  binPath: string,
  corpus: EnsuredCorpus,
  opts: DriverOptions,
  connect: DriverConnect = defaultConnect,
): Promise<EditLatencySample> {
  const file = activeSfc(corpus);
  const uri = pathToFileURL(file).toString();
  const original = readFileSync(file, "utf-8");
  // Inject a typed error before the SFC's `</script>`: `const <name>: "<token>" = 0`
  // is a TS2322 whose message ("Type '0' is not assignable to type '\"<token>\"'.")
  // ECHOES <token> — the unique per-edit fingerprint. The token MUST sit in TYPE
  // position: a string-literal VALUE (`= "<token>"`) widens to `string`, so its message
  // is token-free and the wait could never match it; a string-literal TYPE annotation is
  // preserved verbatim. The injection MUST actually rewrite the SFC (a no-op edit would
  // silently measure nothing).
  const injectTypeError = (name: string, token: string): string =>
    original.replace(/<\/script>/, `const ${name}: "${token}" = 0;\n</script>`);
  if (injectTypeError("__perfProbe", "probe") === original) {
    throw new Error(
      `in-file edit-latency no-op'd on ${file} — corpus shape drift ` +
        "(expected a `</script>` to inject a typed error before)",
    );
  }
  const { client } = await connect(binPath, corpus, "edit-latency");
  const bus = new DiagnosticsBus(client, corpus.dir);
  const latencies: number[] = [];
  let affectedUrisMax = 0;
  try {
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri, languageId: "vue", version: 1, text: original },
    });
    // Wait for the active SFC's INITIAL diagnostics to settle, then PRIME a NON-EMPTY
    // pre-edit baseline (a base-token error) before the first measured edit — mirroring
    // the warm-dependency path — so even the first measured edit transitions from a
    // known non-empty settled set and a queued/empty initial publish can never be timed
    // as an edit's effect.
    await waitForUriDiagnostics(client, uri, SHORT_TIMEOUT);
    client.sendNotification("textDocument/didChange", {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: injectTypeError("__perfTypeErrBase", "perf-base") }],
    });
    await waitForUriDiagnosticsMatching(client, uri, (diags) => diags.length > 0, SHORT_TIMEOUT);
    if (bus.fingerprintsFor(uri).length === 0) {
      throw new Error(
        "in-file edit-latency: the active SFC did not settle a NON-EMPTY pre-edit diagnostic " +
          "baseline before the first measured edit",
      );
    }

    for (let i = 0; i < opts.ops; i++) {
      // EVERY measured edit injects a DISTINCT typed error carrying a per-iteration
      // UNIQUE token (`perf-${i}`); the resulting TS2322 message echoes it. There is NO
      // clear-to-empty half-iteration — every iteration is a present-with-unique-token
      // transition (expectPresent always true), so a stale/queued/empty/opposite publish
      // lacking THIS edit's token cannot satisfy it and times out. The unversioned
      // envelope binds on the pre-edit-set DIFFERENCE + this edit's unique token even
      // though verter_lsp publishes version: null.
      const token = `perf-${i}`;
      const editVersion = i + 3; // +1 didOpen, +1 prime, then the measured edits
      const edited = injectTypeError(`__perfTypeErr${i}`, token);
      const preEditFingerprints = bus.fingerprintsFor(uri);
      bus.reset();
      const t0 = performance.now();
      client.sendNotification("textDocument/didChange", {
        textDocument: { uri, version: editVersion },
        contentChanges: [{ text: edited }],
      });
      // NO .catch: a missing/stale/empty transition is a hard workload failure, never a
      // pass. The version floor binds versioned publishes; the unversioned envelope binds
      // to the pre-edit set + this edit's unique token.
      await waitForDiagnosticTransition(
        client,
        uri,
        {
          code: EDIT_DIAGNOSTIC_CODE,
          expectPresent: true,
          minVersion: editVersion,
          preEditFingerprints,
          editFingerprint: { code: EDIT_DIAGNOSTIC_CODE, messageIncludes: token },
        },
        SHORT_TIMEOUT,
      );
      latencies.push(performance.now() - t0);
      // Let the affected-set burst settle, then measure locality.
      await sleep(SETTLE_MS);
      affectedUrisMax = Math.max(affectedUrisMax, bus.affectedUriCount());
    }

    return {
      latencies,
      affectedUrisMax,
      totalUris: countVue(corpus.dir),
      diagnosticSet: bus.diagnosticSet(),
    };
  } finally {
    bus.dispose();
    await client.kill();
  }
}

export interface IdeQuerySample {
  readonly hoverLatencies: number[];
  readonly completionLatencies: number[];
  readonly hoverHits: number;
  /**
   * The WORST (minimum) per-operation valid-completion-item count over the run —
   * NOT just the last op's. Reducing to the min makes an EARLIER degraded
   * completion visible to the candidate/baseline `completion_item_parity` gate (a
   * single op that returns fewer items pulls the gated scalar down).
   */
  readonly completionItems: number;
  /** The per-operation valid-completion-item counts, in order (for the artifact). */
  readonly completionItemCounts: number[];
  /**
   * The NORMALIZED hover CONTENT at each deterministic probed position (NOT a
   * count) — `posIdx:content`, so a candidate whose hover TEXT diverges from the
   * baseline at a probed position (even at an identical hit count) is caught by the
   * gate's content-equality rail. An empty/whitespace hover already throws upstream
   * (`assertHoverResult`), so every captured entry carries real content.
   */
  readonly hoverContents: string[];
  /**
   * The NORMALIZED completion label SET across the deterministic probed positions
   * (`posIdx:label`, sorted) — content, not a count. A label-set divergence at a
   * probed position is a correctness regression even when the item COUNT matches.
   */
  readonly completionLabelSet: string[];
  readonly diagnosticSet: string[];
}

/** Workload 4: many hovers + many completions, as SEPARATE distributions. */
export async function ideQueryLatency(
  binPath: string,
  corpus: EnsuredCorpus,
  opts: DriverOptions,
  connect: DriverConnect = defaultConnect,
): Promise<IdeQuerySample> {
  const file = activeSfc(corpus);
  const uri = pathToFileURL(file).toString();
  const text = readFileSync(file, "utf-8");
  const { client } = await connect(binPath, corpus, "ide-query");
  const bus = new DiagnosticsBus(client, corpus.dir);
  const hoverLatencies: number[] = [];
  const completionLatencies: number[] = [];
  const completionItemCounts: number[] = [];
  let hoverHits = 0;
  const hoverContents: string[] = [];
  const completionLabelSet = new Set<string>();
  try {
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri, languageId: "vue", version: 1, text },
    });
    await waitForUriDiagnostics(client, uri, SHORT_TIMEOUT).catch(() => undefined);

    const positions = deterministicQueryPositions(text);
    const primary = positions[0];
    for (let i = 0; i < opts.ops; i++) {
      // No .catch in the MEASURED loop: a request error/timeout is a hard
      // workload failure, and a no-result hover/completion (a broken no-op LSP
      // answering fast) must NOT read as a fast passing sample.
      const tH = performance.now();
      const hover = await client.sendRequest<{ contents?: unknown } | null>(
        "textDocument/hover",
        { textDocument: { uri }, position: primary },
        SHORT_TIMEOUT,
      );
      hoverLatencies.push(performance.now() - tH);
      assertHoverResult(hover, "ide-query hover");
      hoverHits++;

      const tC = performance.now();
      const completion = await client.sendRequest<{ items?: unknown[] } | unknown[] | null>(
        "textDocument/completion",
        {
          textDocument: { uri },
          position: { line: primary.line, character: primary.character + 1 },
        },
        SHORT_TIMEOUT,
      );
      completionLatencies.push(performance.now() - tC);
      // Record EVERY op's valid-item count (not just the last), so an earlier
      // degraded/empty completion is visible to the metric below.
      completionItemCounts.push(completionItemCount(completion, "ide-query completion"));
    }

    // CONTENT-correctness pass: probe EACH deterministic position ONCE (untimed),
    // capturing the NORMALIZED hover text + completion label set so a candidate
    // whose query CONTENT diverges from the baseline (even at identical counts) is
    // caught by the gate's content-equality rail — counts alone cannot catch a
    // bogus-but-same-count answer. A no-result query still throws (no fast pass).
    for (let pi = 0; pi < positions.length; pi++) {
      const pos = positions[pi];
      const hover = await client.sendRequest<{ contents?: unknown } | null>(
        "textDocument/hover",
        { textDocument: { uri }, position: pos },
        SHORT_TIMEOUT,
      );
      assertHoverResult(hover, `ide-query hover content@${pi}`);
      const contents = (hover as { contents?: unknown } | null)?.contents;
      hoverContents.push(`${pi}:${normalizeQueryText(hoverContentText(contents), corpus.dir)}`);

      const completion = await client.sendRequest<{ items?: unknown[] } | unknown[] | null>(
        "textDocument/completion",
        { textDocument: { uri }, position: { line: pos.line, character: pos.character + 1 } },
        SHORT_TIMEOUT,
      );
      // A no-result completion is a hard failure (mirrors the measured loop).
      completionItemCount(completion, `ide-query completion content@${pi}`);
      for (const label of completionLabelTexts(completion)) {
        completionLabelSet.add(`${pi}:${normalizeQueryText(label, corpus.dir)}`);
      }
    }

    return {
      hoverLatencies,
      completionLatencies,
      hoverHits,
      // The WORST op's count — a single degraded op pulls the gated parity scalar
      // down (an empty completion already throws above, a hard failure).
      completionItems: completionItemCounts.length > 0 ? Math.min(...completionItemCounts) : 0,
      completionItemCounts,
      hoverContents,
      completionLabelSet: [...completionLabelSet].sort(),
      diagnosticSet: bus.diagnosticSet(),
    };
  } finally {
    bus.dispose();
    await client.kill();
  }
}

export interface WarmLspSample {
  /** Per-edit latency distribution (ms): dependency edit → dependent diagnostics. */
  readonly latencies: number[];
  readonly affectedUrisMax: number;
  readonly totalUris: number;
  readonly diagnosticSet: string[];
}

/**
 * The genuinely-warm signal: ONE persistent client retains the Program across many
 * edits while the dependent SFC is re-typechecked against an imported type module.
 *
 * verter_lsp reliably re-publishes an OPEN dependent when the dependent's OWN buffer
 * changes, but editing an imported module's overlay repeatedly does not re-invalidate
 * the open dependent (its cross-file resolution stays pinned after the first edit). So
 * the imported module is opened ONCE with a STABLE set of per-iteration string-literal
 * aliases (`WarmDepProbeN = "perfDepN"`), and each measured edit re-points the dependent's
 * inline `import("./types").WarmDepProbeN` annotation at the NEXT alias — forcing a warm
 * CROSS-FILE re-resolution whose resulting TS2322 ("Type '0' is not assignable to type
 * '\"perfDepN\"'.") ECHOES that alias's UNIQUE literal token. The erroring TYPE lives in
 * the imported module (genuinely cross-file), the dependent's own edit reliably re-publishes
 * it, and the Program is reused across edits (unlike the cold verter-tsc rerun).
 */
export async function warmDependencyEditLatency(
  binPath: string,
  corpus: EnsuredCorpus,
  opts: DriverOptions,
  connect: DriverConnect = defaultConnect,
): Promise<WarmLspSample> {
  const sfc = activeSfc(corpus);
  const sfcUri = pathToFileURL(sfc).toString();
  const typeModule = siblingTypeModule(sfc);
  const typeUri = pathToFileURL(typeModule).toString();
  const sfcText = readFileSync(sfc, "utf-8");
  const typeText = readFileSync(typeModule, "utf-8");
  // The per-iteration cross-file token (`perfDepN`) and the stable alias that carries it
  // (`WarmDepProbeN`); the `"Base"` key is the at-open annotation the first measured edit
  // transitions away from.
  const depToken = (k: number | string): string => `perfDep${k}`;
  const aliasName = (k: number | string): string => `WarmDepProbe${k}`;
  const keys: Array<number | string> = ["Base", ...Array.from({ length: opts.ops }, (_, i) => i)];
  // The imported module, opened ONCE with all stable aliases (never re-edited, so its
  // cross-file resolution never goes stale under the persistent client).
  const aliasDecls = keys.map((k) => `export type ${aliasName(k)} = "${depToken(k)}";`).join("\n");
  const typesWithAliases = `${typeText}\n${aliasDecls}\n`;
  // Re-point the dependent's annotation at one imported alias via an inline import-type, so
  // the SFC's existing imports are untouched (corpus-shape-robust). The token sits in TYPE
  // position (a string-literal VALUE would widen to `string` and lose the token).
  const annotateAgainst = (k: number | string): string =>
    sfcText.replace(
      /<\/script>/,
      `const __warmDep: import("./types").${aliasName(k)} = 0;\n</script>`,
    );
  // Corpus-shape guard: the annotation edit MUST actually rewrite the dependent (a no-op
  // edit would silently measure nothing).
  if (annotateAgainst("Base") === sfcText) {
    throw new Error(
      `warm-dependency edit no-op'd on ${sfc} — corpus shape drift ` +
        "(expected a `</script>` to inject a cross-file-typed annotation before)",
    );
  }
  const { client } = await connect(binPath, corpus, "warm-lsp");
  const bus = new DiagnosticsBus(client, corpus.dir);
  const latencies: number[] = [];
  let affectedUrisMax = 0;
  try {
    // Open the imported type module (with the stable aliases) and the dependent SFC
    // annotated against the BASE alias, then settle a NON-EMPTY pre-edit baseline. A
    // queued/empty initial publish can then never be timed as an edit's effect (the
    // unversioned envelope binds on the pre-edit-set DIFFERENCE + the per-edit unique token).
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri: typeUri, languageId: "typescript", version: 1, text: typesWithAliases },
    });
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri: sfcUri, languageId: "vue", version: 1, text: annotateAgainst("Base") },
    });
    // REQUIRE a NON-EMPTY settled dependent baseline. A dependent that never diagnoses is
    // broken instrumentation — a hard failure, never a fast pass on an empty/queued baseline.
    await waitForUriDiagnosticsMatching(client, sfcUri, (diags) => diags.length > 0, SHORT_TIMEOUT);
    if (bus.fingerprintsFor(sfcUri).length === 0) {
      throw new Error(
        "warm-dependency: the dependent SFC did not settle a NON-EMPTY pre-edit diagnostic " +
          "baseline before the first measured edit",
      );
    }

    for (let i = 0; i < opts.ops; i++) {
      // Re-point the dependent at THIS iteration's stable imported alias; the warm
      // cross-file re-resolution re-errors TS2322 echoing the alias's unique token. The
      // wait binds on the unversioned envelope: the dependent's NON-EMPTY pre-edit
      // fingerprints (the prior iteration's / base error) + this edit's unique token. A
      // stale republish equal to the pre-edit set, a queued/empty publish, or one lacking
      // THIS edit's token does NOT satisfy.
      const token = depToken(i);
      const edited = annotateAgainst(i);
      const preEditFingerprints = bus.fingerprintsFor(sfcUri);
      bus.reset();
      const t0 = performance.now();
      client.sendNotification("textDocument/didChange", {
        textDocument: { uri: sfcUri, version: i + 2 },
        contentChanges: [{ text: edited }],
      });
      // Measure to the DEPENDENT SFC re-diagnosing with the EXPECTED transition (the warm
      // cross-file recheck). NO .catch: a missing/stale republish is a hard workload
      // failure, not a fast passing sample.
      await waitForDiagnosticTransition(
        client,
        sfcUri,
        {
          code: DEPENDENCY_EDIT_DIAGNOSTIC_CODE,
          expectPresent: true,
          preEditFingerprints,
          editFingerprint: { code: DEPENDENCY_EDIT_DIAGNOSTIC_CODE, messageIncludes: token },
        },
        SHORT_TIMEOUT,
      );
      latencies.push(performance.now() - t0);
      await sleep(SETTLE_MS);
      affectedUrisMax = Math.max(affectedUrisMax, bus.affectedUriCount());
    }

    return {
      latencies,
      affectedUrisMax,
      totalUris: countVue(corpus.dir),
      diagnosticSet: bus.diagnosticSet(),
    };
  } finally {
    bus.dispose();
    await client.kill();
  }
}

// ── helpers ─────────────────────────────────────────────────────────────────
function countVue(dir: string): number {
  let n = 0;
  const walk = (d: string): void => {
    let entries: import("node:fs").Dirent<string>[];
    try {
      entries = readdirSync(d, { withFileTypes: true, encoding: "utf-8" });
    } catch {
      return;
    }
    for (const e of entries) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      if (e.isDirectory()) walk(join(d, e.name));
      else if (e.name.endsWith(".vue")) n++;
    }
  };
  walk(dir);
  return n;
}

/** Find a 0-based {line, character} for the first occurrence of `ident`. */
function findIdentifierPosition(text: string, ident: string): { line: number; character: number } {
  const lines = text.split("\n");
  for (let line = 0; line < lines.length; line++) {
    const ch = lines[line].indexOf(ident);
    if (ch >= 0) return { line, character: ch };
  }
  return { line: 0, character: 0 };
}
