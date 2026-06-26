// The DX corpus SWEEP runner.
//
// Unlike the per-collector unit/integration tests (which ASSERT a fixed verter-vs-
// baseline outcome), this is a RUN-and-RECORD sweep: it drives the REAL `verter-lsp`
// over every committed hermetic corpus scenario on a properly MATERIALIZED workspace
// (real tsconfig + vendored Vue shims), checks each DX signal against the scenario's
// authored Vue-surface invariants AND against the tsgo gold standard, and RECORDS
// every divergence as a finding — it never bakes in an expected pass. The folded
// finding set is written to DX-FINDINGS.md / dx-summary.json.
//
// Gated on DX_LSP_BIN + DX_BASELINE_BIN (so default `pnpm test:unit` stays hermetic):
//   cargo build -p verter_lsp          # target/debug/verter-lsp[.exe]
//   cargo build -p verter_dx_baseline  # target/debug/verter-dx-baseline[.exe]
//   DX_LSP_BIN=$PWD/target/debug/verter-lsp.exe \
//   DX_BASELINE_BIN=$PWD/target/debug/verter-dx-baseline.exe \
//   DX_FINDINGS_OUT="${TMPDIR:-/tmp}/dx" \
//   (tsgo on PATH) pnpm -C packages/dx-harness exec vitest --run test/dxCorpusSweep.run.test.ts
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";
import { describe, expect, it } from "vitest";

import {
  CollectingSink,
  EditBuffer,
  collectAutoImport,
  collectChurn,
  collectRecovery,
  collectorEvent,
  offsetToPosition,
  type CollectorEvent,
  type CollectorLspClient,
  type CollectorName,
  type CollectorSignal,
  type Severity,
} from "../src/collectors/index.js";
import { BridgeClient } from "../src/baseline/bridgeClient.js";
import { runMaterialize } from "../src/baseline/materializeClient.js";
import {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  type MaterializedWorkspace,
} from "../src/materializedWorkspace.js";
import {
  awaitRawLspStartup,
  GET_STATISTICS_METHOD,
  createWarnLineDrainer,
} from "../src/core/startupGate.js";
import { extractQuiescenceCounters, pollUntilQuiesced } from "../src/core/quiescence.js";
import {
  corpusFixturesDir,
  loadScenarioCorpus,
  type Scenario,
  type Probe,
} from "../src/scenario/index.js";
import {
  prepareOracleSource,
  runResolvedOracleQuery,
  type OracleVerterClient,
  type ResolvedOracleQuery,
} from "../src/semantic-oracle/index.js";
import type { DifferentialOutcome } from "../src/differential/index.js";
import {
  buildBaselineManifest,
  buildSummary,
  reduceFindings,
  renderFindingsMarkdown,
  serializeBaselineManifest,
  serializeSummary,
  writeBaselineManifest,
  writeFindingsMarkdown,
  writeSummary,
  type DxFinding,
  type EventObservation,
  type ScenarioIndex,
  type ScenarioMeta,
  type SituatedOutcome,
} from "../src/report/index.js";

const LSP_BIN = process.env.DX_LSP_BIN;
const BASELINE_BIN = process.env.DX_BASELINE_BIN;
const PROVIDER = process.env.DX_LSP_PROVIDER ?? "tsgo";
const OUT_DIR = process.env.DX_FINDINGS_OUT ?? join(process.cwd(), "dx-run");
const TSGO_BIN = process.env.DX_TSGO_BIN;
const REQUEST_TIMEOUT = 25_000;

// ── tiny LSP-response extractors (verter-side, source-space) ─────────────────────

function hoverText(result: unknown): string {
  if (result === null || typeof result !== "object") return "";
  const contents = (result as { contents?: unknown }).contents;
  if (contents === undefined || contents === null) return "";
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) {
    return contents
      .map((c) => (typeof c === "string" ? c : String((c as { value?: unknown }).value ?? "")))
      .join("\n");
  }
  if (typeof contents === "object" && "value" in contents)
    return String((contents as { value: unknown }).value);
  return "";
}

interface DefTarget {
  readonly uri: string;
  readonly line: number;
  readonly character: number;
}
function defTargets(result: unknown): DefTarget[] {
  if (result === null || result === undefined) return [];
  const arr = Array.isArray(result) ? result : [result];
  const out: DefTarget[] = [];
  for (const raw of arr) {
    if (raw === null || typeof raw !== "object") continue;
    const l = raw as Record<string, unknown>;
    const uri = (l.targetUri ?? l.uri) as string | undefined;
    const range = (l.targetSelectionRange ?? l.targetRange ?? l.range) as
      | { start?: { line?: number; character?: number } }
      | undefined;
    if (typeof uri === "string" && range?.start) {
      out.push({ uri, line: range.start.line ?? 0, character: range.start.character ?? 0 });
    }
  }
  return out;
}

function completionLabels(result: unknown): { labels: string[]; isIncomplete: boolean } {
  if (result === null || result === undefined) return { labels: [], isIncomplete: false };
  const items = Array.isArray(result) ? result : ((result as { items?: unknown[] }).items ?? []);
  const isIncomplete = Array.isArray(result)
    ? false
    : Boolean((result as { isIncomplete?: boolean }).isIncomplete);
  const labels = (items as Array<{ label?: unknown }>)
    .map((i) => (typeof i.label === "string" ? i.label : ""))
    .filter((s) => s.length > 0);
  return { labels, isIncomplete };
}

// ── position helpers ─────────────────────────────────────────────────────────────

const TS_KEYWORDS = new Set([
  "const",
  "let",
  "var",
  "function",
  "return",
  "void",
  "interface",
  "type",
  "import",
  "from",
  "as",
  "export",
  "declare",
  "new",
  "this",
  "true",
  "false",
  "null",
  "undefined",
]);

/**
 * Resolve a `@dx-anchor` position to the actual queryable TOKEN, mirroring the
 * oracle `.ts` side (`prepareOracleSource`, which moves to the last identifier on
 * the line). The committed fixtures anchor a SCRIPT line with a TRAILING comment
 * (`const x = props.title; // @dx-anchor props.title`), so the queryable token is
 * the last identifier BEFORE the anchor column; a TEMPLATE anchor leads its element
 * (`<!-- @dx-anchor evt.click --><button @click=...>`), so the token is the `@event`
 * binding (or first identifier) AFTER the anchor column. Without this, verter is
 * queried on the stripped-comment whitespace and returns null — a false divergence.
 */
function resolveAnchorToken(
  text: string,
  line: number,
  character: number,
): { line: number; character: number } {
  const lines = text.split(/\r?\n/);
  const lineText = lines[line] ?? "";
  const ID = /[A-Za-z_$][\w$]*/g;
  let last: RegExpExecArray | null = null;
  let m: RegExpExecArray | null;
  while ((m = ID.exec(lineText)) !== null) {
    if (m.index < character && !TS_KEYWORDS.has(m[0])) last = m;
  }
  if (last) return { line, character: last.index };
  const after = lineText.slice(character);
  const evt = /@([A-Za-z_][\w-]*)/.exec(after);
  if (evt) return { line, character: character + evt.index + 1 };
  const firstAfter = /[A-Za-z_$][\w$]*/.exec(after);
  if (firstAfter) return { line, character: character + firstAfter.index };
  return { line, character };
}

/** Convert an anchor's UTF-16 LSP position back to a JS-string byte (UTF-16) offset. */
function positionToOffset(text: string, line: number, character: number): number {
  let offset = 0;
  let curLine = 0;
  while (curLine < line) {
    const nl = text.indexOf("\n", offset);
    if (nl === -1) return text.length;
    offset = nl + 1;
    curLine++;
  }
  return Math.min(offset + character, text.length);
}

// ── verter driver bootstrap ──────────────────────────────────────────────────────

async function startVerter(root: string): Promise<LspClient> {
  const rootUri = pathToFileURL(root).toString();
  const client = new LspClient("verter-lsp", LSP_BIN!, [root, `--type-provider=${PROVIDER}`]);
  await client.initialize(
    {
      processId: process.pid,
      capabilities: { workspace: { workspaceFolders: true } },
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: "dx-sweep" }],
    },
    30_000,
  );
  const startup = awaitRawLspStartup(client, {
    readyTimeoutMs: 120_000,
    quiescence: { timeoutMs: 30_000 },
  });
  client.sendNotification("initialized", {});
  await startup;
  return client;
}

function quiescer(client: CollectorLspClient): () => Promise<boolean> {
  const drain = createWarnLineDrainer(client.stderr);
  return async () => {
    const result = await pollUntilQuiesced(
      async () =>
        extractQuiescenceCounters(await client.sendRequest(GET_STATISTICS_METHOD, {}, 10_000)),
      drain,
      { timeoutMs: 10_000 },
    );
    return result.quiesced;
  };
}

const fileUri = (root: string, rel: string): string => pathToFileURL(join(root, rel)).toString();

// ── event construction ───────────────────────────────────────────────────────────

interface MkEvent {
  scenario: string;
  probe: string;
  anchor: string;
  collector: CollectorName;
  signal: CollectorSignal;
  ok: boolean;
  severity: Severity;
  detail: string;
  driver?: "rawLsp" | "tsgo";
  data?: unknown;
}
function mkEvent(e: MkEvent): CollectorEvent {
  return collectorEvent({
    collector: e.collector,
    signal: e.signal,
    ok: e.ok,
    severity: e.severity,
    provenance: { detectedBy: e.driver ?? "rawLsp" },
    key: {
      scenario: e.scenario,
      editStepIndex: 0,
      driver: e.driver ?? "rawLsp",
      provider: PROVIDER,
      probe: e.probe,
      version: 1,
      anchor: e.anchor,
    },
    detail: e.detail,
    data: e.data,
  });
}

// ── per-scenario probe drivers ───────────────────────────────────────────────────

interface ScenarioRun {
  scenario: Scenario;
  ws: MaterializedWorkspace;
  verter: LspClient;
  bridge: BridgeClient | null;
  events: CollectorEvent[];
  outcomes: SituatedOutcome[];
  log: (msg: string) => void;
}

function anchorPos(
  ws: MaterializedWorkspace,
  name: string,
): { line: number; character: number } | null {
  const a = ws.anchorMap.get(name);
  return a ? { line: a.line, character: a.character } : null;
}

function openVue(verter: LspClient, uri: string, text: string): void {
  verter.sendNotification("textDocument/didOpen", {
    textDocument: { uri, languageId: "vue", version: 1, text },
  });
}

/** Strip the `<script setup>` body out of a `.vue` and rewrite Vue macros to plain TS. */
function scriptMirror(vueText: string): { ts: string; ok: boolean } {
  const m = vueText.match(/<script setup[^>]*>([\s\S]*?)<\/script>/);
  if (!m) return { ts: "", ok: false };
  let body = m[1].replace(/^\n/, "");
  // Rewrite the Vue compiler macros into self-contained TS so tsgo type-checks the
  // mirror standalone: `define*<T>()` → an opaque value of T; withDefaults unwraps.
  body = body.replace(
    /withDefaults\(\s*defineProps<([\s\S]*?)>\(\)\s*,[\s\S]*?\)/g,
    "(null as unknown as $1)",
  );
  body = body.replace(/define(?:Props|Emits)<([\s\S]*?)>\(\)/g, "(null as unknown as $1)");
  body = body.replace(
    /defineModel<([\s\S]*?)>\([^)]*\)/g,
    "({ value: null as unknown as $1 } as { value: $1 })",
  );
  return { ts: body, ok: true };
}

async function runHoverProbe(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
): Promise<void> {
  const raw = anchorPos(run.ws, probe.anchor);
  if (!raw) return;
  const pos = resolveAnchorToken(entryText, raw.line, raw.character);
  let result: unknown;
  try {
    result = await run.verter.sendRequest(
      "textDocument/hover",
      { textDocument: { uri: entryUri }, position: pos },
      REQUEST_TIMEOUT,
    );
  } catch (err) {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "hover",
        signal: "hover_invariant",
        ok: false,
        severity: "userVisible",
        detail: `verter hover request failed: ${String(err)}`,
      }),
    );
    return;
  }
  const text = hoverText(result);
  // Record the observed hover (informational).
  run.events.push(
    mkEvent({
      scenario: run.scenario.id,
      probe: probe.id,
      anchor: probe.anchor,
      collector: "hover",
      signal: text.length > 0 ? "hover_observed" : "hover_contentless_observed",
      ok: true,
      severity: "candidate",
      detail: text.slice(0, 200) || "(no hover content)",
    }),
  );
  // Check this anchor's authored Vue-surface invariants directly.
  for (const inv of run.scenario.invariants) {
    if (inv.anchor !== probe.anchor || inv.method !== "hover") continue;
    const present = text.includes(inv.value);
    const ok = inv.assertion === "excludes" ? !present : present;
    if (!ok) {
      run.events.push(
        mkEvent({
          scenario: run.scenario.id,
          probe: probe.id,
          anchor: probe.anchor,
          collector: "hover",
          signal: "hover_invariant",
          ok: false,
          severity: "userVisible",
          detail: `hover ${inv.assertion} "${inv.value}" violated (id=${inv.id ?? "?"})`,
          data: { verterValue: text.slice(0, 200), baselineValue: `${inv.assertion} ${inv.value}` },
        }),
      );
    }
  }
}

async function runDefinitionProbe(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
): Promise<void> {
  const raw = anchorPos(run.ws, probe.anchor);
  if (!raw) return;
  const pos = resolveAnchorToken(entryText, raw.line, raw.character);
  let result: unknown;
  try {
    result = await run.verter.sendRequest(
      "textDocument/definition",
      { textDocument: { uri: entryUri }, position: pos },
      REQUEST_TIMEOUT,
    );
  } catch (err) {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "definition",
        signal: "definition_parity",
        ok: false,
        severity: "userVisible",
        detail: `verter definition request failed: ${String(err)}`,
        driver: "tsgo",
        data: {
          class: "baselineOnly",
          verterValue: "request failed",
          baselineValue: "declaration",
        },
      }),
    );
    return;
  }
  const targets = defTargets(result);
  if (targets.length === 0) {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "definition",
        signal: "definition_parity",
        ok: false,
        severity: "userVisible",
        detail: "verter resolved no definition target (tsgo resolves the declaration)",
        driver: "tsgo",
        data: { class: "baselineOnly", verterValue: "no target", baselineValue: "declaration" },
      }),
    );
    return;
  }
  // Definition PRECISION: the headline DX failure is a line-0 fallback target (a
  // desynced source map). Flag any target whose line is 0 AND is not the genuine
  // first line of its own file.
  const lineZero = targets.filter((t) => t.line === 0);
  if (lineZero.length > 0) {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "definition",
        signal: "definition_parity",
        ok: false,
        severity: "userVisible",
        detail: `verter definition landed on line 0 fallback: ${lineZero.map((t) => t.uri).join(", ")}`,
        driver: "tsgo",
        data: {
          class: "rangeMismatch",
          verterValue: `line 0 @ ${lineZero[0].uri}`,
          baselineValue: "exact declaration line",
        },
      }),
    );
  } else {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "definition",
        signal: "definition_parity",
        ok: true,
        severity: "candidate",
        detail: `verter definition → ${targets[0].uri}:${targets[0].line + 1}:${targets[0].character + 1}`,
        driver: "tsgo",
      }),
    );
  }
}

async function runCompletionProbe(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
  requiredLabels: readonly string[],
): Promise<void> {
  // The headline DX signal: type `.` after a typed reference and the member set
  // must NOT collapse to "No Suggestions". The fixtures anchor the line with a
  // TRAILING comment, so insert the `.` AFTER the line's queryable receiver token
  // (the last identifier), not at the anchor column — otherwise the `.` lands after
  // the statement and yields a vacuous empty set (a false collapse). Re-open the doc
  // fresh per probe so a prior probe's `.` never leaks into this one's text.
  const raw = anchorPos(run.ws, probe.anchor);
  if (!raw) return;
  const tok = resolveAnchorToken(entryText, raw.line, raw.character);
  const lineText = entryText.split(/\r?\n/)[tok.line] ?? "";
  const idm = /[A-Za-z_$][\w$]*/.exec(lineText.slice(tok.character));
  const tokenEnd = tok.character + (idm ? idm[0].length : 0);
  const insertPos = { line: tok.line, character: tokenEnd };

  run.verter.sendNotification("textDocument/didClose", { textDocument: { uri: entryUri } });
  run.verter.sendNotification("textDocument/didOpen", {
    textDocument: { uri: entryUri, languageId: "vue", version: 1, text: entryText },
  });
  await new Promise((r) => setTimeout(r, 150));
  run.verter.sendNotification("textDocument/didChange", {
    textDocument: { uri: entryUri, version: 2 },
    contentChanges: [{ range: { start: insertPos, end: insertPos }, text: "." }],
  });
  await new Promise((r) => setTimeout(r, 300));

  let result: unknown;
  try {
    result = await run.verter.sendRequest(
      "textDocument/completion",
      {
        textDocument: { uri: entryUri },
        position: { line: tok.line, character: tokenEnd + 1 },
        context: { triggerKind: 2, triggerCharacter: "." },
      },
      REQUEST_TIMEOUT,
    );
  } catch (err) {
    run.events.push(
      mkEvent({
        scenario: run.scenario.id,
        probe: probe.id,
        anchor: probe.anchor,
        collector: "completion",
        signal: "no_suggestions_collapse",
        ok: false,
        severity: "userVisible",
        detail: `verter completion request failed: ${String(err)}`,
      }),
    );
    return;
  }
  const { labels } = completionLabels(result);
  // COLLAPSE detection (the robust, receiver-type-independent DX signal).
  run.events.push(
    mkEvent({
      scenario: run.scenario.id,
      probe: probe.id,
      anchor: probe.anchor,
      collector: "completion",
      signal: "no_suggestions_collapse",
      ok: labels.length > 0,
      severity: labels.length > 0 ? "candidate" : "userVisible",
      detail:
        labels.length > 0
          ? `member completion after \`.\` non-empty (${labels.length} items: ${labels.slice(0, 12).join(", ")})`
          : `member completion collapsed to NO suggestions after \`.\` (DX-collapse)`,
      data:
        labels.length > 0
          ? undefined
          : { verterValue: "(empty)", baselineValue: requiredLabels.join(", ") },
    }),
  );
  // Member-label correctness only when the receiver is the typed object the scenario
  // pins (its declared members appear) — never flag a different receiver's members.
  if (
    labels.length > 0 &&
    requiredLabels.length > 0 &&
    requiredLabels.some((l) => labels.includes(l))
  ) {
    for (const want of requiredLabels) {
      if (!labels.includes(want)) {
        run.events.push(
          mkEvent({
            scenario: run.scenario.id,
            probe: probe.id,
            anchor: probe.anchor,
            collector: "completion",
            signal: "completion_required_label",
            ok: false,
            severity: "userVisible",
            detail: `expected member "${want}" absent (got: ${labels.slice(0, 12).join(", ")})`,
            data: { verterValue: labels.slice(0, 20).join(", "), baselineValue: want },
          }),
        );
      }
    }
  }
}

interface PublishedDiag {
  message: string;
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  severity?: number;
}
async function runDiagnosticsProbes(
  run: ScenarioRun,
  probes: Probe[],
  entryUri: string,
  entryText: string,
): Promise<void> {
  const collected: PublishedDiag[] = [];
  const handler = (params: { uri?: string; diagnostics?: PublishedDiag[] }): void => {
    if (params.uri === entryUri && Array.isArray(params.diagnostics)) {
      collected.length = 0;
      collected.push(...params.diagnostics);
    }
  };
  run.verter.onNotification("textDocument/publishDiagnostics", handler);
  try {
    await quiescer(run.verter)();
    // A nudge edit to provoke a fresh diagnostics publish, then settle again.
    await new Promise((r) => setTimeout(r, 400));
  } finally {
    run.verter.offNotification("textDocument/publishDiagnostics", handler);
  }
  for (const probe of probes) {
    const pos = anchorPos(run.ws, probe.anchor);
    if (!pos) continue;
    const onLine = collected.filter(
      (d) => d.range.start.line <= pos.line && d.range.end.line >= pos.line,
    );
    for (const inv of run.scenario.invariants) {
      if (inv.anchor !== probe.anchor || inv.method !== "diagnostics") continue;
      if (inv.assertion === "contains") {
        // A genuine error MUST be reported and mention the token.
        const present = onLine.some((d) => d.message.includes(inv.value));
        run.events.push(
          mkEvent({
            scenario: run.scenario.id,
            probe: probe.id,
            anchor: probe.anchor,
            collector: "diagnostics",
            signal: "diagnostics_parity",
            ok: present,
            severity: present ? "candidate" : "userVisible",
            driver: "tsgo",
            detail: present
              ? `real error reported: ${onLine
                  .map((d) => d.message)
                  .join(" | ")
                  .slice(0, 160)}`
              : `expected a diagnostic containing "${inv.value}" but verter reported none on this line (tsgo flags it)`,
            data: present
              ? undefined
              : { class: "baselineOnly", verterValue: "(no diagnostic)", baselineValue: inv.value },
          }),
        );
      } else if (inv.assertion === "excludes") {
        // A valid construct must NOT be spuriously flagged.
        const spurious = onLine.filter((d) => d.message.includes(inv.value));
        const ok = spurious.length === 0;
        run.events.push(
          mkEvent({
            scenario: run.scenario.id,
            probe: probe.id,
            anchor: probe.anchor,
            collector: "diagnostics",
            signal: "diagnostics_parity",
            ok,
            severity: ok ? "candidate" : "userVisible",
            driver: "tsgo",
            detail: ok
              ? `no spurious "${inv.value}" diagnostic (correct)`
              : `SPURIOUS diagnostic mentioning "${inv.value}": ${spurious
                  .map((d) => d.message)
                  .join(" | ")
                  .slice(0, 160)}`,
            data: ok
              ? undefined
              : {
                  class: "verterOnly",
                  verterValue: spurious[0].message.slice(0, 160),
                  baselineValue: "(clean — tsgo reports nothing)",
                },
          }),
        );
      }
    }
  }
}

// ── the semantic-oracle baseline rail (verter `.vue` vs tsgo `.ts` gold standard) ──

async function runOracleScenario(
  run: ScenarioRun,
  entryUri: string,
  entryText: string,
): Promise<void> {
  if (run.bridge === null) return;
  // The committed gold standard mirrors the entry basename: `define-props.vue` ↔
  // `oracles/semantic/define-props.ts`. Anchor names are shared across the pair.
  const base = run.scenario.entryFile.replace(/\.vue$/, ".ts");
  const oracleTsPath = join(corpusOraclesDir(), base);
  let oracleSource: string;
  try {
    oracleSource = readFileSync(oracleTsPath, "utf-8");
  } catch {
    run.log(`  (oracle gold standard missing for ${run.scenario.entryFile}; skipping baseline)`);
    return;
  }
  const prepared = prepareOracleSource(oracleSource);
  const oracleRoot = run.ws.root; // the bridge is rooted here; write the mirror alongside.
  const oraclePath = join(oracleRoot, `__oracle__${base}`);
  writeFileSync(oraclePath, prepared.stripped, "utf-8");
  await run.bridge.open([{ path: oraclePath, content: prepared.stripped, role: "entry" }], 1);

  for (const probe of run.scenario.probes) {
    if (probe.method !== "hover") continue;
    const rawPos = anchorPos(run.ws, probe.anchor);
    const off = prepared.byteOffsets.get(probe.anchor);
    if (!rawPos || off === undefined) continue;
    // Resolve the `.vue` anchor to its queryable token (the oracle `.ts` side is
    // already last-identifier-resolved by `prepareOracleSource`).
    const vuePos = resolveAnchorToken(entryText, rawPos.line, rawPos.character);
    const requiredSnippets = run.scenario.invariants
      .filter(
        (i) => i.anchor === probe.anchor && i.method === "hover" && i.assertion === "contains",
      )
      .map((i) => i.value);
    const resolved: ResolvedOracleQuery = {
      probe,
      binding: { probeId: probe.id, oracleAnchor: probe.anchor, requiredSnippets },
      vue: { uri: entryUri, position: vuePos },
      oracle: {
        uri: pathToFileURL(oraclePath).toString(),
        path: oraclePath,
        version: 1,
        offset: off,
      },
    };
    try {
      const outcomes = await runResolvedOracleQuery(resolved, {
        verter: run.verter as unknown as OracleVerterClient,
        tsgo: run.bridge,
      });
      for (const outcome of outcomes) {
        run.outcomes.push({ scenario: run.scenario.id, driver: "rawLsp", outcome });
      }
    } catch (err) {
      run.log(`  (oracle query failed for ${run.scenario.id}/${probe.id}: ${String(err)})`);
    }
  }
}

function corpusOraclesDir(): string {
  // packages/dx-harness/oracles/semantic, sibling of fixtures/hermetic.
  return join(corpusFixturesDir(), "..", "..", "oracles", "semantic");
}

// ── orchestration ────────────────────────────────────────────────────────────────

async function runScenario(run: ScenarioRun): Promise<void> {
  const { scenario, ws, verter } = run;
  const entryUri = fileUri(ws.root, scenario.entryFile);
  const entryText = readFileSync(join(ws.root, scenario.entryFile), "utf-8");
  openVue(verter, entryUri, entryText);
  await new Promise((r) => setTimeout(r, 250));

  const methods = new Set(scenario.probes.map((p) => p.method));

  // Semantic-oracle scenarios run the tsgo gold-standard baseline rail.
  if (scenario.fixture === "semantic-oracle") {
    await runOracleScenario(run, entryUri, entryText);
  }

  for (const probe of scenario.probes) {
    try {
      if (probe.method === "hover") {
        await runHoverProbe(run, probe, entryUri, entryText);
      } else if (probe.method === "definition") {
        await runDefinitionProbe(run, probe, entryUri, entryText);
      } else if (probe.method === "completion") {
        const required = scenario.invariants
          .filter((i) => i.anchor === probe.anchor && i.assertion === "contains")
          .map((i) => i.value);
        // The member-access fixture's intent is the DrawerItem members.
        const labels = required.length > 0 ? required : ["id", "label"];
        await runCompletionProbe(run, probe, entryUri, entryText, labels);
      }
    } catch (err) {
      run.log(`  (probe ${scenario.id}/${probe.id} threw: ${String(err)})`);
    }
  }

  if (methods.has("diagnostics")) {
    await runDiagnosticsProbes(
      run,
      scenario.probes.filter((p) => p.method === "diagnostics"),
      entryUri,
      entryText,
    );
  }

  // Operational + auto-import collectors (proven, no baseline).
  for (const probe of scenario.probes) {
    try {
      if (probe.method === "autoImport") await runAutoImport(run, probe, entryUri, entryText);
      else if (probe.method === "churn") await runChurnProbe(run, probe, entryUri, entryText);
      else if (probe.method === "recovery") await runRecoveryProbe(run, probe, entryUri, entryText);
    } catch (err) {
      run.log(`  (operational probe ${scenario.id}/${probe.id} threw: ${String(err)})`);
    }
  }
}

function scenarioAnchorOffsets(run: ScenarioRun, entryText: string): Record<string, number> {
  const anchors: Record<string, number> = {};
  for (const a of run.scenario.anchors) {
    const p = anchorPos(run.ws, a);
    if (p) anchors[a] = positionToOffset(entryText, p.line, p.character);
  }
  return anchors;
}

async function runAutoImport(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
): Promise<void> {
  const sink = new CollectingSink();
  await collectAutoImport({
    client: run.verter,
    sink,
    uri: entryUri,
    buffer: new EditBuffer(entryText, scenarioAnchorOffsets(run, entryText)),
    script: run.scenario.script,
    scenario: run.scenario.id,
    probe: probe.id,
    anchor: probe.anchor,
    provider: PROVIDER,
    targetLabel: "computed",
    expectedImport: { symbol: "computed", module: "vue" },
    requestTimeoutMs: REQUEST_TIMEOUT,
  });
  run.events.push(...sink.events);
}

async function runChurnProbe(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
): Promise<void> {
  const sink = new CollectingSink();
  await collectChurn({
    client: run.verter,
    sink,
    uri: entryUri,
    buffer: new EditBuffer(entryText, scenarioAnchorOffsets(run, entryText)),
    script: run.scenario.script,
    scenario: run.scenario.id,
    probe: probe.id,
    anchor: probe.anchor,
    provider: PROVIDER,
    mode: "steadyStateQuiescedEdit",
    threshold: run.scenario.thresholds.steadyStateCompileDelta ?? 50,
    preconditions: {
      syncGenerationMatched: true,
      singleDocumentOpen: true,
      noNewImportsMidMeasurement: true,
    },
    awaitQuiescence: quiescer(run.verter),
    statisticsTimeoutMs: 10_000,
  });
  run.events.push(...sink.events);
}

async function runRecoveryProbe(
  run: ScenarioRun,
  probe: Probe,
  entryUri: string,
  entryText: string,
): Promise<void> {
  const sink = new CollectingSink();
  const anchors = scenarioAnchorOffsets(run, entryText);
  const editAnchor = run.scenario.script[0]?.anchor ?? probe.anchor;
  // The recovery probe must sample a STABLE position the burst does not move or
  // overwrite — otherwise baseline≠after-burst is a probe artifact, not a recovery
  // failure. The edit anchor sits on a blank line; instead probe the last identifier
  // on the nearest non-empty line ABOVE the edit point (e.g. `width` over the Drawer
  // editPoint), which the after-editPoint burst leaves untouched.
  const editRaw = anchorPos(run.ws, editAnchor);
  let stableOffset = anchors[editAnchor] ?? 0;
  if (editRaw) {
    const lines = entryText.split(/\r?\n/);
    for (let ln = editRaw.line - 1; ln >= 0; ln--) {
      const ids = [...(lines[ln] ?? "").matchAll(/[A-Za-z_$][\w$]*/g)].filter(
        (x) => !TS_KEYWORDS.has(x[0]),
      );
      if (ids.length > 0) {
        stableOffset = positionToOffset(entryText, ln, ids[ids.length - 1].index ?? 0);
        break;
      }
    }
  }
  await collectRecovery({
    client: run.verter,
    sink,
    uri: entryUri,
    buffer: new EditBuffer(entryText, { ...anchors, __probe: stableOffset }),
    burst: run.scenario.script,
    scenario: run.scenario.id,
    probe: probe.id,
    anchor: "__probe",
    provider: PROVIDER,
    maxRecoveryMs: run.scenario.thresholds.recovery?.maxRecoveryMs ?? 30_000,
    awaitQuiescence: quiescer(run.verter),
    correlatedSignals: () => [],
    requestTimeoutMs: REQUEST_TIMEOUT,
  });
  run.events.push(...sink.events);
}

// ── the run ──────────────────────────────────────────────────────────────────────

describe.skipIf(!LSP_BIN || !BASELINE_BIN)(
  "DX corpus sweep (real verter-lsp + tsgo baseline)",
  () => {
    it("drives every hermetic scenario, records verter-vs-baseline divergences, and writes DX-FINDINGS.md", async () => {
      const logLines: string[] = [];
      const log = (m: string): void => {
        logLines.push(m);
        // eslint-disable-next-line no-console
        console.log(m);
      };
      const corpus = loadScenarioCorpus();
      const scenarioIndex: Record<string, ScenarioMeta> = {};
      for (const s of corpus) {
        const probes: Record<
          string,
          {
            mappingPolicy: Probe["mappingPolicy"];
            confidence: Probe["confidence"];
            dimension: Probe["dimension"];
          }
        > = {};
        for (const p of s.probes)
          probes[p.id] = {
            mappingPolicy: p.mappingPolicy,
            confidence: p.confidence,
            dimension: p.dimension,
          };
        scenarioIndex[s.id] = { fixture: s.fixture, probes };
      }

      // Group scenarios by fixture so each workspace is materialized once.
      const byFixture = new Map<string, Scenario[]>();
      for (const s of corpus) {
        const list = byFixture.get(s.fixture) ?? [];
        list.push(s);
        byFixture.set(s.fixture, list);
      }

      const events: CollectorEvent[] = [];
      const outcomes: SituatedOutcome[] = [];
      let lastWsRoot = "";

      for (const [fixture, scenarios] of byFixture) {
        log(`\n=== fixture: ${fixture} (${scenarios.length} scenario(s)) ===`);
        let ws: MaterializedWorkspace | null = null;
        let verter: LspClient | null = null;
        let bridge: BridgeClient | null = null;
        try {
          ws = await createMaterializedWorkspace({
            fixtureDir: join(corpusFixturesDir(), fixture),
            repoRoot: process.cwd(),
            baselineBin: BASELINE_BIN,
            tsgoBin: TSGO_BIN,
            typeProvider: PROVIDER,
            strictVueVersion: false,
          });
          lastWsRoot = ws.root;
          if (ws.materializeReport.compileErrors.length > 0) {
            log(`  materialize compileErrors: ${ws.materializeReport.compileErrors.length}`);
          }
          verter = await startVerter(ws.root);

          const needsBridge = scenarios.some((s) => s.fixture === "semantic-oracle");
          if (needsBridge) {
            bridge = new BridgeClient(BASELINE_BIN!);
            const hello = await bridge.hello({
              workspaceRoot: ws.root,
              repoRoot: process.cwd(),
              provider: PROVIDER as "tsgo" | "tsserver",
              strictCi: false,
              toolRoot: {},
            });
            if (hello.type === "hello" && hello.skipped) {
              log(`  bridge skipped: ${hello.skipReason}`);
              await bridge.dispose();
              bridge = null;
            }
          }

          for (const scenario of scenarios) {
            log(`  scenario: ${scenario.id} (${scenario.probes.length} probe(s))`);
            const run: ScenarioRun = { scenario, ws, verter, bridge, events, outcomes, log };
            await runScenario(run);
          }
        } catch (err) {
          log(`  !! fixture ${fixture} failed: ${String(err)}`);
        } finally {
          if (bridge) await bridge.dispose();
          if (verter) await verter.kill();
          if (ws) disposeMaterializedWorkspace(ws);
        }
      }

      // Fold into findings.
      const obs: EventObservation[] = events.map((event) => ({ event }));
      const result = reduceFindings({
        scenarios: scenarioIndex,
        events: obs,
        outcomes,
        workspaceRoot: lastWsRoot,
      });
      const summary = buildSummary({
        findings: result.findings,
        baselineRan: result.baselineRan,
        allowlistHits: result.allowlistHits,
        allowlistVersion: 1,
      });
      const manifest = buildBaselineManifest(outcomes);

      mkdirSync(OUT_DIR, { recursive: true });
      writeFindingsMarkdown(join(OUT_DIR, "DX-FINDINGS.harness.md"), result.findings);
      writeSummary(join(OUT_DIR, "dx-summary.json"), summary);
      writeBaselineManifest(join(OUT_DIR, "baseline-manifest.json"), manifest);
      writeFileSync(
        join(OUT_DIR, "dx-events.json"),
        `${JSON.stringify(events, null, 2)}\n`,
        "utf8",
      );
      writeFileSync(join(OUT_DIR, "dx-sweep-log.txt"), `${logLines.join("\n")}\n`, "utf8");
      // The prioritized prompt-spec table (machine output; the curated, hand-triaged
      // DX-FINDINGS.md is authored on top of this and is NOT overwritten by a re-run).
      writeFileSync(
        join(OUT_DIR, "DX-FINDINGS.machine.md"),
        renderPromptFindings(result.findings, summary, manifest),
        "utf8",
      );

      log(`\n=== SWEEP COMPLETE ===`);
      log(
        `events=${events.length} outcomes=${outcomes.length} findings=${result.findings.length} baselineRan=${result.baselineRan.probes}`,
      );
      log(`severity: ${JSON.stringify(summary.bySeverity)}`);

      expect(result.baselineRan.probes).toBeGreaterThan(0);
      expect(events.length + outcomes.length).toBeGreaterThan(0);
    }, 900_000);
  },
);

// ── the prompt-spec prioritized findings table ───────────────────────────────────

function renderPromptFindings(
  findings: readonly DxFinding[],
  summary: ReturnType<typeof buildSummary>,
  manifest: ReturnType<typeof buildBaselineManifest>,
): string {
  const lines: string[] = [];
  lines.push("# Verter DX Findings — auto-discovered (dx-harness corpus sweep)", "");
  lines.push(
    "_Generated by the dx-harness corpus sweep: real `verter-lsp` driven over the committed",
    "hermetic corpus on materialized workspaces, differenced against the tsgo gold standard._",
    "",
  );
  lines.push("## Summary", "");
  lines.push(`- findings: ${findings.length}`);
  lines.push(
    `- by severity: ${(["S0", "S1", "S2", "S3", "S4"] as const).map((s) => `${s}=${summary.bySeverity[s]}`).join(", ")}`,
  );
  lines.push(
    `- by dimension: artifactParity=${summary.byDimension.artifactParity}, vueSemanticValidity=${summary.byDimension.vueSemanticValidity}`,
  );
  lines.push(`- baseline-ran (distinct probes): ${summary.baselineRan.probes}  ⟸ MUST be > 0`);
  lines.push(`- baseline executions (tsgo): ${manifest.totalExecutions}`);
  lines.push("");
  if (findings.length === 0) {
    lines.push("_No divergences recorded._", "");
    return `${lines.join("\n")}\n`;
  }
  lines.push("## Prioritized findings", "");
  lines.push(
    "| # | severity | scenario | signal | verter behavior | baseline (tsgo) behavior | divergence | root-cause hint |",
  );
  lines.push(
    "|---|----------|----------|--------|-----------------|--------------------------|------------|-----------------|",
  );
  findings.forEach((f, i) => {
    const cell = (s: string): string => s.replace(/\|/g, "\\|").replace(/\n/g, " ").slice(0, 160);
    lines.push(
      `| ${i + 1} | ${f.severity} | ${f.scenario} | ${f.signal} | ${cell(f.verterBehavior || "—")} | ${cell(f.baselineBehavior || "—")} | ${f.divergence ?? "—"} | ${cell(f.rootCauseHint ?? "—")} |`,
    );
  });
  lines.push("");
  return `${lines.join("\n")}\n`;
}
