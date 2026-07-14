// End-to-end raw-LSP signal collectors against the REAL `verter-lsp` binary, plus the
// curated `vue-semantic-validity` rail against the REAL `verter-dx-baseline` bridge.
//
// Gated on DX_LSP_BIN (an absolute path to the built verter binary) so the default
// `pnpm test:unit` stays hermetic; the baseline rail is additionally gated on
// DX_BASELINE_BIN. Build them with
//   cargo build -p verter_lsp           # produces target/debug/verter-lsp[.exe]
//   cargo build -p verter_dx_baseline   # produces the bridge binary
// then run, e.g.:
//   DX_LSP_BIN=$PWD/target/debug/verter-lsp \
//   DX_BASELINE_BIN=$PWD/target/debug/verter-dx-baseline \
//   pnpm -C packages/dx-harness test
//
// Two suites:
//  - the verter-only collectors whose contract needs no baseline: latency (real repeated
//    requests), churn (a steady-state `getStatistics` delta), logs (stderr collection +
//    the correlation gate), recovery (real probes + a forced-correlated negative control),
//    and auto-import (completion + resolve + applied-edit structural verification);
//  - the baseline rail, where completion / hover / definition / diagnostics each DRIVE
//    verter live AND consume the bridge's REAL typed output. Every baseline positive first
//    asserts the bridge returned the expected typed result, then a faithful baseline yields
//    agreement and a deliberately-divergent one yields the named divergence class — so a
//    mis-wired collector (verter unqueried, or the baseline never threaded into the verdict)
//    fails the gate. Each block records a skip reason when its binary/provider is absent.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";
import { afterEach, describe, expect, it } from "vitest";

import {
  collectAutoImport,
  collectChurn,
  collectCompletion,
  collectDefinition,
  collectDiagnostics,
  collectHover,
  collectLatency,
  collectLogs,
  collectRecovery,
  CollectingSink,
  EditBuffer,
  offsetToPosition,
  type CollectorLspClient,
  type CompletionBaseline,
  type DefinitionBaseline,
  type HoverBaseline,
  type ProbeSnapshot,
} from "../src/collectors/index.js";
import {
  BridgeClient,
  bridgeCompletionFact,
  bridgeDefinitionFact,
  bridgeDiagnosticsFact,
  bridgeHoverFact,
  GeneratedDocument,
  normalizeDefinition,
  type NormalizedHover,
  type ParsedSourceMap,
  type Probe,
  type ProviderInputs,
} from "../src/index.js";
import { awaitRawLspStartup, GET_STATISTICS_METHOD } from "../src/core/startupGate.js";
import { createWarnLineDrainer } from "../src/core/startupGate.js";
import { extractQuiescenceCounters, pollUntilQuiesced } from "../src/core/quiescence.js";

const BIN = process.env.DX_LSP_BIN;
const BASELINE_BIN = process.env.DX_BASELINE_BIN;
const PROVIDER = process.env.DX_LSP_PROVIDER ?? "tsgo";

const DOC = [
  '<script setup lang="ts">',
  'const greeting = "hello"',
  "const echo = greeting",
  "</script>",
  "<template>",
  "  <div>{{ greeting }}</div>",
  "</template>",
  "",
].join("\n");

const tmps: string[] = [];
const clients: LspClient[] = [];
const bridges: BridgeClient[] = [];
afterEach(async () => {
  for (const c of clients.splice(0)) await c.kill();
  for (const b of bridges.splice(0)) await b.dispose();
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** Offset of the SECOND occurrence of `needle` (the usage on the `const echo = greeting` line). */
function usageOffset(needle: string): number {
  const first = DOC.indexOf(needle);
  return DOC.indexOf(needle, first + 1);
}

function workspace(files: readonly string[]): string {
  const dir = mkdtempSync(join(tmpdir(), "dx-collectors-ws-"));
  tmps.push(dir);
  for (const name of files) writeFileSync(join(dir, name), DOC);
  return dir;
}

/** A workspace whose files carry DISTINCT authored content (name → source). */
function workspaceWith(files: Readonly<Record<string, string>>): string {
  const dir = mkdtempSync(join(tmpdir(), "dx-collectors-ws-"));
  tmps.push(dir);
  for (const [name, content] of Object.entries(files)) writeFileSync(join(dir, name), content);
  return dir;
}

async function startedClient(root: string): Promise<LspClient> {
  const rootUri = pathToFileURL(root).toString();
  const client = new LspClient("verter-lsp", BIN!, [root, `--type-provider=${PROVIDER}`]);
  clients.push(client);
  await client.initialize(
    {
      processId: process.pid,
      capabilities: { workspace: { workspaceFolders: true } },
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: "dx-collectors" }],
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

/** Reuse the shared quiescence gate live: counters + stderr WARN drain. */
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

const fileUri = (root: string, name: string): string => pathToFileURL(join(root, name)).toString();

describe.skipIf(!BIN)("raw-LSP signal collectors (real binary)", () => {
  it("latency: drives N real repeated requests and summarizes count === iterations", async () => {
    const root = workspace(["latency.vue"]);
    const client = await startedClient(root);
    const latSink = new CollectingSink();
    await collectLatency({
      client,
      sink: latSink,
      uri: fileUri(root, "latency.vue"),
      buffer: new EditBuffer(DOC, { ident: usageOffset("greeting") + 2 }),
      scenario: "live",
      probe: "hover-latency",
      anchor: "ident",
      provider: PROVIDER,
      method: "hover",
      lspMethod: "textDocument/hover",
      iterations: 5,
      requestTimeoutMs: 20_000,
    });
    const latency = latSink.events.find((e) => e.collector === "latency");
    expect(latency).toBeDefined();
    expect(latency?.signal).toBe("latency_summary");
    // One timed sample per real request — proves the loop issued exactly `iterations`.
    expect((latency?.data as { count?: number }).count).toBe(5);
  });

  it("churn: reads getStatistics before+after a quiesced edit and reports a numeric steady-state delta", async () => {
    const root = workspace(["churn.vue"]);
    const client = await startedClient(root);
    const sink = new CollectingSink();
    await collectChurn({
      client,
      sink,
      uri: fileUri(root, "churn.vue"),
      buffer: new EditBuffer(DOC, { tail: usageOffset("greeting") + "greeting".length }),
      script: [{ kind: "insert", anchor: "tail", text: " " }],
      scenario: "live",
      probe: "churn",
      anchor: "tail",
      provider: PROVIDER,
      mode: "steadyStateQuiescedEdit",
      threshold: 50,
      preconditions: {
        syncGenerationMatched: true,
        singleDocumentOpen: true,
        noNewImportsMidMeasurement: true,
      },
      awaitQuiescence: quiescer(client),
      statisticsTimeoutMs: 10_000,
    });
    const churn = sink.events.find((e) => e.collector === "churn");
    expect(churn).toBeDefined();
    // A `steady_state` delta (not `attribution_uncertain`) proves the preconditions held —
    // i.e. `$/verter/getStatistics` was read at quiescence both BEFORE and AFTER the edit.
    expect(churn?.signal).toBe("churn_steady_state_delta");
    expect(typeof (churn?.data as { delta?: number }).delta).toBe("number");
  });

  it("logs: collects the child's real stderr and never promotes uncorrelated mapping text to a hint", async () => {
    const root = workspace(["logs.vue"]);
    const client = await startedClient(root);
    // The server logs to stderr during startup, so the buffer the collector reads is non-empty —
    // proving the collector classifies a REAL collected stderr stream, not an empty stand-in.
    expect(client.stderr.text().length).toBeGreaterThan(0);
    const logSink = new CollectingSink();
    collectLogs({
      client,
      sink: logSink,
      scenario: "live",
      probe: "logs",
      anchor: "doc",
      provider: PROVIDER,
      version: 1,
      editStepIndex: 0,
      semanticFailures: [],
    });
    expect(logSink.events.every((e) => e.collector === "logs")).toBe(true);
    // With no correlated semantic failure, a mapping-failure line stays benign — it must
    // never be promoted to a `mapping_root_cause_hint`.
    expect(logSink.events.every((e) => e.signal !== "mapping_root_cause_hint")).toBe(true);
  });

  it("auto-import: resolves a cross-file candidate and binds the exact symbol from the exact module", async () => {
    // A sibling module exports `helperValue`; the `.vue` references it un-imported, so
    // completion offers the auto-import candidate. Resolving it must apply a real import
    // STATEMENT — the primary assertion verifies the resolved item structurally binds the
    // exact `helperValue` symbol from the exact `./helper` module: a missing candidate,
    // failed resolve, a wrong-symbol import, or a wrong module (even one whose specifier
    // merely CONTAINS `./helper`) each fails the `verifyAutoImport` gate.
    const vueSrc = [
      '<script setup lang="ts">',
      "const local = ",
      "</script>",
      "<template>",
      "  <div />",
      "</template>",
      "",
    ].join("\n");
    const root = workspaceWith({
      "helper.ts": "export const helperValue = 42\n",
      "autoImport.vue": vueSrc,
    });
    const client = await startedClient(root);
    const sink = new CollectingSink();
    const exprAnchor = vueSrc.indexOf("const local = ") + "const local = ".length;
    await collectAutoImport({
      client,
      sink,
      uri: fileUri(root, "autoImport.vue"),
      buffer: new EditBuffer(vueSrc, { expr: exprAnchor }),
      script: [{ kind: "insert", anchor: "expr", text: "helperValue", burst: true }],
      scenario: "live-auto-import",
      probe: "auto-import-cross-file",
      anchor: "expr",
      provider: PROVIDER,
      targetLabel: "helperValue",
      expectedImport: { symbol: "helperValue", module: "./helper" },
      requestTimeoutMs: 20_000,
    });
    const events = sink.events.filter((e) => e.collector === "autoImport");
    expect(events).toHaveLength(1);
    expect(events[0].signal).toBe("auto_import_applied");
    expect(events[0].ok).toBe(true);
  });

  it("auto-import: a DOTTED component file (`Model.Named.vue`) offers a SANITIZED tag and binds the valid identifier", async () => {
    // USER-FACING REGRESSION SURFACE (drives the REAL verter-lsp binary, both
    // providers via DX_LSP_PROVIDER): a sibling component file whose stem has a
    // `.` (`Model.Named.vue`) must surface a workspace-component tag completion
    // under the SANITIZED label `ModelNamed`, and resolving it must apply a
    // VALID `import ModelNamed from './Model.Named.vue'` — the binding is the
    // sanitized identifier, the module specifier is the real on-disk path.
    //
    // DISCRIMINATING: pre-fix the derived name was the invalid `Model.Named`, so
    // the offered label + import binding were `Model.Named`, producing the syntax
    // error `import Model.Named from './Model.Named.vue'`. With that behavior:
    //   - the `targetLabel: "ModelNamed"` candidate is never offered
    //     (`auto_import_no_candidate`), OR
    //   - the resolved import binds `Model.Named`, not `ModelNamed`
    //     (`verifyAutoImport` fails the exact-symbol gate).
    // Either way `ok` is false. Only the sanitizer makes this green.
    const parentSrc = [
      '<script setup lang="ts">',
      "const x = 1",
      "</script>",
      "<template>",
      "  <",
      "</template>",
      "",
    ].join("\n");
    const root = workspaceWith({
      // The dotted-stem component the parent has NOT imported yet.
      "Model.Named.vue": '<script setup lang="ts"></script>\n<template><div /></template>\n',
      "App.vue": parentSrc,
    });
    const client = await startedClient(root);
    const sink = new CollectingSink();
    // Anchor immediately after the `<` in the template tag-name position; the
    // script then types the sanitized component name to drive the tag completion.
    const tagAnchor = parentSrc.indexOf("  <") + "  <".length;
    await collectAutoImport({
      client,
      sink,
      uri: fileUri(root, "App.vue"),
      buffer: new EditBuffer(parentSrc, { tag: tagAnchor }),
      script: [{ kind: "insert", anchor: "tag", text: "ModelNamed", burst: true }],
      scenario: "live-auto-import",
      probe: "auto-import-dotted-component",
      anchor: "tag",
      provider: PROVIDER,
      targetLabel: "ModelNamed",
      expectedImport: { symbol: "ModelNamed", module: "./Model.Named.vue" },
      requestTimeoutMs: 20_000,
    });
    const events = sink.events.filter((e) => e.collector === "autoImport");
    expect(events).toHaveLength(1);
    // The resolved import must structurally bind the sanitized identifier from
    // the real module path — never the invalid `Model.Named` form.
    expect(events[0].signal).toBe("auto_import_applied");
    expect(events[0].ok).toBe(true);
  });

  it("recovery: drives real probes, captures non-empty snapshots, and confirms return-to-baseline", async () => {
    const root = workspace(["recovery.vue"]);
    const client = await startedClient(root);
    const uri = fileUri(root, "recovery.vue");
    const quiesce = quiescer(client);
    const sink = new CollectingSink();
    // The snapshot probe sits ON the `greeting` usage (line 2); the burst appends harmless
    // trailing whitespace AFTER it, so the probed surface is unchanged and the post-burst
    // snapshot returns to the baseline. The probe offset is before the edit, so it is unmoved.
    const probeOffset = usageOffset("greeting") + 2;
    const editOffset = usageOffset("greeting") + "greeting".length;
    const buffer = new EditBuffer(DOC, { probe: probeOffset, edit: editOffset });

    await collectRecovery({
      client,
      sink,
      uri,
      buffer,
      // Typed without waiting between characters — the recovery stress pattern.
      burst: [{ kind: "insert", anchor: "edit", text: "   ", burst: true }],
      scenario: "live",
      probe: "recovery",
      anchor: "probe",
      provider: PROVIDER,
      maxRecoveryMs: 30_000,
      // The collector DRIVES the completion/hover/diagnostics probes itself; the shared
      // quiescer is the only injected gate (no opaque snapshot callback).
      awaitQuiescence: quiesce,
      correlatedSignals: () => [],
      requestTimeoutMs: 20_000,
    });
    const recovery = sink.events.find((e) => e.collector === "recovery");
    expect(recovery).toBeDefined();
    expect(recovery?.signal).toBe("recovery_baseline_restored");
    const data = recovery?.data as { baseline: ProbeSnapshot; afterBurst: ProbeSnapshot };
    // The collector drove the REAL probes — a constant-empty snapshot (the original
    // gate-bypass) would leave empty completion sets and null hover labels here.
    expect(data.baseline.completionLabels.length).toBeGreaterThan(0);
    expect(data.afterBurst.completionLabels.length).toBeGreaterThan(0);
    expect(data.baseline.hoverLabel).not.toBeNull();
    expect(data.afterBurst.hoverLabel).not.toBeNull();
    // The published-diagnostic key sets are equivalent across the burst (set semantics).
    expect(new Set(data.afterBurst.diagnosticKeys)).toEqual(new Set(data.baseline.diagnosticKeys));
    expect(recovery?.ok).toBe(true);
  });

  it("recovery: a forced correlated signal is the negative control → recovery_not_restored", async () => {
    const root = workspace(["recoveryNeg.vue"]);
    const client = await startedClient(root);
    const uri = fileUri(root, "recoveryNeg.vue");
    const quiesce = quiescer(client);
    const sink = new CollectingSink();
    const probeOffset = usageOffset("greeting") + 2;
    const editOffset = usageOffset("greeting") + "greeting".length;
    const buffer = new EditBuffer(DOC, { probe: probeOffset, edit: editOffset });

    await collectRecovery({
      client,
      sink,
      uri,
      buffer,
      burst: [{ kind: "insert", anchor: "edit", text: "   ", burst: true }],
      scenario: "live",
      probe: "recovery-neg",
      anchor: "probe",
      provider: PROVIDER,
      maxRecoveryMs: 30_000,
      awaitQuiescence: quiesce,
      // A correlated critical signal during the window FAILS recovery regardless of the
      // (real, driven) probe surface — the discriminating negative control.
      correlatedSignals: () => [{ severity: "critical", signal: "forced_correlated_signal" }],
      requestTimeoutMs: 20_000,
    });
    const recovery = sink.events.find((e) => e.collector === "recovery");
    expect(recovery).toBeDefined();
    expect(recovery?.signal).toBe("recovery_not_restored");
    expect(recovery?.ok).toBe(false);
    expect(recovery?.severity).toBe("critical");
  });
});

// verter-on-`.vue` vs the curated `.ts` gold standard / a real bridge baseline, driven
// END-TO-END through the REAL signal collectors against BOTH live binaries (verter LSP +
// the `verter-dx-baseline` bridge). The `vueSemanticValidity` rail (mapping policy `none`)
// compares verter's `.vue` answer in native source space against the bridge's `.ts` answer
// in its own; the `baselineAt` rail threads the bridge's REAL typed output into the
// collector's own comparator. Each collector DRIVES verter itself and consumes the bridge's
// REAL normalized output: a faithful baseline yields agreement, a deliberately-divergent one
// yields the expected divergence class — so a mis-wired collector (verter unqueried, or the
// baseline never threaded into the verdict) fails this gate. The block is gated on BOTH
// binaries; an unavailable provider skips with a recorded `hello.skipReason`.
describe.skipIf(!BASELINE_BIN || !BIN)(
  "vue-semantic-validity via the real signal collectors (verter LSP + verter-dx-baseline)",
  () => {
    async function startedBridge(root: string): Promise<BridgeClient | null> {
      const bridge = new BridgeClient(BASELINE_BIN!);
      bridges.push(bridge);
      const hello = await bridge.hello({
        workspaceRoot: root,
        repoRoot: process.cwd(),
        provider: PROVIDER as "tsgo" | "tsserver",
        strictCi: false,
        toolRoot: {},
      });
      expect(hello.type).toBe("hello");
      if (hello.type !== "hello") throw new Error("expected hello");
      if (hello.skipped) {
        // The provider binary is unavailable here — a legitimate skip carrying a reason.
        expect(typeof hello.skipReason).toBe("string");
        return null;
      }
      return bridge;
    }

    const oracleProbe = (id: string, method: Probe["method"], anchor: string): Probe => ({
      id,
      method,
      anchor,
      mappingPolicy: "none",
      confidence: "high",
      dimension: "vueSemanticValidity",
      requiresSourceMap: false,
      requiredDrivers: [],
      capabilityRequirements: [],
    });

    /**
     * Query the bridge for a hover, ASSERT it returned the expected typed `hover` result
     * with content, and fold it through {@link bridgeHoverFact}.
     */
    async function bridgeHoverFactAt(
      bridge: BridgeClient,
      path: string,
      offset: number,
    ): Promise<ProviderInputs<NormalizedHover | null>> {
      const uri = pathToFileURL(path).toString();
      const response = await bridge.query({ method: "hover", uri, path, offset, version: 1 });
      expect(response.type).toBe("query");
      if (response.type !== "query" || response.result.kind !== "hover") {
        throw new Error("expected a hover query result");
      }
      expect(response.result.hover).not.toBeNull();
      return { tsgo: bridgeHoverFact(response) };
    }

    const COMPLETION_VUE = [
      '<script setup lang="ts">',
      "const objLit = { alphaProp: 1, betaProp: 2 }",
      "const pick = objLit",
      "</script>",
      "<template><div /></template>",
      "",
    ].join("\n");
    const COMPLETION_TS = "const objLit = { alphaProp: 1, betaProp: 2 }\nconst pick = objLit.\n";

    it("completion: collectCompletion parity with a faithful bridge set, divergence on a perturbed one", async () => {
      const vueRoot = workspaceWith({ "probe.vue": COMPLETION_VUE });
      const tsRoot = workspaceWith({ "probe.ts": COMPLETION_TS });
      const client = await startedClient(vueRoot);
      const bridge = await startedBridge(tsRoot);
      if (bridge === null) return;
      const tsPath = join(tsRoot, "probe.ts");
      const tsUri = pathToFileURL(tsPath).toString();
      await bridge.open([{ path: tsPath, content: COMPLETION_TS, role: "entry" }], 1);

      // The bridge must FIRST return the expected typed `completion` result for `objLit.`.
      const tsMemberOffset = COMPLETION_TS.indexOf("objLit.") + "objLit.".length;
      const response = await bridge.query({
        method: "completion",
        uri: tsUri,
        path: tsPath,
        offset: tsMemberOffset,
        version: 1,
        triggerCharacter: ".",
      });
      expect(response.type).toBe("query");
      if (response.type !== "query" || response.result.kind !== "completion") {
        throw new Error("expected a completion query result");
      }
      expect(response.result.items.map((i) => i.label)).toEqual(
        expect.arrayContaining(["alphaProp", "betaProp"]),
      );
      const fact = bridgeCompletionFact(response);
      if (!fact.ok) throw new Error("bridge refused the completion query");
      const bridgeSet = fact.output;

      const vueUri = fileUri(vueRoot, "probe.vue");
      const memberAnchor =
        COMPLETION_VUE.indexOf("objLit", COMPLETION_VUE.indexOf("const pick = objLit")) +
        "objLit".length;
      const insertDot = [{ kind: "insert" as const, anchor: "member", text: ".", burst: true }];

      // Faithful: the unperturbed bridge member set — verter's `.vue` member completion agrees.
      const okSink = new CollectingSink();
      await collectCompletion({
        client,
        sink: okSink,
        uri: vueUri,
        buffer: new EditBuffer(COMPLETION_VUE, { member: memberAnchor }),
        script: insertDot,
        scenario: "baseline-live",
        probe: "completion",
        anchor: "member",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        baselineAt: async () => ({ provider: PROVIDER, completion: bridgeSet }),
      });
      const okParity = okSink.events.filter((e) => e.signal === "completion_parity");
      expect(okParity.length).toBeGreaterThan(0);
      expect(okParity.every((e) => e.ok)).toBe(true);
      expect(
        okSink.events.filter((e) => e.signal === "no_suggestions_collapse").every((e) => e.ok),
      ).toBe(true);

      // Divergent: a deliberately-perturbed bridge set carrying a label verter cannot have.
      const perturbed: CompletionBaseline = {
        provider: PROVIDER,
        completion: {
          items: [...bridgeSet.items, { label: "gammaProp", kind: "Property" }],
          isIncomplete: bridgeSet.isIncomplete,
        },
      };
      const badSink = new CollectingSink();
      await collectCompletion({
        client,
        sink: badSink,
        uri: vueUri,
        buffer: new EditBuffer(COMPLETION_VUE, { member: memberAnchor }),
        script: insertDot,
        scenario: "baseline-live",
        probe: "completion",
        anchor: "member",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        baselineAt: async () => perturbed,
      });
      const badParity = badSink.events.filter((e) => e.signal === "completion_parity" && !e.ok);
      expect(badParity.length).toBeGreaterThan(0);
      expect(
        badParity.some((e) => {
          const cls = (e.data as { class?: string }).class;
          return cls === "missingLabel" || cls === "typeLabelMismatch";
        }),
      ).toBe(true);
    });

    const HOVER_VUE = [
      '<script setup lang="ts">',
      "const count: number = 1",
      "</script>",
      "<template>",
      "  <div>{{ count }}</div>",
      "</template>",
      "",
    ].join("\n");
    const HOVER_TS = 'const count: number = 1\nconst label: string = "x"\n';

    it("hover: collectHover agrees with both the oracle and the baseline, diverges on a different-typed one", async () => {
      const vueRoot = workspaceWith({ "probe.vue": HOVER_VUE });
      const tsRoot = workspaceWith({ "probe.ts": HOVER_TS });
      const client = await startedClient(vueRoot);
      const bridge = await startedBridge(tsRoot);
      if (bridge === null) return;
      const tsPath = join(tsRoot, "probe.ts");
      await bridge.open([{ path: tsPath, content: HOVER_TS, role: "entry" }], 1);

      const vueUri = fileUri(vueRoot, "probe.vue");
      const identAnchor = { ident: HOVER_VUE.indexOf("count") };
      const probe = oracleProbe("hover.count", "hover", "ident");

      // The faithful providers come from the mirrored `count` (number); they feed BOTH the
      // curated oracle AND the `baselineAt` parity rail in one collectHover call.
      const numberProviders = await bridgeHoverFactAt(bridge, tsPath, HOVER_TS.indexOf("count"));
      const numberHover = numberProviders.tsgo;
      if (numberHover === undefined || !numberHover.ok) throw new Error("expected a hover fact");

      const okSink = new CollectingSink();
      await collectHover({
        client,
        sink: okSink,
        uri: vueUri,
        buffer: new EditBuffer(HOVER_VUE, identAnchor),
        scenario: "baseline-live",
        probe: "hover",
        anchor: "ident",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        oracle: { probe, providers: numberProviders, requiredSnippets: ["number"] },
        baselineAt: async () => ({ provider: PROVIDER, hover: numberHover.output }),
      });
      const validity = okSink.events.filter((e) => e.signal === "hover_vue_semantic_validity");
      expect(validity).toHaveLength(1);
      expect(validity[0].ok).toBe(true);
      const parity = okSink.events.filter((e) => e.signal === "hover_parity");
      expect(parity.length).toBeGreaterThan(0);
      expect(parity.every((e) => e.ok)).toBe(true);

      // Divergent: the providers come from `label` (string) — a different type than verter's
      // `count: number`, so BOTH the oracle and the baseline parity rail fault as typeLabelMismatch.
      const stringProviders = await bridgeHoverFactAt(bridge, tsPath, HOVER_TS.indexOf("label"));
      const stringHover = stringProviders.tsgo;
      if (stringHover === undefined || !stringHover.ok) throw new Error("expected a hover fact");

      const badSink = new CollectingSink();
      await collectHover({
        client,
        sink: badSink,
        uri: vueUri,
        buffer: new EditBuffer(HOVER_VUE, identAnchor),
        scenario: "baseline-live",
        probe: "hover",
        anchor: "ident",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        oracle: { probe, providers: stringProviders },
        baselineAt: async () => ({ provider: PROVIDER, hover: stringHover.output }),
      });
      const badValidity = badSink.events.filter(
        (e) => e.signal === "hover_vue_semantic_validity" && !e.ok,
      );
      expect(badValidity).toHaveLength(1);
      expect((badValidity[0].data as { class?: string }).class).toBe("typeLabelMismatch");
      const badParity = badSink.events.filter((e) => e.signal === "hover_parity" && !e.ok);
      expect(badParity.length).toBeGreaterThan(0);
      expect(
        badParity.some((e) => (e.data as { class?: string }).class === "typeLabelMismatch"),
      ).toBe(true);
    });

    const DEF_VUE = [
      '<script setup lang="ts">',
      "const target = 1",
      "const use = target",
      "</script>",
      "<template>",
      "  <div>{{ use }}</div>",
      "</template>",
      "",
    ].join("\n");
    const DEF_TS = "const target = 1\nconst use = target\n";
    // A small hand-authored GENERATED artifact (a `.vue.tsx`, an `isGeneratedUri` path): the
    // bridge resolves a definition INSIDE it, and the source map below projects that generated
    // location back to verter's authored `.vue` target.
    const DEF_GENERATED = "const target = 1\nconst use = target\n";

    /** A V3 map with one mapped segment: a generated position → an authored `{ line, column }`. */
    function singleSegmentMap(
      source: string,
      generatedStart: { readonly line: number; readonly character: number },
      authoredStart: { readonly line: number; readonly character: number },
    ): ParsedSourceMap {
      const lines: {
        genColumn: number;
        source: { index: number; line: number; column: number };
      }[][] = [];
      for (let line = 0; line < generatedStart.line; line++) lines.push([]);
      lines.push([
        {
          genColumn: generatedStart.character,
          source: { index: 0, line: authoredStart.line, column: authoredStart.character },
        },
      ]);
      return { sources: [source], lines };
    }

    it("definition: collectDefinition(baselineAt, no expected) agrees on the mapped generated location, diverges on a wrong range", async () => {
      const vueRoot = workspaceWith({ "def.vue": DEF_VUE });
      const genRoot = workspaceWith({ "def.vue.tsx": DEF_GENERATED });
      const client = await startedClient(vueRoot);
      const bridge = await startedBridge(genRoot);
      if (bridge === null) return;

      // The bridge resolves a definition INSIDE the generated artifact and must FIRST return
      // the expected typed `definition` result with at least one location.
      const genPath = join(genRoot, "def.vue.tsx");
      const genUri = pathToFileURL(genPath).toString();
      await bridge.open([{ path: genPath, content: DEF_GENERATED, role: "entry" }], 1);
      const genUsageOffset = DEF_GENERATED.indexOf("target", DEF_GENERATED.indexOf("const use"));
      const response = await bridge.query({
        method: "definition",
        uri: genUri,
        path: genPath,
        offset: genUsageOffset,
        version: 1,
      });
      expect(response.type).toBe("query");
      const defFact = bridgeDefinitionFact(response);
      if (!defFact.ok) throw new Error("bridge refused the definition query");
      expect(defFact.output.length).toBeGreaterThan(0);
      const bridgeLoc = defFact.output[0];

      // Drive verter once to learn its REAL authored `.vue` target (so the map projects the
      // generated location onto exactly that target).
      const vueUri = fileUri(vueRoot, "def.vue");
      const usageOff = DEF_VUE.indexOf("target", DEF_VUE.indexOf("const use"));
      const rawDef: unknown = await client.sendRequest(
        "textDocument/definition",
        {
          textDocument: { uri: vueUri },
          position: offsetToPosition(DEF_VUE, usageOff, client.positionEncoding),
        },
        20_000,
      );
      const verterTargets = normalizeDefinition(
        rawDef as Parameters<typeof normalizeDefinition>[0],
      );
      const vTarget = verterTargets.find((t) => t.fromGenerated !== true);
      expect(vTarget).toBeDefined();
      if (vTarget === undefined) throw new Error("verter resolved no authored target");
      expect(vTarget.uri).toBe(vueUri);

      // The bridge's generated byte range → a generated `{ line, character }` (the position the
      // map's segment is keyed at, matching what `compareDefinition` will recompute internally).
      const genRange = new GeneratedDocument(DEF_GENERATED).byteRangeToPosition(
        bridgeLoc.start,
        bridgeLoc.end,
      );
      const baseline: DefinitionBaseline = {
        provider: PROVIDER,
        locations: { locations: [bridgeLoc], texts: { [bridgeLoc.path]: DEF_GENERATED } },
      };

      // Faithful: the map projects the generated location onto verter's authored target → agreement.
      const okSink = new CollectingSink();
      await collectDefinition({
        client,
        sink: okSink,
        uri: vueUri,
        buffer: new EditBuffer(DEF_VUE, { use: usageOff }),
        scenario: "baseline-live",
        probe: "definition",
        anchor: "use",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        map: singleSegmentMap(vTarget.uri, genRange.start, vTarget.range.start),
        baselineAt: async () => baseline,
      });
      const ok = okSink.events.filter((e) => e.collector === "definition");
      expect(ok).toHaveLength(1);
      expect(ok[0].signal).toBe("definition_parity");
      expect(ok[0].ok).toBe(true);

      // Divergent: the SAME real bridge location projected onto a DIFFERENT authored range in the
      // same file → a range mismatch (no `expected` short-circuit governs this comparison).
      const badSink = new CollectingSink();
      await collectDefinition({
        client,
        sink: badSink,
        uri: vueUri,
        buffer: new EditBuffer(DEF_VUE, { use: usageOff }),
        scenario: "baseline-live",
        probe: "definition",
        anchor: "use",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        map: singleSegmentMap(vTarget.uri, genRange.start, {
          line: vTarget.range.start.line + 7,
          character: 0,
        }),
        baselineAt: async () => baseline,
      });
      const bad = badSink.events.filter((e) => e.collector === "definition" && !e.ok);
      expect(bad).toHaveLength(1);
      expect(bad[0].signal).toBe("definition_parity");
      expect((bad[0].data as { class?: string }).class).toBe("rangeMismatch");
    });

    it("definition: the expected-identity rail resolves the authored target and diverges on a wrong one", async () => {
      // This rail compares verter against an EXPECTED authored identity (`expected` governs and
      // returns before baseline comparison); it complements — and does not replace — the
      // baselineAt rail above.
      const vueRoot = workspaceWith({ "def.vue": DEF_VUE });
      const tsRoot = workspaceWith({ "def.ts": DEF_TS });
      const client = await startedClient(vueRoot);
      const bridge = await startedBridge(tsRoot);
      if (bridge === null) return;

      // Resolvability gate: confirm the real bridge resolves the mirrored `.ts` declaration.
      const tsPath = join(tsRoot, "def.ts");
      const tsUri = pathToFileURL(tsPath).toString();
      await bridge.open([{ path: tsPath, content: DEF_TS, role: "entry" }], 1);
      const tsDef = await bridge.query({
        method: "definition",
        uri: tsUri,
        path: tsPath,
        offset: DEF_TS.indexOf("target", DEF_TS.indexOf("use")),
        version: 1,
      });
      expect(tsDef.type).toBe("query");
      const tsFact = bridgeDefinitionFact(tsDef);
      if (!tsFact.ok) throw new Error("bridge refused the definition query");
      expect(tsFact.output.length).toBeGreaterThan(0);

      const vueUri = fileUri(vueRoot, "def.vue");
      const usageAnchor = { use: DEF_VUE.indexOf("target", DEF_VUE.indexOf("use")) };

      // Faithful: verter resolves the usage to a target in the SAME `.vue` file.
      const okSink = new CollectingSink();
      await collectDefinition({
        client,
        sink: okSink,
        uri: vueUri,
        buffer: new EditBuffer(DEF_VUE, usageAnchor),
        scenario: "oracle-live",
        probe: "definition",
        anchor: "use",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        expected: { uri: vueUri },
      });
      const ok = okSink.events.filter((e) => e.collector === "definition");
      expect(ok).toHaveLength(1);
      expect(ok[0].signal).toBe("definition_parity");
      expect(ok[0].ok).toBe(true);

      // Divergent: a WRONG authored identity — verter's real target does not match it.
      const wrongUri = pathToFileURL(join(vueRoot, "nonexistent.vue")).toString();
      const badSink = new CollectingSink();
      await collectDefinition({
        client,
        sink: badSink,
        uri: vueUri,
        buffer: new EditBuffer(DEF_VUE, usageAnchor),
        scenario: "oracle-live",
        probe: "definition",
        anchor: "use",
        provider: PROVIDER,
        requestTimeoutMs: 20_000,
        expected: { uri: wrongUri },
      });
      const bad = badSink.events.filter((e) => e.collector === "definition" && !e.ok);
      expect(bad).toHaveLength(1);
      expect((bad[0].data as { class?: string }).class).toBe("wrongTarget");
    });

    const BAD_VUE = [
      '<script setup lang="ts">',
      "const bad: string = 1",
      "</script>",
      "<template>",
      "  <div>{{ bad }}</div>",
      "</template>",
      "",
    ].join("\n");
    const BAD_TS = "export const bad: string = 1\n"; // 2322: number is not assignable to string
    const GOOD_TS = "export const good: number = 1\n"; // clean

    it("diagnostics: collectDiagnostics agrees with the matching `.ts` oracle error and flags verter-only vs a clean oracle", async () => {
      const vueRoot = workspaceWith({ "bad.vue": BAD_VUE });
      const tsRoot = workspaceWith({ "badOracle.ts": BAD_TS, "goodOracle.ts": GOOD_TS });
      const client = await startedClient(vueRoot);
      const bridge = await startedBridge(tsRoot);
      if (bridge === null) return;

      const badPath = join(tsRoot, "badOracle.ts");
      const goodPath = join(tsRoot, "goodOracle.ts");
      await bridge.open(
        [
          { path: badPath, content: BAD_TS, role: "entry" },
          { path: goodPath, content: GOOD_TS, role: "support" },
        ],
        1,
      );
      const badDiag = await bridge.diagnostics({
        uri: pathToFileURL(badPath).toString(),
        path: badPath,
        version: 1,
      });
      expect(badDiag.type).toBe("diagnostics");
      if (badDiag.type !== "diagnostics") throw new Error("expected diagnostics");
      expect(badDiag.diagnostics.length).toBeGreaterThan(0); // the `.ts` oracle flags the error
      const goodDiag = await bridge.diagnostics({
        uri: pathToFileURL(goodPath).toString(),
        path: goodPath,
        version: 1,
      });
      if (goodDiag.type !== "diagnostics") throw new Error("expected diagnostics");
      expect(goodDiag.diagnostics).toHaveLength(0); // the clean `.ts` oracle flags nothing

      const vueUri = fileUri(vueRoot, "bad.vue");
      const quiesce = quiescer(client);
      const probe = oracleProbe("diag.bad", "diagnostics", "doc");

      // Faithful: verter's real `.vue` error matches the `.ts` oracle error (paired by code),
      // folded through `bridgeDiagnosticsFact`.
      const okSink = new CollectingSink();
      await collectDiagnostics({
        client,
        sink: okSink,
        uri: vueUri,
        buffer: new EditBuffer(BAD_VUE, { doc: 0 }),
        scenario: "oracle-live",
        probe: "diagnostics",
        anchor: "doc",
        provider: PROVIDER,
        settle: async () => {
          await quiesce();
        },
        oracle: { probe, providers: { tsgo: bridgeDiagnosticsFact(badDiag) } },
      });
      const ok = okSink.events.filter((e) => e.signal === "diagnostics_vue_semantic_validity");
      expect(ok).toHaveLength(1);
      expect(ok[0].ok).toBe(true);

      // Divergent: verter's real `.vue` error vs a CLEAN oracle → a verter-only finding.
      const badSink = new CollectingSink();
      await collectDiagnostics({
        client,
        sink: badSink,
        uri: vueUri,
        buffer: new EditBuffer(BAD_VUE, { doc: 0 }),
        scenario: "oracle-live",
        probe: "diagnostics",
        anchor: "doc",
        provider: PROVIDER,
        settle: async () => {
          await quiesce();
        },
        oracle: { probe, providers: { tsgo: bridgeDiagnosticsFact(goodDiag) } },
      });
      const bad = badSink.events.filter(
        (e) => e.signal === "diagnostics_vue_semantic_validity" && !e.ok,
      );
      expect(bad).toHaveLength(1);
      expect((bad[0].data as { class?: string }).class).toBe("verterOnly");
    });
  },
);
