/**
 * JBT1H.3 / JBT1H-AC2: measure the whole process tree including the TypeScript
 * provider. Excluding the provider invalidates the retained-memory row.
 * Unavailable metrics stay labelled unknown — never guessed zeros.
 */

import {
  unknownMetric,
  type Metric,
  type ProcessTreeSnapshot,
  type TypeProviderStatus,
} from "./types.js";

export type MemoryRowStatus = "valid" | "invalidated" | "unknown";

export interface MemoryRow {
  readonly status: MemoryRowStatus;
  readonly retainedMemory: Metric;
  readonly providerCpu: Metric;
  readonly providerWall: Metric;
  readonly outboundBytes: Metric;
  readonly reason: string;
  readonly typeProviderStatus: TypeProviderStatus;
}

export function providerRequiredForMemory(tree: ProcessTreeSnapshot): {
  readonly ok: boolean;
  readonly reason: string;
} {
  if (tree.typeProviderStatus === "excluded" || tree.typeProviderStatus === "missing") {
    return {
      ok: false,
      reason:
        `TypeScript provider process ${tree.typeProviderStatus} ` +
        `(${tree.typeProviderReason}) — retained-memory row is invalidated (JBT1H-AC2)`,
    };
  }
  if (tree.typeProviderStatus === "unknown") {
    return {
      ok: false,
      reason:
        `TypeScript provider process unknown (${tree.typeProviderReason}) — ` +
        `retained-memory cannot be claimed; metric stays unknown`,
    };
  }
  if (tree.typeProviderPids.length === 0) {
    return {
      ok: false,
      reason: "TypeScript provider status is observed but no provider pids were recorded",
    };
  }
  const known = new Set(tree.members.map((member) => member.pid));
  const outside = tree.typeProviderPids.filter((pid) => !known.has(pid));
  if (outside.length > 0) {
    return {
      ok: false,
      reason:
        `TypeScript provider pids ${outside.join(", ")} are not in the process tree — ` +
        `excluding the provider invalidates the memory row (JBT1H-AC2)`,
    };
  }
  return { ok: true, reason: tree.typeProviderReason };
}

export function evaluateMemoryRow(input: {
  readonly tree: ProcessTreeSnapshot;
  readonly retainedMemory: Metric;
  readonly providerCpu: Metric;
  readonly providerWall: Metric;
  readonly outboundBytes: Metric;
}): MemoryRow {
  const provider = providerRequiredForMemory(input.tree);
  if (!provider.ok) {
    const claimedNumber = input.retainedMemory.status === "measured";
    return {
      status: claimedNumber ? "invalidated" : "unknown",
      retainedMemory: claimedNumber
        ? unknownMetric(`invalidated: ${provider.reason}`)
        : input.retainedMemory.status === "unknown"
          ? input.retainedMemory
          : unknownMetric(provider.reason),
      providerCpu: keepUnknown(input.providerCpu, provider.reason),
      providerWall: keepUnknown(input.providerWall, provider.reason),
      outboundBytes: input.outboundBytes,
      reason: provider.reason,
      typeProviderStatus: input.tree.typeProviderStatus,
    };
  }
  return {
    status: input.retainedMemory.status === "measured" ? "valid" : "unknown",
    retainedMemory: input.retainedMemory,
    providerCpu: input.providerCpu,
    providerWall: input.providerWall,
    outboundBytes: input.outboundBytes,
    reason: provider.reason,
    typeProviderStatus: input.tree.typeProviderStatus,
  };
}

function keepUnknown(metric: Metric, reason: string): Metric {
  if (metric.status === "measured") return unknownMetric(`invalidated: ${reason}`);
  return metric;
}
