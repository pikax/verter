import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";

import type {
  StatisticsEvent,
  StatisticsEventType,
  StatisticsSnapshot,
  StatisticsSummary,
} from "@verter/language-shared";

export type StatisticsOptions = {
  enabled?: boolean;
  persistToFile?: boolean;
  filePath?: string;
  maxSessionEntries?: number;
  maxPersistedEntries?: number;
};

type ResolvedStatisticsOptions = Required<Omit<StatisticsOptions, "filePath">> & {
  filePath?: string;
};

type MutableSummary = {
  count: number;
  totalMs: number;
  minMs: number;
  maxMs: number;
};

type PersistedStatistics = {
  version: 1;
  updatedAt?: string;
  byType: Record<string, MutableSummary>;
  byFile: Record<string, MutableSummary>;
  events?: StatisticsEvent[];
};

const DEFAULTS: ResolvedStatisticsOptions = {
  enabled: false,
  persistToFile: false,
  filePath: undefined,
  maxSessionEntries: 500,
  maxPersistedEntries: 2000,
};

export class StatisticsManager {
  private options: ResolvedStatisticsOptions;

  private sessionEvents: StatisticsEvent[] = [];
  private sessionByType = new Map<string, MutableSummary>();
  private sessionByFile = new Map<string, MutableSummary>();

  private globalByType = new Map<string, MutableSummary>();
  private globalByFile = new Map<string, MutableSummary>();
  private persistedEvents: StatisticsEvent[] = [];

  private persistTimer: NodeJS.Timeout | null = null;
  private loadPromise: Promise<void> | null = null;
  private lastPersistedAt: string | undefined;

  constructor(options?: StatisticsOptions) {
    this.options = this.resolveOptions(options);
    if (this.options.persistToFile && this.options.filePath) {
      this.loadPromise = this.loadPersisted();
    }
  }

  updateOptions(options?: StatisticsOptions) {
    const previousPath = this.options.filePath;
    this.options = this.resolveOptions(options);

    if (
      this.options.persistToFile &&
      this.options.filePath &&
      previousPath !== this.options.filePath
    ) {
      this.loadPromise = this.loadPersisted();
    }
  }

  recordEvent(input: {
    type: StatisticsEventType | string;
    uri?: string;
    durationMs: number;
    startedAt?: number;
    meta?: Record<string, unknown>;
  }): StatisticsEvent | null {
    if (!this.options.enabled) return null;

    const startedAt = input.startedAt ?? Date.now() - input.durationMs;
    const event: StatisticsEvent = {
      id: randomUUID(),
      type: input.type,
      uri: input.uri,
      durationMs: input.durationMs,
      startedAt,
      meta: input.meta,
    };

    this.sessionEvents.push(event);
    if (this.sessionEvents.length > this.options.maxSessionEntries) {
      this.sessionEvents.shift();
    }

    this.upsertSummary(this.sessionByType, event.type, event.durationMs);
    if (event.uri) {
      this.upsertSummary(this.sessionByFile, event.uri, event.durationMs);
    }

    this.upsertSummary(this.globalByType, event.type, event.durationMs);
    if (event.uri) {
      this.upsertSummary(this.globalByFile, event.uri, event.durationMs);
    }

    this.persistedEvents.push(event);
    if (this.persistedEvents.length > this.options.maxPersistedEntries) {
      this.persistedEvents.shift();
    }

    this.queuePersist();

    return event;
  }

  async track<T>(
    type: StatisticsEventType | string,
    uri: string | undefined,
    fn: () => Promise<T> | T,
    meta?: Record<string, unknown>,
  ): Promise<T> {
    const start = performance.now();
    try {
      return await fn();
    } finally {
      const durationMs = performance.now() - start;
      this.recordEvent({
        type,
        uri,
        durationMs,
        meta,
        startedAt: Date.now() - durationMs,
      });
    }
  }

  snapshot(params?: {
    includeEvents?: boolean;
    scope?: "session" | "global" | "all";
  }): StatisticsSnapshot {
    const includeSession = params?.scope !== "global";
    const includeGlobal = params?.scope !== "session";
    const includeEvents = params?.includeEvents ?? false;

    return {
      enabled: this.options.enabled,
      session: {
        events: includeSession && includeEvents ? [...this.sessionEvents] : undefined,
        byType: includeSession ? this.toSummaryObject(this.sessionByType) : {},
        byFile: includeSession ? this.toSummaryObject(this.sessionByFile) : {},
      },
      global: includeGlobal
        ? {
            byType: this.toSummaryObject(this.globalByType),
            byFile: this.toSummaryObject(this.globalByFile),
            path: this.options.filePath,
            updatedAt: this.lastPersistedAt,
            eventCount: this.persistedEvents.length,
          }
        : undefined,
    };
  }

  private resolveOptions(options?: StatisticsOptions): ResolvedStatisticsOptions {
    const merged: ResolvedStatisticsOptions = {
      ...DEFAULTS,
      ...options,
    };

    if (merged.persistToFile && merged.filePath) {
      merged.filePath = resolve(merged.filePath);
    }

    return merged;
  }

  private upsertSummary(map: Map<string, MutableSummary>, key: string, durationMs: number) {
    const current = map.get(key);
    if (!current) {
      map.set(key, {
        count: 1,
        totalMs: durationMs,
        minMs: durationMs,
        maxMs: durationMs,
      });
      return;
    }

    current.count += 1;
    current.totalMs += durationMs;
    current.minMs = Math.min(current.minMs, durationMs);
    current.maxMs = Math.max(current.maxMs, durationMs);
  }

  private toSummaryObject(map: Map<string, MutableSummary>): Record<string, StatisticsSummary> {
    const output: Record<string, StatisticsSummary> = {};
    for (const [key, value] of map.entries()) {
      output[key] = {
        count: value.count,
        totalMs: value.totalMs,
        minMs: value.minMs,
        maxMs: value.maxMs,
        averageMs: value.count ? value.totalMs / value.count : 0,
      };
    }
    return output;
  }

  private queuePersist() {
    if (!this.options.persistToFile || !this.options.filePath) return;

    if (this.persistTimer) {
      clearTimeout(this.persistTimer);
    }

    this.persistTimer = setTimeout(() => {
      this.flushPersisted().catch((err) => {
        console.error("[statistics] Failed to persist statistics", err);
      });
    }, 50);
  }

  private async flushPersisted() {
    if (!this.options.persistToFile || !this.options.filePath) return;
    if (this.loadPromise) {
      await this.loadPromise;
      this.loadPromise = null;
    }

    const payload: PersistedStatistics = {
      version: 1,
      updatedAt: new Date().toISOString(),
      byType: this.toPersistable(this.globalByType),
      byFile: this.toPersistable(this.globalByFile),
      events: this.persistedEvents.slice(-this.options.maxPersistedEntries),
    };

    await mkdir(dirname(this.options.filePath), { recursive: true });
    await writeFile(this.options.filePath, JSON.stringify(payload, null, 2), "utf-8");
    this.lastPersistedAt = payload.updatedAt;
  }

  private toPersistable(map: Map<string, MutableSummary>): Record<string, MutableSummary> {
    const output: Record<string, MutableSummary> = {};
    for (const [key, value] of map.entries()) {
      output[key] = { ...value };
    }
    return output;
  }

  private async loadPersisted() {
    if (!this.options.filePath || !this.options.persistToFile) return;
    if (!existsSync(this.options.filePath)) return;

    try {
      const raw = await readFile(this.options.filePath, "utf-8");
      const parsed = JSON.parse(raw) as PersistedStatistics;

      for (const [key, summary] of Object.entries(parsed.byType ?? {})) {
        this.globalByType.set(key, { ...summary });
      }
      for (const [key, summary] of Object.entries(parsed.byFile ?? {})) {
        this.globalByFile.set(key, { ...summary });
      }
      if (parsed.events && Array.isArray(parsed.events)) {
        this.persistedEvents = parsed.events.slice(-this.options.maxPersistedEntries);
      }
      this.lastPersistedAt = parsed.updatedAt;
    } catch (e) {
      console.error(
        `[statistics] Failed to read persisted statistics from ${this.options.filePath}`,
        e,
      );
    }
  }
}
