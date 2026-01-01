/**
 * @ai-generated - This test file was generated with AI assistance.
 * Covers StatisticsManager behaviour:
 * - Aggregates events by type and file
 * - track helper records timed work
 * - Persistence writes aggregates to disk
 */

import { describe, expect, it, vi } from "vitest";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { StatisticsManager } from "./StatisticsManager";

describe("StatisticsManager", () => {
  it("groups events by type and file when enabled", () => {
    const stats = new StatisticsManager({ enabled: true, maxSessionEntries: 10 });

    stats.recordEvent({ type: "read-file", uri: "/foo.vue", durationMs: 10 });
    stats.recordEvent({ type: "read-file", uri: "/foo.vue", durationMs: 30 });

    const snapshot = stats.snapshot({ includeEvents: true });

    expect(snapshot.enabled).toBe(true);
    expect(snapshot.session.byType["read-file"].count).toBe(2);
    expect(snapshot.session.byType["read-file"].totalMs).toBe(40);
    expect(snapshot.session.byType["read-file"].averageMs).toBeCloseTo(20);
    expect(snapshot.session.byFile["/foo.vue"].count).toBe(2);
    expect(snapshot.session.events?.length).toBe(2);
  });

  it("tracks async work and records duration", async () => {
    const stats = new StatisticsManager({ enabled: true });

    const result = await stats.track("diagnostics", "/bar.vue", async () => {
      await new Promise((resolve) => setTimeout(resolve, 5));
      return 42;
    });

    expect(result).toBe(42);
    const snapshot = stats.snapshot();
    expect(snapshot.session.byType["diagnostics"].count).toBe(1);
    expect(snapshot.session.byFile["/bar.vue"].count).toBe(1);
  });

  it("persists aggregates when configured", async () => {
    vi.useFakeTimers();
    const folder = await mkdtemp(join(tmpdir(), "verter-stats-"));
    const filePath = join(folder, "stats.json");

    const stats = new StatisticsManager({
      enabled: true,
      persistToFile: true,
      filePath,
      maxPersistedEntries: 10,
    });

    try {
      stats.recordEvent({ type: "diagnostics", uri: "/persist.vue", durationMs: 7 });

      await vi.runAllTimersAsync();
      await (stats as any).flushPersisted();
      const raw = await readFile(filePath, "utf-8");
      const persisted = JSON.parse(raw);

      expect(persisted.byType["diagnostics"].count).toBe(1);
      expect(persisted.byFile["/persist.vue"].totalMs).toBe(7);
    } finally {
      vi.useRealTimers();
    }
  });
});
