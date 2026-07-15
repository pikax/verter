import { describe, expect, it } from "vitest";

import {
  CollectingSink,
  EditBuffer,
  collectRecovery,
  decideRecovery,
  type CollectorLspClient,
  type ProbeSnapshot,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 3,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "recovery",
  version: 8,
  anchor: "cursor",
};

const snapshot = (over: Partial<ProbeSnapshot> = {}): ProbeSnapshot => ({
  completionLabels: ["count", "name"],
  hoverLabel: "const count: number",
  diagnosticKeys: ["code:2304"],
  ...over,
});

describe("decideRecovery — pass when baseline returns within threshold with no correlated signal", () => {
  it("PASSES when the post-burst state equals the baseline, within threshold, no signal", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot(),
      afterBurst: snapshot(),
      recoveredMs: 120,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(true);
    expect(event.signal).toBe("recovery_baseline_restored");
    expect((event.data as { equivalent?: boolean }).equivalent).toBe(true);
  });

  it("treats label-set order as irrelevant (set-equivalent snapshots pass)", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot({ completionLabels: ["count", "name"] }),
      afterBurst: snapshot({ completionLabels: ["name", "count"] }),
      recoveredMs: 50,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(true);
  });

  it("does NOT treat [count,name] and [count,count] as equivalent (true set semantics, not length+membership)", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot({ completionLabels: ["count", "name"] }),
      afterBurst: snapshot({ completionLabels: ["count", "count"] }),
      recoveredMs: 50,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(false);
    expect((event.data as { equivalent?: boolean }).equivalent).toBe(false);
  });

  it("FAILS when completions do not return to the baseline set", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot({ completionLabels: ["count", "name"] }),
      afterBurst: snapshot({ completionLabels: ["count"] }),
      recoveredMs: 50,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(false);
    expect(event.severity).toBe("userVisible");
    expect((event.data as { equivalent?: boolean }).equivalent).toBe(false);
  });

  it("FAILS when a correlated signal appears (and adopts its severity if more severe)", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot(),
      afterBurst: snapshot(),
      recoveredMs: 50,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [{ severity: "critical", signal: "server_error" }],
    });
    expect(event.ok).toBe(false);
    expect(event.severity).toBe("critical");
  });

  it("FAILS when recovery exceeds the time threshold", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot(),
      afterBurst: snapshot(),
      recoveredMs: 900,
      maxRecoveryMs: 500,
      quiesced: true,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(false);
    expect((event.data as { withinThreshold?: boolean }).withinThreshold).toBe(false);
  });

  it("FAILS when quiescence was never reached after the burst", () => {
    const event = decideRecovery({
      key,
      baseline: snapshot(),
      afterBurst: snapshot(),
      quiesced: false,
      correlatedSignals: [],
    });
    expect(event.ok).toBe(false);
    expect((event.data as { quiesced?: boolean }).quiesced).toBe(false);
  });
});

/**
 * A fake LSP client recording issued requests and returning canned completion / hover
 * responses, with a hook to publish diagnostics — so {@link collectRecovery}'s OWN
 * probe-driving (it builds the snapshot from real `textDocument/completion` +
 * `textDocument/hover` + published diagnostics, NOT an opaque caller callback) is
 * verifiable without spawning a server.
 */
class FakeRecoveryClient implements CollectorLspClient {
  readonly positionEncoding = "utf-16" as const;
  readonly serverCapabilities = {};
  readonly stderr = { text: (): string => "" };
  readonly issued: { method: string }[] = [];
  private completionCalls = 0;
  private readonly diagHandlers = new Set<(p: unknown) => void>();

  constructor(
    private readonly completionSets: readonly (readonly string[])[],
    private readonly hoverContents: string,
  ) {}

  async sendRequest<T = unknown>(method: string): Promise<T> {
    this.issued.push({ method });
    if (method === "textDocument/completion") {
      const idx = Math.min(this.completionCalls, this.completionSets.length - 1);
      this.completionCalls += 1;
      const labels = this.completionSets[idx];
      return { items: labels.map((label) => ({ label, kind: 10 })), isIncomplete: false } as T;
    }
    if (method === "textDocument/hover") {
      return { contents: "```typescript\n" + this.hoverContents + "\n```" } as T;
    }
    return null as T;
  }
  sendNotification(): void {}
  onNotification(method: string, handler: (p: unknown) => void): void {
    if (method === "textDocument/publishDiagnostics") this.diagHandlers.add(handler);
  }
  offNotification(method: string, handler: (p: unknown) => void): void {
    if (method === "textDocument/publishDiagnostics") this.diagHandlers.delete(handler);
  }
  /** Simulate a `publishDiagnostics` push (the way diagnostics settle during quiescence). */
  publishDiagnostics(uri: string, diagnostics: unknown[]): void {
    for (const handler of this.diagHandlers) handler({ uri, diagnostics });
  }
}

const TEXT = 'const greeting = "hi"\nconst echo = greeting\n';
const tailOffset = TEXT.indexOf("greeting", 15) + "greeting".length; // end of the usage on line 2
const burst = [{ kind: "insert", anchor: "tail", text: ".length", burst: true }] as const;

describe("collectRecovery — drives the real completion/hover/diagnostics probes itself", () => {
  it("issues completion + hover at the anchor and reads published diagnostics into the snapshot", async () => {
    const uri = "file:///recovery.vue";
    const client = new FakeRecoveryClient([["count", "name"]], "const echo: string");
    const sink = new CollectingSink();
    const buffer = new EditBuffer(TEXT, { tail: tailOffset });
    const unusedDiagnostic = {
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 10 } },
      severity: 2,
      code: "6133",
      message: "'echo' is declared but its value is never read.",
    };

    await collectRecovery({
      client,
      sink,
      uri,
      buffer,
      burst: [...burst],
      scenario: "hermetic",
      probe: "recovery",
      anchor: "tail",
      provider: "tsgo",
      maxRecoveryMs: 5_000,
      // Diagnostics settle during quiescence — the gate is the reused quiescer callback.
      awaitQuiescence: async () => {
        client.publishDiagnostics(uri, [unusedDiagnostic]);
        return true;
      },
      correlatedSignals: () => [],
    });

    // The collector drove BOTH probes ITSELF for the baseline AND the post-burst snapshot
    // (no opaque snapshot callback): two completion requests and two hover requests.
    expect(client.issued.filter((r) => r.method === "textDocument/completion")).toHaveLength(2);
    expect(client.issued.filter((r) => r.method === "textDocument/hover")).toHaveLength(2);

    const recovery = sink.events.find((e) => e.collector === "recovery");
    expect(recovery).toBeDefined();
    const data = recovery?.data as { baseline: ProbeSnapshot; afterBurst: ProbeSnapshot };
    // The snapshot reflects the REAL probe responses — not a constant-empty stand-in.
    expect(data.baseline.completionLabels).toEqual(["count", "name"]);
    expect(data.baseline.hoverLabel).toBe("const echo: string");
    expect(data.baseline.diagnosticKeys).toEqual(["code:6133"]);
    expect(data.afterBurst.completionLabels).toEqual(["count", "name"]);
    // A real settle that returned to a baseline-equivalent state passes.
    expect(recovery?.ok).toBe(true);
  });

  it("fails recovery (ok:false) when the driven post-burst completion set drifts from the baseline", async () => {
    const uri = "file:///recovery.vue";
    // baseline completion = [count,name]; post-burst = [count] — a real drift the probes observe.
    const client = new FakeRecoveryClient([["count", "name"], ["count"]], "const echo: string");
    const sink = new CollectingSink();
    const buffer = new EditBuffer(TEXT, { tail: tailOffset });

    await collectRecovery({
      client,
      sink,
      uri,
      buffer,
      burst: [...burst],
      scenario: "hermetic",
      probe: "recovery",
      anchor: "tail",
      provider: "tsgo",
      awaitQuiescence: async () => true,
      correlatedSignals: () => [],
    });

    const recovery = sink.events.find((e) => e.collector === "recovery");
    expect(recovery?.ok).toBe(false);
    expect((recovery?.data as { equivalent?: boolean }).equivalent).toBe(false);
    expect((recovery?.data as { afterBurst?: ProbeSnapshot }).afterBurst?.completionLabels).toEqual(
      ["count"],
    );
  });
});
