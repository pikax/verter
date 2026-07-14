import { describe, it, expect } from "vitest";
import { coldResolveCompanion, COLD_READ_BLOCK_CAP_MS } from "./coldRead";
import type { CarrierStoreReader, ReadyFile } from "@verter/language-shared";

/**
 * A minimal in-memory stand-in for `CarrierStoreReader` exposing only the two
 * methods `coldResolveCompanion` touches (`readyFile`, `lastGoodBlobFor`). It
 * lets a test flip readiness mid-block deterministically.
 */
class FakeReader {
  private ready = new Map<string, ReadyFile>();
  private lastGood = new Map<string, string>();
  /** Becomes-ready after N `readyFile` polls (simulates a mid-block publish). */
  private becomeReadyAfter = new Map<string, { count: number; entry: ReadyFile }>();
  private polls = new Map<string, number>();

  setReady(provider: string, entry: ReadyFile): void {
    this.ready.set(provider, entry);
  }
  setLastGood(provider: string, content: string): void {
    this.lastGood.set(provider, content);
  }
  becomesReadyAfter(provider: string, polls: number, entry: ReadyFile): void {
    this.becomeReadyAfter.set(provider, { count: polls, entry });
  }

  readyFile(provider: string): ReadyFile | undefined {
    if (this.ready.has(provider)) {
      return this.ready.get(provider);
    }
    const pending = this.becomeReadyAfter.get(provider);
    if (pending) {
      const seen = (this.polls.get(provider) ?? 0) + 1;
      this.polls.set(provider, seen);
      if (seen >= pending.count) {
        return pending.entry;
      }
    }
    return undefined;
  }

  lastGoodBlobFor(provider: string): string | undefined {
    return this.lastGood.get(provider);
  }
}

const sampleReady = (): ReadyFile => ({
  content_hash: "aaaa",
  version: 1,
  script_kind: "TSX",
  role: "CarrierIde",
  map_hash: "bbbb",
  blob_rel: "blobs/blake3-aaaa.tsx",
});

const asReader = (f: FakeReader) => f as unknown as CarrierStoreReader;

describe("coldResolveCompanion", () => {
  it("returns ready immediately when already published (no block)", () => {
    const f = new FakeReader();
    f.setReady("d:/ws/src/A.vue.tsx", sampleReady());
    const start = Date.now();
    const result = coldResolveCompanion(asReader(f), "d:/ws/src/A.vue.tsx");
    expect(result.kind).toBe("ready");
    // No bounded-block happened.
    expect(Date.now() - start).toBeLessThan(COLD_READ_BLOCK_CAP_MS);
  });

  it("returns last-good when not ready but a previous blob exists (last-good beats blocking)", () => {
    const f = new FakeReader();
    f.setLastGood("d:/ws/src/A.vue.tsx", "export const A = 1;");
    const start = Date.now();
    const result = coldResolveCompanion(asReader(f), "d:/ws/src/A.vue.tsx");
    expect(result).toEqual({ kind: "lastGood", content: "export const A = 1;" });
    // No bounded-block happened.
    expect(Date.now() - start).toBeLessThan(COLD_READ_BLOCK_CAP_MS);
  });

  it("bounded-blocks then returns negative on timeout (no ready, no last-good)", () => {
    const f = new FakeReader();
    const cap = 40;
    const start = Date.now();
    const result = coldResolveCompanion(asReader(f), "d:/ws/src/Cold.vue.tsx", cap, 10);
    const elapsed = Date.now() - start;
    expect(result).toEqual({ kind: "negative" });
    // It blocked roughly up to the cap, and never beyond it.
    expect(elapsed).toBeGreaterThanOrEqual(cap - 15);
    expect(elapsed).toBeLessThan(cap + 80);
  });

  it("returns the blob the moment the companion becomes ready mid-block", () => {
    const f = new FakeReader();
    // Becomes ready on the 2nd manifest poll, well within the cap.
    f.becomesReadyAfter("d:/ws/src/Warm.vue.tsx", 2, sampleReady());
    const result = coldResolveCompanion(asReader(f), "d:/ws/src/Warm.vue.tsx", 200, 5);
    expect(result.kind).toBe("ready");
    if (result.kind === "ready") {
      expect(result.readyFile.blob_rel).toBe("blobs/blake3-aaaa.tsx");
    }
  });

  it("never blocks past the cap even when the companion never warms", () => {
    const f = new FakeReader();
    const cap = 60;
    const start = Date.now();
    coldResolveCompanion(asReader(f), "d:/ws/src/NeverWarms.vue.tsx", cap, 10);
    expect(Date.now() - start).toBeLessThan(cap + 80);
  });
});
