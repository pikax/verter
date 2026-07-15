import { StderrBuffer } from "@verter/lsp-test-client";
import { describe, expect, it } from "vitest";

import {
  GET_STATISTICS_METHOD,
  TYPE_PROVIDER_SYNC_COMPLETE_METHOD,
  VERTER_READY_METHOD,
  awaitRawLspStartup,
  createWarnLineDrainer,
  type StartupLspClient,
} from "../src/core/startupGate.js";
import type { QuiescenceCounters } from "../src/core/quiescence.js";

const C = (compile: number, upsert: number, cacheHits: number): QuiescenceCounters => ({
  compile,
  upsert,
  cacheHits,
});

/**
 * A structural stand-in for `@verter/lsp-test-client`'s `LspClient` exposing only
 * the slice the startup gate consumes. `getStatistics` snapshots are synthesized
 * from a live counters source; `onSendRequest` lets a test inject notifications at
 * a precise poll index (to model a generation superseding mid-quiescence).
 */
class FakeStartupClient implements StartupLspClient {
  private readonly handlers = new Map<string, Array<(params: any) => void>>();
  // A real StderrBuffer so the fake's `stderr` matches the surface the gate
  // consumes on a live LspClient (`text()` over the same partial-line semantics).
  readonly stderr = new StderrBuffer();
  onSendRequest?: (callIndex: number) => void;
  private statsCalls = 0;

  constructor(private readonly counters: () => QuiescenceCounters) {}

  onNotification(method: string, handler: (params: any) => void): void {
    const list = this.handlers.get(method) ?? [];
    list.push(handler);
    this.handlers.set(method, list);
  }

  offNotification(method: string, handler: (params: any) => void): void {
    const list = this.handlers.get(method);
    if (!list) return;
    const idx = list.indexOf(handler);
    if (idx >= 0) list.splice(idx, 1);
  }

  handlerCount(method: string): number {
    return this.handlers.get(method)?.length ?? 0;
  }

  emit(method: string, params: unknown): void {
    for (const handler of [...(this.handlers.get(method) ?? [])]) handler(params);
  }

  async sendRequest<T = any>(method: string, _params?: any, _timeout?: number): Promise<T> {
    if (method !== GET_STATISTICS_METHOD) throw new Error(`unexpected request: ${method}`);
    const idx = this.statsCalls++;
    this.onSendRequest?.(idx);
    const c = this.counters();
    const snapshot = {
      enabled: true,
      session: {
        byType: {
          "host:compile": { count: c.compile },
          "host:upsert": { count: c.upsert },
          "host:cache_hits": { count: c.cacheHits },
        },
        byFile: {},
      },
    };
    return snapshot as T;
  }
}

const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

function makeClock(step: number) {
  let t = 0;
  return () => {
    const v = t;
    t += step;
    return v;
  };
}

/** A virtual clock whose `sleep` advances `now` — models wall-clock deterministically. */
function virtualClock() {
  let t = 0;
  return {
    now: (): number => t,
    sleep: (ms: number): Promise<void> => {
      t += ms;
      return Promise.resolve();
    },
  };
}

describe("createWarnLineDrainer", () => {
  it("observes a scanner/drain/sync WARN split across two stderr chunks exactly once", () => {
    // The regression: a line-count cursor over `lines()` advances past the
    // unterminated partial `"WARN workspace_"`, then slices the COMPLETED line
    // away — a genuine new WARN is dropped and quiescence wrongly passes.
    let text = "";
    const drain = createWarnLineDrainer({ text: () => text });

    // Chunk 1 delivers only the start of the WARN line (no newline yet).
    text += "WARN workspace_";
    expect(drain()).toEqual([]); // partial line: nothing has completed yet

    // Chunk 2 completes the SAME line.
    text += "scanner busy\n";
    expect(drain()).toEqual(["WARN workspace_scanner busy"]); // completed → seen once

    // Idempotent: a completed line is never re-emitted on a later drain.
    expect(drain()).toEqual([]);
  });

  it("emits each completed WARN once and ignores non-warn / incomplete lines", () => {
    let text = "";
    const drain = createWarnLineDrainer({ text: () => text });

    // Pre-window flush returns nothing.
    expect(drain()).toEqual([]);

    // A non-matching INFO line and a complete drain WARN in one chunk.
    text += "INFO workspace ready\nWARN workspace_scanner draining\n";
    expect(drain()).toEqual(["WARN workspace_scanner draining"]);

    // A WARN without a watched keyword does not reset quiescence.
    text += "WARN something unrelated\n";
    expect(drain()).toEqual([]);

    // A trailing partial WARN is withheld until its newline arrives.
    text += "WARN type provider sync";
    expect(drain()).toEqual([]);
    text += " stalled\n";
    expect(drain()).toEqual(["WARN type provider sync stalled"]);
  });

  it("restarts its cursor if the buffer is cleared below it", () => {
    let text = "WARN workspace_scanner busy\n";
    const stderr = { text: () => text };
    const drain = createWarnLineDrainer(stderr);
    expect(drain()).toEqual(["WARN workspace_scanner busy"]);

    // A clear() shrinks the buffer; the drainer must not slice past its end.
    text = "WARN drain restarted\n";
    expect(drain()).toEqual(["WARN drain restarted"]);
  });
});

describe("awaitRawLspStartup", () => {
  it("does NOT signal ready on `$/verter/ready` alone — it waits for the matching sync", async () => {
    // A probe must not proceed on `ready` before the matching-generation
    // `typeProviderSyncComplete`. With only `ready`, the gate must never resolve;
    // here it times out waiting for the matched generation.
    const fake = new FakeStartupClient(() => C(1, 1, 1));
    const p = awaitRawLspStartup(fake, { readyTimeoutMs: 40 });
    fake.emit(VERTER_READY_METHOD, { gen: 1 });
    await expect(p).rejects.toThrow(/matched init generation/);
    // Cleanup: subscriptions are removed even on the timeout path.
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(0);
    expect(fake.handlerCount(TYPE_PROVIDER_SYNC_COMPLETE_METHOD)).toBe(0);
  });

  it("resolves once ready+sync match a generation AND the host quiesces", async () => {
    const fake = new FakeStartupClient(() => C(10, 5, 2)); // stable counters
    let resolved = false;
    const p = awaitRawLspStartup(fake, {
      readyTimeoutMs: 1000,
      quiescence: { intervalMs: 1, timeoutMs: 1000 },
      sleep: () => Promise.resolve(),
      now: makeClock(1),
    }).then((r) => {
      resolved = true;
      return r;
    });

    await flush();
    fake.emit(VERTER_READY_METHOD, { gen: 1 });
    await flush();
    // Negative: ready alone must not have resolved the gate.
    expect(resolved).toBe(false);

    fake.emit(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, { gen: 1 });
    const r = await p;
    expect(r.matchedGeneration).toBe(1);
    expect(r.quiescence.quiesced).toBe(true);
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(0);
  });

  it("bounds quiescence by the remaining ready budget, not the full quiescence timeout", async () => {
    // readyTimeoutMs is the TOTAL budget to reach a quiesced matched generation.
    // With counters that never settle, quiescence can only end at a deadline; a
    // match must not then wait the (much larger) quiescence.timeoutMs and overrun.
    let n = 0;
    const fake = new FakeStartupClient(() => C(n++, 0, 0)); // changes every poll → never quiesces
    const clock = virtualClock();
    const READY = 100;
    const p = awaitRawLspStartup(fake, {
      readyTimeoutMs: READY,
      statisticsTimeoutMs: 10_000,
      quiescence: { intervalMs: 25, timeoutMs: 10_000 }, // 100x the ready budget
      sleep: clock.sleep,
      now: clock.now,
    });
    fake.emit(VERTER_READY_METHOD, { gen: 1 });
    fake.emit(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, { gen: 1 });

    await expect(p).rejects.toThrow(/raw LSP startup/);
    // The total wall clock is a HARD cap: it lands AT the deadline, not the
    // 10000ms quiescence timeout (the bug overruns to ~10000) and not a poll
    // interval past it. With intervalMs dividing READY this is exactly READY.
    expect(clock.now()).toBeLessThanOrEqual(READY);
    // Subscriptions are removed on the deadline path too.
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(0);
    expect(fake.handlerCount(TYPE_PROVIDER_SYNC_COMPLETE_METHOD)).toBe(0);
  });

  it("does NOT overshoot the deadline by a poll interval on a non-divisible interval", async () => {
    // The discriminating case: readyTimeoutMs=100 with intervalMs=60 does NOT
    // divide the budget. The buggy uncapped sleep advances 0→60→120 and only
    // re-checks the budget at the TOP of the next poll, so the gate fails at ~120
    // — a poll interval PAST the deadline. Capping the inter-poll sleep to the
    // remaining budget makes the loop fail AT the deadline (~100).
    let n = 0;
    const fake = new FakeStartupClient(() => C(n++, 0, 0)); // changes every poll → never quiesces
    const clock = virtualClock();
    const READY = 100;
    const INTERVAL = 60; // oversized vs the residual budget AND non-divisible
    const p = awaitRawLspStartup(fake, {
      readyTimeoutMs: READY,
      statisticsTimeoutMs: 10_000,
      quiescence: { intervalMs: INTERVAL, timeoutMs: 10_000 },
      sleep: clock.sleep,
      now: clock.now,
    });
    fake.emit(VERTER_READY_METHOD, { gen: 1 });
    fake.emit(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, { gen: 1 });

    await expect(p).rejects.toThrow(/raw LSP startup/);
    // HARD cap: lands AT ~READY (=100), NOT READY+INTERVAL slack (the bug → ~120).
    expect(clock.now()).toBeLessThanOrEqual(READY);
    // Subscriptions are removed on the deadline path too.
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(0);
    expect(fake.handlerCount(TYPE_PROVIDER_SYNC_COMPLETE_METHOD)).toBe(0);
  });

  it("aborts the pre-match ready+sync wait promptly and removes handlers", async () => {
    // An abort DURING the pre-match wait (before ready/sync arrive) must reject
    // promptly — not be ignored until the ready timeout (here 60s) fires — and
    // remove the notification handlers so a cancelled harness can tear down.
    const fake = new FakeStartupClient(() => C(1, 1, 1));
    const ac = new AbortController();
    const p = awaitRawLspStartup(fake, { readyTimeoutMs: 60_000, signal: ac.signal });
    await flush();
    // We are waiting pre-match with both subscriptions registered.
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(1);
    expect(fake.handlerCount(TYPE_PROVIDER_SYNC_COMPLETE_METHOD)).toBe(1);

    ac.abort();
    // Settle vs a short timer: the bug leaves `p` pending (→ "pending") until the
    // 60s ready timeout; the fix rejects with an abort error well before that.
    const settled = await Promise.race([
      p.then(
        () => "resolved",
        (e: unknown) => (e instanceof Error && /abort/i.test(e.message) ? "rejected" : "other"),
      ),
      new Promise<string>((resolve) => {
        const t = setTimeout(() => resolve("pending"), 250);
        (t as { unref?: () => void }).unref?.();
      }),
    ]);
    expect(settled).toBe("rejected");
    // No handler leak on the abort path.
    expect(fake.handlerCount(VERTER_READY_METHOD)).toBe(0);
    expect(fake.handlerCount(TYPE_PROVIDER_SYNC_COMPLETE_METHOD)).toBe(0);
  });

  it("re-arms on a generation that supersedes mid-quiescence and resolves at the newest", async () => {
    const fake = new FakeStartupClient(() => C(7, 7, 7)); // stable across the run
    // Inject a superseding generation during the gen-1 quiescence poll loop.
    fake.onSendRequest = (idx) => {
      if (idx === 1) fake.emit(VERTER_READY_METHOD, { gen: 2 });
      if (idx === 2) fake.emit(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, { gen: 2 });
    };
    const p = awaitRawLspStartup(fake, {
      readyTimeoutMs: 1000,
      quiescence: { intervalMs: 1, timeoutMs: 1000 },
      sleep: () => Promise.resolve(),
      now: makeClock(1),
    });
    fake.emit(VERTER_READY_METHOD, { gen: 1 });
    fake.emit(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, { gen: 1 });

    const r = await p;
    // Newest-wins: the gate must report the superseding generation, not gen 1.
    expect(r.matchedGeneration).toBe(2);
    expect(r.quiescence.quiesced).toBe(true);
  });
});
