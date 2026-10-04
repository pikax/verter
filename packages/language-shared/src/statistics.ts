export type StatisticsEventType =
  | "diagnostics"
  | "diagnostics:document"
  | "diagnostics:style"
  | "read-file"
  | "parse"
  | "process";

export type StatisticsEvent = {
  id: string;
  type: StatisticsEventType | string;
  uri?: string;
  durationMs: number;
  startedAt: number;
  meta?: Record<string, unknown>;
};

export type StatisticsSummary = {
  count: number;
  totalMs: number;
  averageMs: number;
  minMs: number;
  maxMs: number;
};

export type StatisticsSnapshot = {
  diagnostics?: Record<string, { version: number; ready: boolean }>;
  enabled: boolean;
  session: {
    events?: StatisticsEvent[];
    byType: Record<string, StatisticsSummary>;
    byFile: Record<string, StatisticsSummary>;
  };
  global?: {
    byType: Record<string, StatisticsSummary>;
    byFile: Record<string, StatisticsSummary>;
    path?: string;
    updatedAt?: string;
    eventCount?: number;
  };
};

export type StatisticsRequestParams = {
  includeEvents?: boolean;
  scope?: "session" | "global" | "all";
  /**
   * The open document whose diagnostics status the caller is waiting on. The
   * server records it as demand for that document, so its owed work is served
   * ahead of unrelated work.
   */
  uri?: string;
};
