/** Behavioral contract tests for endurance framework/mode parity and attestation. */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";
import type { LspClient } from "@verter/lsp-test-client";

import {
  CHURN_ACCEPTANCE_MIN_CYCLES,
  CHURN_RETENTION_COUNTERS,
  ENDURANCE_LANES,
  EnduranceSession,
  FailureBag,
  LatencyRecorder,
  RequestTracker,
  buildCarrierSet,
  buildComponentFixture,
  buildComponentEventSiteProbes,
  buildComponentIntegrationProbes,
  carrierStormProbes,
  collectCorpusCarrierFiles,
  disposeWorkspace,
  deriveCorpusProbes,
  churnCarrierContent,
  DEFAULT_ENDURANCE_LANE,
  decideChurnGrowth,
  decideChurnRetention,
  decideChurnSlope,
  describeProcessTreeRss,
  extractRetentionReading,
  sampleProcessTreeRss,
  heavyUpdateFixture,
  loadEnduranceConfig,
  parseProviderRuntimeAttestation,
  receiptCoreFailures,
  runRenameCycles,
  runSoakScenario,
  soakProbes,
  typeInsertion,
  type EnduranceConfig,
  type EnduranceLane,
  type EnduranceProbe,
  type ChurnCheckpoint,
  type ProcessTreeRssDeps,
  type RetentionReading,
  type ScenarioContext,
} from "../src/endurance/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const temporaryDirectories: string[] = [];

function temporaryDirectory(prefix: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  temporaryDirectories.push(dir);
  return dir;
}

afterEach(() => {
  while (temporaryDirectories.length > 0) {
    disposeWorkspace(temporaryDirectories.pop()!);
  }
});

describe("endurance framework and language-mode matrix", () => {
  it("types insertion prefixes from one stable base and leaves one final insertion", async () => {
    const changes: string[] = [];
    const checkpoints: string[] = [];
    const client = {
      positionEncoding: "utf-16",
      documentPositions(text: string) {
        return {
          utf16ToPosition(offset: number) {
            const prefix = text.slice(0, offset);
            const lines = prefix.split("\n");
            return { line: lines.length - 1, character: lines.at(-1)!.length };
          },
        };
      },
      sendNotification(method: string, params: unknown) {
        if (method !== "textDocument/didChange") return;
        changes.push(
          (params as { contentChanges: Array<{ text: string }> }).contentChanges[0].text,
        );
      },
      async sendRequest() {
        return [{ label: "length" }];
      },
    } as unknown as LspClient;
    const config = { ...loadEnduranceConfig(), typingCps: 10_000 };
    const session = new EnduranceSession(
      client,
      temporaryDirectory("verter-endurance-insertion-"),
      {
        config,
        recorder: new LatencyRecorder(config.windowMs),
        tracker: new RequestTracker(),
      },
    );
    const relativePath = "src/Insertion.vue";
    session.openFile(relativePath, "before<anchor>after");
    const context = { session, config } as unknown as ScenarioContext;
    const failures = new FailureBag();

    await typeInsertion(
      context,
      relativePath,
      "<anchor>",
      "abc",
      [1, 2].map((atLength) => ({
        atLength,
        makeProbe(typed: string): EnduranceProbe {
          checkpoints.push(typed);
          return {
            kind: "completion",
            relativePath,
            needle: typed,
            cursorOffset: typed.length,
            expectLabels: ["length"],
            label: `insertion checkpoint ${atLength}`,
          };
        },
      })),
      failures,
    );

    expect(checkpoints).toEqual(["a", "ab"]);
    expect(changes).toEqual([
      "beforea<anchor>after",
      "beforeab<anchor>after",
      "beforeabc<anchor>after",
    ]);
    expect(session.textOf(relativePath)).toBe("beforeabc<anchor>after");
    expect(failures.list).toEqual([]);
  });

  it("hard-probes renamed and restored symbols on every scale rename cycle", async () => {
    const probes: EnduranceProbe[] = [];
    let text =
      "const anchorValue = 1;\n" +
      "const anchorValueLength = anchorValue.toString().length;\n" +
      "console.log(anchorValue);\n";
    const originalText = text;
    const session = {
      textOf() {
        return text;
      },
      changeFile(_relativePath: string, next: string) {
        text = next;
      },
      async runProbe(probe: EnduranceProbe) {
        probes.push(probe);
        return {
          classification: "answered",
          latencyMs: 0,
          mismatch: null,
          result: null,
        };
      },
    };
    const config = loadEnduranceConfig();
    const context = {
      scenario: "scale-heavy-update",
      route: config.route,
      lane: ENDURANCE_LANES[0],
      session,
      config,
      sampler: null,
      providerAttestation: () => ({ route: config.route }),
    } as unknown as ScenarioContext;
    const failures = new FailureBag();
    const cycles = 2;

    const finalSanityPass = await runRenameCycles(
      context,
      "src/Scale.vue",
      "anchorValue",
      cycles,
      failures,
    );

    expect(finalSanityPass).toBe(true);
    expect(text).toBe(originalText);
    expect(failures.list).toEqual([]);
    expect(probes).toHaveLength(cycles * 4);
    for (let cycle = 0; cycle < cycles; cycle += 1) {
      const renamed = `anchorValue__renamed${cycle}`;
      const cycleProbes = probes.slice(cycle * 4, cycle * 4 + 4);
      expect(cycleProbes.map((probe) => probe.kind)).toEqual([
        "hover",
        "definition",
        "hover",
        "definition",
      ]);
      for (const hover of [cycleProbes[0], cycleProbes[2]]) {
        if (hover.kind === "hover") expect(hover.forbidIncludes).toContain("any");
      }
      const restoredHover = cycleProbes[2];
      expect(restoredHover.label).toContain("restored usage hover");
      if (restoredHover.kind === "hover") {
        expect(restoredHover.expectIncludes).toContain("anchorValue");
        expect(restoredHover.occurrence).toBe(2);
        expect(restoredHover.forbidIncludes).toEqual(expect.arrayContaining([renamed, "any"]));
        expect(restoredHover.informational).not.toBe(true);
      }
      const restoredDefinition = cycleProbes[3];
      expect(restoredDefinition.label).toContain("restored usage definition");
      if (restoredDefinition.kind === "definition") {
        expect(restoredDefinition.expectLineNeedle).toBe("const anchorValue");
        expect(restoredDefinition.informational).not.toBe(true);
      }
    }
  });

  it("renames dollar-delimited JS identifiers without touching prefix collisions", async () => {
    const probes: EnduranceProbe[] = [];
    let text =
      "const $anchorValue$ = 1;\n" +
      "const $anchorValue$Length = $anchorValue$.toString().length;\n" +
      "console.log($anchorValue$);\n";
    const originalText = text;
    const config = loadEnduranceConfig();
    const context = {
      scenario: "scale-heavy-update",
      route: config.route,
      lane: ENDURANCE_LANES[0],
      session: {
        textOf: () => text,
        changeFile: (_relativePath: string, next: string) => {
          text = next;
        },
        async runProbe(probe: EnduranceProbe) {
          probes.push(probe);
          return {
            classification: "answered",
            latencyMs: 0,
            mismatch: null,
            result: null,
          };
        },
      },
      config,
      sampler: null,
      providerAttestation: () => ({ route: config.route }),
    } as unknown as ScenarioContext;
    const failures = new FailureBag();

    const finalSanityPass = await runRenameCycles(
      context,
      "src/Dollar.vue",
      "$anchorValue$",
      1,
      failures,
    );

    expect(finalSanityPass).toBe(true);
    expect(text).toBe(originalText);
    expect(failures.list).toEqual([]);
    expect(probes).toHaveLength(4);
    expect(probes[0].needle).toBe("$anchorValue$__renamed0");
    expect(probes[2].needle).toBe("$anchorValue$");
    expect(probes[2].occurrence).toBe(2);
  });

  it("sends framework and TS-family language IDs in didOpen", () => {
    const notifications: Array<{ method: string; params: unknown }> = [];
    const client = {
      sendNotification(method: string, params: unknown) {
        notifications.push({ method, params });
      },
    } as unknown as LspClient;
    const config = loadEnduranceConfig();
    const session = new EnduranceSession(
      client,
      temporaryDirectory("verter-endurance-language-id-"),
      {
        config,
        recorder: new LatencyRecorder(config.windowMs),
        tracker: new RequestTracker(),
      },
    );

    session.openFile("src/Card.vue", "<template />");
    session.openFile("src/Card.svelte", "<div />");
    session.openFile("src/contract.ts", "export {};");
    session.openFile("src/contract.js", "export {};");
    session.openFile("src/contract.tsx", "export {};");
    session.openFile("src/contract.jsx", "export {};");

    expect(
      notifications.map(
        ({ params }) =>
          (params as { textDocument: { languageId: string } }).textDocument.languageId,
      ),
    ).toEqual(["vue", "svelte", "typescript", "javascript", "typescriptreact", "javascriptreact"]);
  });

  it("covers Vue and Svelte in TypeScript and JavaScript modes", () => {
    expect(ENDURANCE_LANES.map((lane) => lane.id)).toEqual([
      "vue-ts",
      "vue-js",
      "svelte-ts",
      "svelte-js",
    ]);
  });

  it.each<EnduranceLane>([
    { id: "vue-ts", framework: "vue", mode: "ts" },
    { id: "vue-js", framework: "vue", mode: "js" },
    { id: "svelte-ts", framework: "svelte", mode: "ts" },
    { id: "svelte-js", framework: "svelte", mode: "js" },
  ])("build-component fixture and typing completion are lane-specific: $id", (lane) => {
    const fixture = buildComponentFixture(lane);
    const unusedLabel = lane.framework === "vue" ? "unused-only" : "unusedOnly";
    expect(fixture.childPath.endsWith(`.${lane.framework}`)).toBe(true);
    expect(fixture.parentPath.endsWith(`.${lane.framework}`)).toBe(true);
    expect(fixture.childFinal).toContain(lane.framework === "svelte" ? "$props()" : "defineProps");
    expect(fixture.childFinal).toContain(lane.framework === "svelte" ? "{#snippet" : "defineSlots");
    expect(fixture.childFinal).toContain(lane.framework === "svelte" ? "onclick" : "defineEmits");
    if (lane.framework === "svelte") {
      expect(fixture.childFinal).toContain("children?:");
      expect(fixture.childFinal).toContain("Snippet<[boolean]>");
      expect(fixture.childFinal).toContain("onclick?.()");
      expect(fixture.childFinal).toContain("title.length");
      expect(fixture.childFinal).toContain("levelValue = level");
      expect(fixture.childFinal).toContain("{@render children?.(true)}");
      expect(fixture.parentFinal).toContain("{#snippet children(active");
      expect(fixture.parentFinal).not.toContain("onclick={onSave} />");
    }
    expect(fixture.childFinal.includes('lang="ts"')).toBe(lane.mode === "ts");

    const midTyping = fixture.parentCheckpoints.find((checkpoint) => {
      const candidate = checkpoint.makeProbe(fixture.parentFinal.slice(0, checkpoint.atLength));
      return candidate.kind === "completion" && candidate.expectLabels.includes(unusedLabel);
    });
    expect(midTyping, "real-LSP completion must be scheduled during parent typing").toBeDefined();
    expect(midTyping!.atLength).toBeLessThan(fixture.parentFinal.length);
    const probe = midTyping!.makeProbe(fixture.parentFinal.slice(0, midTyping!.atLength));
    expect(probe.kind).toBe("completion");
    if (probe.kind === "completion") {
      expect(probe.expectLabels).toContain(unusedLabel);
      const typed = fixture.parentFinal.slice(0, midTyping!.atLength);
      expect(typed.endsWith("<DraftCard ")).toBe(true);
      for (const label of probe.expectLabels) expect(typed).not.toContain(label);
      expect(probe.informational).not.toBe(true);
    }

    for (const checkpoint of fixture.childCheckpoints) {
      const typed = fixture.childMemberInsertion.slice(0, checkpoint.atLength);
      const childProbe = checkpoint.makeProbe(typed);
      if (childProbe.kind === "completion") expect(childProbe.expectLabels).toEqual(["length"]);
    }
    for (const checkpoint of fixture.parentMemberCheckpoints) {
      const typed = fixture.parentMemberInsertion.slice(0, checkpoint.atLength);
      const parentProbe = checkpoint.makeProbe(typed);
      if (parentProbe.kind === "completion") expect(parentProbe.expectLabels).toEqual(["length"]);
    }

    const integrationProbes = buildComponentIntegrationProbes(fixture);
    if (lane.framework === "vue") {
      expect(integrationProbes.some((probe) => probe.label.includes("slot-name"))).toBe(true);
      const slotMapping = integrationProbes.find(
        (probe) => probe.kind === "definition" && probe.label.includes("slot-name mapped"),
      );
      expect(slotMapping).toBeDefined();
      if (slotMapping?.kind === "definition") {
        expect(slotMapping.needle).toBe("#active");
        expect(slotMapping.expectUriSuffix).toBe(`/${fixture.childPath}`);
        expect(slotMapping.expectLineNeedle).toContain(
          lane.mode === "ts" ? "defineSlots<{ active" : '<slot name="active"',
        );
        expect(slotMapping.informational).not.toBe(true);
      }
      const mappedActive = integrationProbes.find(
        (probe) =>
          probe.kind === "definition" &&
          probe.needle.includes("{ active") &&
          probe.expectUriSuffix === `/${fixture.childPath}`,
      );
      if (lane.mode === "ts") expect(mappedActive).toBeDefined();
      else expect(mappedActive).toBeUndefined();
      if (mappedActive?.kind === "definition") {
        expect(mappedActive.expectLineNeedle).toContain("active(props: { active: boolean })");
        expect(mappedActive.informational).not.toBe(true);
      }
      expect(fixture.parentFinal).toContain("{ active }");
      expect(fixture.childFinal).toContain(':active="true"');
    } else {
      expect(integrationProbes.some((probe) => probe.label.includes("active definition"))).toBe(
        true,
      );
      expect(
        integrationProbes.some((probe) => probe.label.includes("incoming children render")),
      ).toBe(true);
    }
    const hardTypedHovers = integrationProbes.filter(
      (probe) => probe.kind === "hover" && probe.informational !== true,
    );
    expect(hardTypedHovers).toHaveLength(lane.mode === "ts" ? 1 : 0);
    for (const probe of hardTypedHovers) {
      if (probe.kind === "hover") expect(probe.forbidIncludes).toContain("any");
    }

    const eventSiteProbes = buildComponentEventSiteProbes(fixture);
    expect(eventSiteProbes).toHaveLength(2);
    if (lane.framework === "svelte") {
      expect(eventSiteProbes.map((probe) => probe.needle)).toEqual([
        "onclick={saveDraft}",
        "onclick={onSave}",
      ]);
      expect(eventSiteProbes.every((probe) => probe.informational !== true)).toBe(true);
    }
  });

  it.each(ENDURANCE_LANES)("storm carriers preserve the $id framework/mode", (lane) => {
    const set = buildCarrierSet(3, lane);
    expect(set.carriers).toHaveLength(3);
    expect(set.carriers.every((file) => file.endsWith(`.${lane.framework}`))).toBe(true);
    const source = set.files[set.carriers[2]];
    expect(source).toContain(lane.framework === "svelte" ? "$props()" : "defineProps");
    expect(source.includes('lang="ts"')).toBe(lane.mode === "ts");

    const probes = carrierStormProbes(set.carriers, lane);
    const d1 = probes.filter(
      (probe) => probe.kind === "completion" && probe.label.includes("(D1)"),
    );
    expect(d1).toHaveLength(2);
    for (const probe of d1) {
      if (probe.kind !== "completion") continue;
      const probeSource = set.files[probe.relativePath];
      for (const label of probe.expectLabels) expect(probeSource).not.toContain(label);
    }
    if (lane.framework === "svelte") {
      expect(source).toContain("Snippet<[string]>");
      expect(source).toContain("onfire?.(");
      expect(source).toContain("{#snippet carrierSnippet");
      expect(source).toContain("{@render carrierSnippet");
      for (const fragment of ["callback-event definition", "snippet hover", "snippet definition"]) {
        const matching = probes.filter((probe) => probe.label.includes(fragment));
        expect(matching.length).toBeGreaterThan(0);
        if (fragment !== "snippet hover" || lane.mode === "ts") {
          expect(matching.every((probe) => probe.informational !== true)).toBe(true);
        }
      }
      const eventSites = probes.filter((probe) => probe.label.includes("event-site definition"));
      expect(eventSites).toHaveLength(2);
      expect(eventSites.every((probe) => probe.needle.includes("onclick={"))).toBe(true);
      expect(eventSites.every((probe) => probe.informational !== true)).toBe(true);
    }
  });

  it("pins the truthful hard-hover contract per probe position", () => {
    const root = temporaryDirectory("verter-endurance-hover-contract-");
    const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
    execFileSync(process.execPath, [generator, root, "8", "42"]);
    const corpusHovers = deriveCorpusProbes(root, { maxFiles: 32 }).probes.filter(
      (probe) => probe.kind === "hover" && probe.informational !== true,
    );
    // Synthetic-corpus hovers are script-position (typed) probes: the strong
    // contract holds for all of them (proven green on the real tsserver route
    // by the scale lanes).
    expect(corpusHovers.length).toBeGreaterThan(0);
    for (const probe of corpusHovers) {
      if (probe.kind === "hover") expect(probe.forbidIncludes).toContain("any");
    }

    const laneProbes: EnduranceProbe[] = [];
    for (const lane of ENDURANCE_LANES) {
      const set = buildCarrierSet(3, lane);
      const carriers = set.carriers;
      laneProbes.push(...carrierStormProbes(carriers, lane), ...soakProbes(carriers, lane));
      laneProbes.push(...buildComponentIntegrationProbes(buildComponentFixture(lane)));
    }
    const hardLaneHovers = laneProbes.filter(
      (probe) => probe.kind === "hover" && probe.informational !== true,
    );
    // Vue template-mapped positions: Verter owns the binding NAME (hard +
    // non-empty + name fragment) while the provider owns the TYPE TEXT, which
    // truthfully surfaces `any` on the tsserver route today (the documented
    // provider type-quality gap) — no type fragment may be forbidden, but the
    // hover must stay hard (never demoted to informational).
    const templatePosition = hardLaneHovers.filter(
      (probe) => probe.relativePath.endsWith(".vue") && probe.needle.startsWith("{{"),
    );
    expect(templatePosition.length).toBeGreaterThan(0);
    for (const probe of templatePosition) {
      if (probe.kind !== "hover") continue;
      expect(probe.requireNonEmpty).toBe(true);
      expect(probe.forbidIncludes ?? []).not.toContain("any");
    }
    // Script-position / typed hovers keep the strong contract.
    const typedPosition = hardLaneHovers.filter(
      (probe) => !(probe.relativePath.endsWith(".vue") && probe.needle.startsWith("{{")),
    );
    expect(typedPosition.length).toBeGreaterThan(0);
    for (const probe of typedPosition) {
      if (probe.kind === "hover") expect(probe.forbidIncludes).toContain("any");
    }
  });

  it.each(ENDURANCE_LANES)("heavy-update mutates the complete $id prop surface", (lane) => {
    const fixture = heavyUpdateFixture(lane);
    if (lane.framework === "svelte") {
      expect(fixture.destructure).toContain("label, count, onselect");
      expect(fixture.destructureWithBadge).toContain("label, count, badge, onselect");
      expect(fixture.childContent).toContain(fixture.destructure!);
    } else {
      expect(fixture.destructure).toBeNull();
      expect(fixture.destructureWithBadge).toBeNull();
    }
  });
});

describe("endurance config and runtime attestation", () => {
  function recordingClient(changes: string[]): LspClient {
    return {
      positionEncoding: "utf-16",
      stderr: { text: () => "" },
      documentPositions(text: string) {
        return {
          utf16ToPosition(offset: number) {
            const prefix = text.slice(0, offset);
            const lines = prefix.split("\n");
            return { line: lines.length - 1, character: lines.at(-1)!.length };
          },
        };
      },
      sendNotification(method: string, params: unknown) {
        if (method !== "textDocument/didChange") return;
        changes.push(
          (params as { contentChanges: Array<{ text: string }> }).contentChanges[0].text,
        );
      },
      async sendRequest() {
        // Yield a macrotask turn like real stdio I/O does, so a tight
        // query-worker loop cannot starve the typer's timers.
        await new Promise((resolve) => setImmediate(resolve));
        return null;
      },
    } as unknown as LspClient;
  }

  function soakContext(config: EnduranceConfig, session: EnduranceSession): ScenarioContext {
    return {
      scenario: "soak",
      route: config.route,
      lane: ENDURANCE_LANES[0],
      session,
      config,
      sampler: null,
      providerAttestation: () => ({
        pid: 1,
        kind: "fake",
        evidence: "typeProviderStarted",
        aliveAtEnd: true,
        restartCount: 0,
        providerStartCount: 1,
        reloadProjectsCount: 0,
        restartLogCount: 0,
      }),
    };
  }

  const TYPED_ALPHABET = "abcdefghijklmnopqrstuvwxyz";
  const STABLE_HOVER_PROBE: EnduranceProbe = {
    kind: "hover",
    relativePath: "src/Stable.vue",
    needle: "stable",
    expectIncludes: [],
    informational: true,
    label: "stable hover",
  };

  it("ends a completed soak typing pass without a redundant trailing change", async () => {
    const changes: string[] = [];
    const config = {
      ...loadEnduranceConfig(),
      soakDurationMs: 2,
      typingCps: 10_000,
      windowMs: 1,
    };
    const session = new EnduranceSession(
      recordingClient(changes),
      temporaryDirectory("verter-endurance-deadline-"),
      {
        config,
        recorder: new LatencyRecorder(config.windowMs),
        tracker: new RequestTracker(),
      },
    );
    session.openFile("src/Stable.vue", "stable");
    session.openFile("src/Scratch.vue", "");

    const receipt = await runSoakScenario(soakContext(config, session), {
      probes: [STABLE_HOVER_PROBE],
      typingFile: { relativePath: "src/Scratch.vue", typedText: TYPED_ALPHABET },
      durationMs: 2,
      queryWorkers: 1,
    });

    // A 2ms deadline always starts the first typing pass (Date.now() has 1ms
    // resolution, so the typer's first check cannot observe T+2 in the same
    // synchronous stretch) and always expires DURING it (9 chunks x >=1ms
    // clamped timers > 2ms), so the typer sends exactly one full pass
    // (3 chars per didChange) and the scenario must NOT re-send the
    // identical final text after the loop.
    expect(changes).toHaveLength(Math.ceil(TYPED_ALPHABET.length / 3));
    expect(changes.at(-1)).toBe(TYPED_ALPHABET);
    expect(receiptCoreFailures(receipt)).toEqual([]);
  });

  it("restores the typed buffer with one trailing change when the deadline cuts a soak cycle", async () => {
    const changes: string[] = [];
    const config = {
      ...loadEnduranceConfig(),
      soakDurationMs: 250,
      typingCps: 10_000,
      windowMs: 1,
    };
    const session = new EnduranceSession(
      recordingClient(changes),
      temporaryDirectory("verter-endurance-deadline-cut-"),
      {
        config,
        recorder: new LatencyRecorder(config.windowMs),
        tracker: new RequestTracker(),
      },
    );
    session.openFile("src/Stable.vue", "stable");
    session.openFile("src/Scratch.vue", "");

    const receipt = await runSoakScenario(soakContext(config, session), {
      probes: [STABLE_HOVER_PROBE],
      typingFile: { relativePath: "src/Scratch.vue", typedText: TYPED_ALPHABET },
      durationMs: 250,
      queryWorkers: 1,
    });

    // One full typing pass, one mid-cycle clear (""), then the 250ms deadline
    // expires (the 200ms + 100ms cycle sleeps cannot fire early, so exactly
    // one cycle runs) and the scenario restores the typed text with ONE
    // trailing didChange.
    expect(changes).toHaveLength(Math.ceil(TYPED_ALPHABET.length / 3) + 2);
    expect(changes.at(-2)).toBe("");
    expect(changes.at(-1)).toBe(TYPED_ALPHABET);
    expect(receiptCoreFailures(receipt)).toEqual([]);
  });

  it("preserves a fractional degradation factor", () => {
    const config = loadEnduranceConfig({ VERTER_ENDURANCE_DEGRADATION_FACTOR: "1.5" });
    expect(config.degradationFactor).toBe(1.5);
  });

  it("counts provider restarts and reloadProjects from emitted evidence only", () => {
    const stderr = [
      '[verter-meta-trace] event=start name="tsserver_transport_command" detail="command=reloadProjects args=none"',
      '[verter-meta-trace] event=end name="tsserver_transport_command" detail="command=reloadProjects args=none"',
      "INFO tsserver restarted successfully (attempt 1)",
    ].join("\n");
    expect(parseProviderRuntimeAttestation(stderr)).toEqual({
      restartLogCount: 1,
      reloadProjectsCount: 1,
    });
    expect(parseProviderRuntimeAttestation("provider healthy")).toEqual({
      restartLogCount: 0,
      reloadProjectsCount: 0,
    });
  });

  it("gates reloadProjects at the designed single-recovery bound, never below", () => {
    const base = {
      schemaVersion: 1,
      scenario: "soak",
      route: "tsserver",
      framework: "vue",
      mode: "ts",
      startedAt: "2026-01-01T00:00:00.000Z",
      durationMs: 1,
      requestsSent: 1,
      requestsAnswered: 1,
      requestsCancelled: 0,
      requestsErrored: 0,
      requestsUnanswered: 0,
      editsSent: 0,
      latency: { overall: { p50: 1, p95: 1, max: 1, count: 1 }, windows: [] },
      maxRssBytes: null,
      rssSupported: false,
      providerAliveAtEnd: true,
      providerProcess: { pid: 1, kind: "tsserver", evidence: "typeProviderStarted" },
      restartCount: 0,
      finalSanityPass: true,
      degradationCheck: null,
      typeQuality: { informational: [], settledEmpty: [] },
      config: loadEnduranceConfig(),
      frameworks: { vue: { ts: { requestsSent: 1, requestsUnanswered: 0, editsSent: 0 } } },
      throughputCeiling: null,
      failures: [],
    } as const;
    const clean = { ...base, reloadProjectsCount: 0 };
    expect(receiptCoreFailures(clean as never)).toEqual([]);
    const oneDesignedRecovery = { ...base, reloadProjectsCount: 1 };
    expect(
      receiptCoreFailures(oneDesignedRecovery as never),
      "a single designed singleflight recovery event must not fail the lane",
    ).toEqual([]);
    const storm = { ...base, reloadProjectsCount: 2 };
    expect(
      receiptCoreFailures(storm as never).some((failure) =>
        failure.includes("reloadProjectsCount"),
      ),
      "repeated reloads are the storm class and must fail hard",
    ).toBe(true);
    const restarted = { ...base, restartCount: 1 };
    expect(receiptCoreFailures(restarted as never).length).toBeGreaterThan(0);
  });
});

describe("scale corpus framework/mode parity", () => {
  it("collects both Vue and Svelte carriers", () => {
    const root = temporaryDirectory("verter-endurance-parity-");
    writeFileSync(path.join(root, "A.vue"), "<template />");
    writeFileSync(path.join(root, "B.svelte"), "<div />");
    writeFileSync(path.join(root, "ignore.ts"), "export {};");
    expect(collectCorpusCarrierFiles(root, 10)).toEqual(["A.vue", "B.svelte"]);
  });

  it("the deterministic generator emits every framework/mode lane", () => {
    const root = temporaryDirectory("verter-endurance-generator-");
    const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
    execFileSync(process.execPath, [generator, root, "2", "42"]);
    for (const lane of ENDURANCE_LANES) {
      const extension = lane.framework;
      expect(existsSync(path.join(root, "src", lane.id, `Corpus0.${extension}`))).toBe(true);
      expect(existsSync(path.join(root, "src", lane.id, `App.${extension}`))).toBe(true);
    }
  });

  it("every generated D1 probe references a declared imported prop", () => {
    const root = temporaryDirectory("verter-endurance-generator-refs-");
    const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
    execFileSync(process.execPath, [generator, root, "8", "42"]);
    const derivation = deriveCorpusProbes(root, { maxFiles: 16 });
    expect(derivation.lanes.map((section) => section.renameTarget?.file)).toHaveLength(4);
    expect(derivation.lanes.every((section) => section.renameTarget !== null)).toBe(true);
    for (const section of derivation.lanes) {
      expect(new Set(section.probes.map((probe) => probe.kind))).toEqual(
        new Set(["hover", "completion", "definition"]),
      );
    }
    const d1Probes = derivation.probes.filter(
      (probe) => probe.kind === "completion" && probe.label.includes("component attr completion"),
    );
    expect(d1Probes.length).toBeGreaterThan(0);
    for (const probe of d1Probes) {
      if (probe.kind !== "completion") continue;
      expect(probe.informational).not.toBe(true);
      const source = readFileSync(path.join(root, probe.relativePath), "utf8");
      for (const label of probe.expectLabels) expect(source).not.toContain(label);
      const tag = /^<([A-Z][\w$]*)/.exec(probe.needle)?.[1];
      expect(tag).toBeDefined();
      const importSource = new RegExp(`import\\s+${tag}\\s+from\\s+["']([^"']+)["']`).exec(
        source,
      )?.[1];
      expect(importSource).toBeDefined();
      const target = path.resolve(path.dirname(path.join(root, probe.relativePath)), importSource!);
      const targetSource = readFileSync(target, "utf8");
      for (const label of probe.expectLabels) expect(targetSource).toContain(label);
    }
  });

  it("derives hard Svelte callback and snippet probes in both modes", () => {
    const root = temporaryDirectory("verter-endurance-generator-svelte-constructs-");
    const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
    execFileSync(process.execPath, [generator, root, "8", "42"]);
    const derivation = deriveCorpusProbes(root, { maxFiles: 32 });
    for (const mode of ["ts", "js"] as const) {
      const section = derivation.lanes.find(
        (candidate) => candidate.framework === "svelte" && candidate.mode === mode,
      );
      expect(section).toBeDefined();
      for (const fragment of ["callback-event definition", "snippet definition"]) {
        const probes = section!.probes.filter((probe) => probe.label.includes(fragment));
        expect(probes.length).toBeGreaterThan(0);
        expect(probes.every((probe) => probe.informational !== true)).toBe(true);
      }
      if (mode === "ts") {
        const snippetHovers = section!.probes.filter((probe) =>
          probe.label.includes("snippet hover"),
        );
        expect(snippetHovers.length).toBeGreaterThan(0);
        for (const probe of snippetHovers) {
          if (probe.kind === "hover") expect(probe.forbidIncludes).toContain("any");
        }
      }
    }
  });

  it("best-effort skips unresolved external component imports without throwing", () => {
    const root = temporaryDirectory("verter-endurance-external-imports-");
    mkdirSync(path.join(root, "src"), { recursive: true });
    writeFileSync(
      path.join(root, "src", "Alias.vue"),
      [
        '<script setup lang="ts">',
        'import MissingCard from "@/components/MissingCard.vue";',
        'const heading = "alias";',
        "const headingLength = heading.length;",
        "</script>",
        '<template><MissingCard :title="heading" /><MissingCard /></template>',
      ].join("\n"),
    );
    writeFileSync(
      path.join(root, "src", "Barrel.svelte"),
      [
        '<script lang="ts">',
        '  import MissingCard from "../components";',
        '  let heading = $state("barrel");',
        "  const headingLength = heading.length;",
        "</script>",
        "<MissingCard title={heading} />",
        "<MissingCard />",
      ].join("\n"),
    );

    expect(() => deriveCorpusProbes(root, { maxFiles: 10 })).not.toThrow();
    const derivation = deriveCorpusProbes(root, { maxFiles: 10 });
    expect(
      derivation.probes.filter(
        (probe) => probe.kind === "completion" && probe.label.includes("component attr completion"),
      ),
    ).toHaveLength(0);
  });
});

describe("process-tree RSS sampling", () => {
  const SERVER = 4000;
  const PROVIDER = 4001;
  const table: readonly { pid: number; ppid: number; image: string }[] = [
    { pid: 1, ppid: 0, image: "init" },
    { pid: SERVER, ppid: 1, image: "verter-lsp" },
    { pid: PROVIDER, ppid: SERVER, image: "tsgo" },
  ];
  const deps = (
    rows: readonly { pid: number; ppid: number; image: string }[] | null,
    rss: Record<number, number | null>,
  ): ProcessTreeRssDeps => ({
    snapshotProcessTable: async () => rows,
    readProcessRssBytes: async (pid: number) => rss[pid] ?? null,
  });

  it("sums the server and its provider child when the whole tree is readable", async () => {
    const sample = await sampleProcessTreeRss(
      SERVER,
      deps(table, { [SERVER]: 200, [PROVIDER]: 300 }),
    );
    expect(sample.observable).toBe(true);
    expect(sample.totalBytes).toBe(500);
    expect(sample.unavailable).toBeNull();
    expect(sample.members.map((member) => member.pid).sort()).toEqual([SERVER, PROVIDER]);
  });

  it("reports a missing process table as UNAVAILABLE, not a root-only reading", async () => {
    // Without the table the tree's MEMBERSHIP is unknown: the provider child may
    // be retaining every document version and nothing here can see it. A
    // root-only figure would be flat and would bless exactly that session.
    const sample = await sampleProcessTreeRss(SERVER, deps(null, { [SERVER]: 200 }));
    expect(sample.observable).toBe(false);
    expect(sample.totalBytes).toBeNull();
    expect(sample.unavailable?.kind).toBe("topology-unavailable");
  });

  it("reports a discovered-but-unreadable provider child as UNAVAILABLE", async () => {
    // The member is KNOWN to be in the tree, so omitting its bytes is not a
    // narrower measurement — it is a wrong one.
    const sample = await sampleProcessTreeRss(
      SERVER,
      deps(table, { [SERVER]: 200, [PROVIDER]: null }),
    );
    expect(sample.observable).toBe(false);
    expect(sample.totalBytes).toBeNull();
    expect(sample.unavailable?.kind).toBe("member-unreadable");
    expect(sample.unreadablePids).toEqual([PROVIDER]);
    expect(describeProcessTreeRss(sample)).toContain("UNAVAILABLE");
  });

  it("cannot satisfy the growth bound from an incomplete tree", async () => {
    // The end-to-end leg of the same defect: an incomplete sample must not be
    // able to produce a passing growth verdict at either checkpoint.
    const complete = await sampleProcessTreeRss(
      SERVER,
      deps(table, { [SERVER]: 200, [PROVIDER]: 300 }),
    );
    const partial = await sampleProcessTreeRss(
      SERVER,
      deps(table, { [SERVER]: 200, [PROVIDER]: null }),
    );
    for (const [baseline, final] of [
      [complete, partial],
      [partial, complete],
    ] as const) {
      const verdict = decideChurnGrowth(baseline, final, 1.25, 64 * 1024 ** 2);
      expect(verdict.observable).toBe(false);
      expect(verdict.pass).toBe(false);
      expect(verdict.detail).toContain("NOT evaluated");
    }
  });

  it("cannot satisfy the growth bound when a baseline member left the tree", async () => {
    // A provider that respawned between the checkpoints takes its retained
    // bytes with it, so the surviving figure understates the session. The
    // comparison is refused rather than read as improvement.
    const baseline = await sampleProcessTreeRss(
      SERVER,
      deps(table, { [SERVER]: 200, [PROVIDER]: 300 }),
    );
    const final = await sampleProcessTreeRss(SERVER, deps([table[0], table[1]], { [SERVER]: 210 }));
    expect(final.observable).toBe(true);
    const verdict = decideChurnGrowth(baseline, final, 1.25, 64 * 1024 ** 2);
    expect(verdict.observable).toBe(false);
    expect(verdict.pass).toBe(false);
    expect(verdict.detail).toContain(String(PROVIDER));
  });
});

describe("churn growth verdict", () => {
  const observable = (totalBytes: number) => ({
    observable: true as const,
    totalBytes,
    members: [{ pid: 1, image: "verter-lsp", rssBytes: totalBytes }],
    unreadablePids: [],
    unavailable: null,
    atMs: 0,
  });
  const unobservable = {
    observable: false as const,
    totalBytes: null,
    members: [{ pid: 1, image: null, rssBytes: null }],
    unreadablePids: [1],
    unavailable: { kind: "member-unreadable" as const, detail: "pid 1 unreadable" },
    atMs: 0,
  };
  const MIB = 1024 ** 2;

  it("fails a session that kept every churned document version", () => {
    // 1000 cycles retaining ~300KiB per synced version against a 400MiB
    // baseline: the shape of an insert-only surface store, and the exact
    // failure this lane exists to produce.
    const verdict = decideChurnGrowth(observable(400 * MIB), observable(700 * MIB), 1.25, 64 * MIB);
    expect(verdict.observable).toBe(true);
    expect(verdict.pass).toBe(false);
    expect(verdict.growthBytes).toBe(300 * MIB);
    expect(verdict.detail).toContain("700.0MiB");
  });

  it("passes a bounded session that only wobbles inside the floor", () => {
    const verdict = decideChurnGrowth(observable(400 * MIB), observable(412 * MIB), 1.25, 64 * MIB);
    expect(verdict.pass).toBe(true);
    expect(verdict.ratio).toBeCloseTo(1.03, 2);
  });

  it("reports an unreadable platform as UNAVAILABLE, never as a measured pass", () => {
    const verdict = decideChurnGrowth(observable(400 * MIB), unobservable, 1.25, 64 * MIB);
    expect(verdict.observable).toBe(false);
    expect(verdict.finalBytes).toBeNull();
    expect(verdict.allowedBytes).toBeNull();
    expect(verdict.detail).toContain("UNAVAILABLE");
    // The bound was not evaluated, so it was not satisfied: the lane consuming
    // this verdict must go red for a missing proof, never green.
    expect(verdict.pass).toBe(false);
  });

  it("sizes the churn carrier so one retained version is measurable", () => {
    // A leak is per synced version, so the fixture must be big enough that a
    // thousand of them leave allocator noise behind. 140 blocks is ~32KiB of
    // carrier source before the provider's generated surface.
    const content = churnCarrierContent(140, DEFAULT_ENDURANCE_LANE);
    expect(content.length).toBeGreaterThan(28 * 1024);
    expect(content).toContain("interface ChurnProps {");
    expect(content).toContain("churnField139?: string;");
    expect(content).toContain("{{ churnHeadline }}");
  });
});

describe("churn retained-byte slope verdict", () => {
  const MIB = 1024 ** 2;
  const KIB = 1024;
  const sample = (totalBytes: number, pids: readonly number[] = [1, 2]) => ({
    observable: true as const,
    totalBytes,
    members: pids.map((pid) => ({
      pid,
      image: pid === 1 ? "verter-lsp" : "tsgo",
      rssBytes: totalBytes / pids.length,
    })),
    unreadablePids: [] as number[],
    unavailable: null,
    atMs: 0,
  });
  const unobservable = {
    observable: false as const,
    totalBytes: null,
    members: [{ pid: 1, image: null, rssBytes: null }],
    unreadablePids: [1],
    unavailable: { kind: "member-unreadable" as const, detail: "pid 1 unreadable" },
    atMs: 0,
  };
  /** A flat retained-object set: the byte slope is the only thing under test here. */
  const flatRetention: RetentionReading = {
    liveArtifacts: 4,
    retainedRetiredVersions: 2,
    liveRoots: 1,
    snapshotLeases: 4,
    carrierCandidates: 2,
    publicationLanes: 3,
    semanticNodes: 1,
    semanticMemoEntries: 1,
    unresolvedReach: 1,
    relationProofs: 1,
    relateKeys: 1,
    shapeCacheEntries: 1,
    flowGraphs: 1,
    flowHashEntries: 1,
    flowLoweredEntries: 1,
    mapperFingerprints: 1,
    pinnedBytes: 1_000_000,
    retainedBytes: 500_000,
    refusalsPressure: 0,
  };
  /** Baseline plus `windows` readings, each growing by `bytesPerCycle`. */
  const run = (
    bytesPerCycle: number,
    { cycles = 1000, warmup = 100, windows = 4, quiesced = true } = {},
  ): ChurnCheckpoint[] => {
    const baselineBytes = 75 * MIB;
    const measured = cycles - warmup;
    const checkpoints: ChurnCheckpoint[] = [
      {
        cyclesCompleted: warmup,
        quiesced,
        sample: sample(baselineBytes),
        retention: flatRetention,
      },
    ];
    for (let window = 1; window <= windows; window += 1) {
      const at = warmup + Math.round((measured * window) / windows);
      checkpoints.push({
        cyclesCompleted: at,
        quiesced,
        sample: sample(baselineBytes + (at - warmup) * bytesPerCycle),
        retention: flatRetention,
      });
    }
    return checkpoints;
  };
  const options = { allowedBytesPerCycle: 16 * KIB, minimumCycles: CHURN_ACCEPTANCE_MIN_CYCLES };

  it("rejects a strictly LINEAR leak that the two-endpoint envelope admits", () => {
    // ~85 KiB retained on every post-warm-up cycle against a 75 MiB baseline is
    // 75 MiB of growth over 900 cycles — inside `baseline * 1.25 + 64 MiB`, so
    // the envelope blesses it. It is exactly the shape WSP6-AC1 forbids.
    const checkpoints = run(85 * KIB);
    const envelope = decideChurnGrowth(
      checkpoints[0].sample,
      checkpoints[checkpoints.length - 1].sample,
      1.25,
      64 * MIB,
    );
    expect(
      envelope.pass,
      "the two-endpoint envelope is the weaker oracle this slope check exists to replace",
    ).toBe(true);

    const slope = decideChurnSlope(checkpoints, options);
    expect(slope.observable).toBe(true);
    expect(slope.pass).toBe(false);
    expect(slope.segments.every((segment) => !segment.withinBound)).toBe(true);
    expect(slope.detail).toContain("BREACH");
  });

  it("passes a session that plateaus after the baseline", () => {
    const slope = decideChurnSlope(run(0), options);
    expect(slope.observable).toBe(true);
    expect(slope.pass).toBe(true);
    expect(slope.finalBytesPerCycle).toBe(0);
  });

  it("refuses to evaluate a run shorter than the criterion's 1000 cycles", () => {
    // The lane may be configured down for a smoke run; what it may not do is
    // report a PASS for a bound that run could not have exercised.
    const slope = decideChurnSlope(run(0, { cycles: 200 }), options);
    expect(slope.observable).toBe(false);
    expect(slope.pass).toBe(false);
    expect(slope.cyclesCompleted).toBe(200);
    expect(slope.detail).toContain("NOT evaluated");
    expect(slope.detail).toContain("1000");
  });

  it("refuses to evaluate fewer than two post-baseline windows", () => {
    const checkpoints = run(0, { windows: 4 }).slice(0, 2);
    const slope = decideChurnSlope(checkpoints, options);
    expect(slope.observable).toBe(false);
    expect(slope.pass).toBe(false);
    expect(slope.detail).toContain("at least two later quiesced readings");
  });

  it("refuses a reading taken before the host quiesced", () => {
    const checkpoints = run(0);
    const slope = decideChurnSlope(
      checkpoints.map((checkpoint, index) =>
        index === 2 ? { ...checkpoint, quiesced: false } : checkpoint,
      ),
      options,
    );
    expect(slope.observable).toBe(false);
    expect(slope.pass).toBe(false);
    expect(slope.detail).toContain("quiescence");
  });

  it("refuses an incomplete whole-tree reading rather than reading it as flat", () => {
    const checkpoints = run(0);
    const slope = decideChurnSlope(
      checkpoints.map((checkpoint, index) =>
        index === 3 ? { ...checkpoint, sample: unobservable } : checkpoint,
      ),
      options,
    );
    expect(slope.observable).toBe(false);
    expect(slope.pass).toBe(false);
    expect(slope.detail).toContain("complete whole-tree observations");
  });

  it("refuses when a process left the tree between readings", () => {
    // A respawned provider takes its retained bytes with it; the remaining
    // figure understates the session and must not be read as a plateau.
    const checkpoints = run(0).map((checkpoint, index) =>
      index >= 3 ? { ...checkpoint, sample: sample(40 * MIB, [1]) } : checkpoint,
    );
    const slope = decideChurnSlope(checkpoints, options);
    expect(slope.observable).toBe(false);
    expect(slope.pass).toBe(false);
    expect(slope.detail).toContain("left the tree");
  });
});

describe("churn retained-object verdict", () => {
  const MIB = 1024 ** 2;
  const sample = (totalBytes: number) => ({
    observable: true as const,
    totalBytes,
    members: [{ pid: 1, image: "verter-lsp", rssBytes: totalBytes }],
    unreadablePids: [] as number[],
    unavailable: null,
    atMs: 0,
  });
  const baselineReading: RetentionReading = {
    liveArtifacts: 4,
    retainedRetiredVersions: 2,
    liveRoots: 1,
    snapshotLeases: 4,
    carrierCandidates: 2,
    publicationLanes: 3,
    semanticNodes: 1,
    semanticMemoEntries: 1,
    unresolvedReach: 1,
    relationProofs: 1,
    relateKeys: 1,
    shapeCacheEntries: 1,
    flowGraphs: 1,
    flowHashEntries: 1,
    flowLoweredEntries: 1,
    mapperFingerprints: 1,
    pinnedBytes: 1_000_000,
    retainedBytes: 500_000,
    refusalsPressure: 0,
  };
  /**
   * Baseline plus `windows` quiesced readings over a 1000-cycle run (100 warm-up,
   * 900 measured). `readingAt` shapes each checkpoint's retention from the cycles
   * elapsed since the baseline and the checkpoint's index (0 = baseline).
   */
  const run = (
    readingAt: (sinceBaseline: number, index: number) => RetentionReading | null,
    { cycles = 1000, warmup = 100, windows = 4, quiesced = true } = {},
  ): ChurnCheckpoint[] => {
    const measured = cycles - warmup;
    const checkpoints: ChurnCheckpoint[] = [
      { cyclesCompleted: warmup, quiesced, sample: sample(75 * MIB), retention: readingAt(0, 0) },
    ];
    for (let window = 1; window <= windows; window += 1) {
      const at = warmup + Math.round((measured * window) / windows);
      checkpoints.push({
        cyclesCompleted: at,
        quiesced,
        sample: sample(75 * MIB),
        retention: readingAt(at - warmup, window),
      });
    }
    return checkpoints;
  };
  // The lane's default `churnRetentionObjectsPerCycle`.
  const options = { allowedObjectsPerCycle: 0.25 };

  it("passes flat readings and reports every counter's trend", () => {
    const verdict = decideChurnRetention(
      run(() => baselineReading),
      options,
    );
    expect(verdict.observable).toBe(true);
    expect(verdict.pass).toBe(true);
    expect(verdict.cyclesCompleted).toBe(1000);
    expect(verdict.pressureRefusals).toBe(0);
    expect(verdict.trends.map((trend) => trend.counter)).toEqual([...CHURN_RETENTION_COUNTERS]);
    expect(verdict.trends.every((trend) => trend.perCycle === 0 && trend.withinBound)).toBe(true);
    expect(verdict.detail).not.toContain("BREACH");
  });

  it("fails a retainer that keeps one object per document version", () => {
    // One lease per synced version, never released on close: the counter grows
    // by exactly the cycles run, which no per-cycle amortisation bound admits.
    const verdict = decideChurnRetention(
      run((sinceBaseline) => ({
        ...baselineReading,
        snapshotLeases: baselineReading.snapshotLeases + sinceBaseline,
      })),
      options,
    );
    expect(verdict.observable).toBe(true);
    expect(verdict.pass).toBe(false);
    const leases = verdict.trends.find((trend) => trend.counter === "snapshotLeases");
    expect(leases?.withinBound).toBe(false);
    expect(leases?.perCycle).toBe(1);
    expect(leases?.final).toBe(904);
    expect(verdict.detail).toContain("BREACH");
    expect(verdict.detail).toContain("snapshotLeases");
    // The other counters stayed flat and are named without a breach.
    expect(verdict.trends.filter((trend) => !trend.withinBound)).toHaveLength(1);
  });

  it("passes a bounded sawtooth that returns near its baseline", () => {
    // Superseded versions pile up between sweeps and are released by the next
    // one: a high peak is not a leak as long as the final reading is back near
    // where it started. 2→4 over 900 cycles is ~0.002/cycle, far inside 0.25.
    const retired = [2, 60, 12, 60, 4];
    const verdict = decideChurnRetention(
      run((_since, index) => ({ ...baselineReading, retainedRetiredVersions: retired[index] })),
      options,
    );
    expect(verdict.observable).toBe(true);
    expect(verdict.pass).toBe(true);
    const trend = verdict.trends.find((t) => t.counter === "retainedRetiredVersions");
    expect(trend?.peak).toBe(60);
    expect(trend?.baseline).toBe(2);
    expect(trend?.final).toBe(4);
    expect(trend?.perCycle).toBeCloseTo(2 / 900, 6);
    expect(verdict.detail).toContain("peak 60");
    expect(verdict.detail).not.toContain("BREACH");
  });

  it("refuses when any checkpoint carries no reading, never reading it as zero", () => {
    const verdict = decideChurnRetention(
      run((_since, index) => (index === 2 ? null : baselineReading)),
      options,
    );
    expect(verdict.observable).toBe(false);
    expect(verdict.pass).toBe(false);
    expect(verdict.trends).toEqual([]);
    expect(verdict.pressureRefusals).toBeNull();
    expect(verdict.detail).toContain("UNAVAILABLE");
    expect(verdict.detail).toContain("NOT evaluated");
  });

  it("refuses a reading taken before the host quiesced", () => {
    const checkpoints = run(() => baselineReading).map((checkpoint, index) =>
      index === 3 ? { ...checkpoint, quiesced: false } : checkpoint,
    );
    const verdict = decideChurnRetention(checkpoints, options);
    expect(verdict.observable).toBe(false);
    expect(verdict.pass).toBe(false);
    expect(verdict.detail).toContain("quiescence");
  });

  it("fails when the aggregate account refused reservations for pressure", () => {
    // WSP6.3: pressure outcomes stay explicit and must not occur on the admitted
    // standard corpus. Flat counters do not excuse a refusal.
    const verdict = decideChurnRetention(
      run((_since, index) => ({ ...baselineReading, refusalsPressure: index === 4 ? 3 : 0 })),
      options,
    );
    expect(verdict.observable).toBe(true);
    expect(verdict.pass).toBe(false);
    expect(verdict.pressureRefusals).toBe(3);
    expect(verdict.trends.every((trend) => trend.withinBound)).toBe(true);
    expect(verdict.detail).toContain("pressure");
    expect(verdict.detail).toContain("BREACH");
  });
});

describe("retention reading extraction", () => {
  const reading: RetentionReading = {
    liveArtifacts: 4,
    retainedRetiredVersions: 2,
    liveRoots: 1,
    snapshotLeases: 4,
    carrierCandidates: 2,
    publicationLanes: 3,
    semanticNodes: 1,
    semanticMemoEntries: 1,
    unresolvedReach: 1,
    relationProofs: 1,
    relateKeys: 1,
    shapeCacheEntries: 1,
    flowGraphs: 1,
    flowHashEntries: 1,
    flowLoweredEntries: 1,
    mapperFingerprints: 1,
    pinnedBytes: 1_000_000,
    retainedBytes: 500_000,
    refusalsPressure: 0,
  };

  it("projects a full retention object into a reading", () => {
    expect(extractRetentionReading({ requests: 12, retention: { ...reading } })).toEqual(reading);
  });

  it("returns null when the snapshot reports no retention", () => {
    expect(extractRetentionReading({ requests: 12 })).toBeNull();
    expect(extractRetentionReading({ retention: null })).toBeNull();
    expect(extractRetentionReading(null)).toBeNull();
  });

  it("returns null for a non-numeric field rather than reading it as zero", () => {
    expect(extractRetentionReading({ retention: { ...reading, snapshotLeases: "4" } })).toBeNull();
    expect(extractRetentionReading({ retention: { ...reading, liveRoots: NaN } })).toBeNull();
    const { refusalsPressure: _dropped, ...missingOne } = reading;
    expect(extractRetentionReading({ retention: missingOne })).toBeNull();
  });

  it("ignores fields the reading does not name", () => {
    const projected = extractRetentionReading({
      retention: { ...reading, sweepsRun: 17, lastSweepAtMs: 123 },
    });
    expect(projected).toEqual(reading);
    expect(projected).not.toHaveProperty("sweepsRun");
  });
});
