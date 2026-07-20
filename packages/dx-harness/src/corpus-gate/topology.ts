/**
 * Execution topology and measurement isolation for the corpus gate.
 *
 * Latency percentiles measured while three ~1 GB language servers and their
 * providers fight over one box are not gating numbers — they measure the box.
 * So the gate separates two axes:
 *
 *  - TOPOLOGY: how route sessions are scheduled on THIS executor (`serial` —
 *    one at a time, the default — or `parallel` — concurrently).
 *  - ISOLATION: whether a route's measurements were free of gate-induced
 *    contention. Isolation is DERIVED from what the orchestrator observed
 *    (peak concurrent route sessions during the route's own window) combined
 *    with what the executor attests about the machine. It is never a bare
 *    claim, and a route runner cannot declare its own isolation.
 *
 * Only isolated routes gate on latency. Contended routes still publish their
 * percentiles, labelled ADVISORY, and every other assertion — stability,
 * wedge, liveness, unexpected-empty, accounting, memory — gates in BOTH modes,
 * because contention does not make a wedge or an empty result valid.
 *
 * CI fan-out (one route per machine) is deliberately NOT `parallel`: each
 * machine runs a single-route `serial` gate with a `dedicated` attestation, so
 * the wall clock drops to the slowest route while latency stays gating.
 */
import os from "node:os";

import type {
  CorpusExecutionTopology,
  CorpusExecutorAttestation,
  CorpusMachineCapability,
  CorpusRouteIsolation,
} from "./types.js";

/** Cores a route session needs to itself before local parallelism is allowed. */
export const MIN_CORES_PER_PARALLEL_ROUTE = 4;
/** Memory a route session needs to itself before local parallelism is allowed. */
export const MIN_MEMORY_BYTES_PER_PARALLEL_ROUTE = 4 * 1024 * 1024 * 1024;

/** Observe this machine's resources (the capability gate's only input). */
export function probeMachineCapability(): CorpusMachineCapability {
  return { cpuCount: os.cpus().length, totalMemBytes: os.totalmem() };
}

export interface TopologyResolution {
  readonly requested: CorpusExecutionTopology;
  readonly effective: CorpusExecutionTopology;
  /** Populated iff the requested topology was not honoured. */
  readonly downgradeReason: string | null;
}

/**
 * Resolve the effective topology. `parallel` is honoured only when the machine
 * can give every concurrent route session its own cores and memory; otherwise
 * the run DOWNGRADES to serial and records why. Downgrading never loses
 * verification — it costs wall clock and buys valid measurements.
 */
export function resolveExecutionTopology(
  requested: CorpusExecutionTopology,
  routeCount: number,
  capability: CorpusMachineCapability,
): TopologyResolution {
  if (requested === "serial") {
    return { requested, effective: "serial", downgradeReason: null };
  }
  if (routeCount <= 1) {
    return {
      requested,
      effective: "serial",
      downgradeReason: `parallel topology is meaningless for ${routeCount} route(s)`,
    };
  }
  const neededCores = routeCount * MIN_CORES_PER_PARALLEL_ROUTE;
  const neededMemory = routeCount * MIN_MEMORY_BYTES_PER_PARALLEL_ROUTE;
  if (capability.cpuCount < neededCores || capability.totalMemBytes < neededMemory) {
    return {
      requested,
      effective: "serial",
      downgradeReason:
        `machine cannot isolate ${routeCount} concurrent route sessions ` +
        `(has ${capability.cpuCount} cores / ${Math.round(capability.totalMemBytes / 1024 ** 3)} GiB, ` +
        `needs ${neededCores} cores / ${Math.round(neededMemory / 1024 ** 3)} GiB)`,
    };
  }
  return { requested, effective: "parallel", downgradeReason: null };
}

export interface IsolationObservation {
  readonly topology: CorpusExecutionTopology;
  readonly executor: CorpusExecutorAttestation;
  /** Peak concurrent route sessions observed during this route's own window. */
  readonly observedConcurrentRoutes: number;
}

/**
 * Classify one route's measurement isolation (pure).
 *
 * Fail-closed: latency gates only when the orchestrator OBSERVED this route as
 * the sole in-flight session AND nothing refutes executor exclusivity. A
 * `dedicated` attestation refuted by observed concurrency is recorded as a
 * contradiction — a hard defect, not a downgrade.
 */
export function classifyIsolation(observation: IsolationObservation): CorpusRouteIsolation {
  const { topology, executor, observedConcurrentRoutes } = observation;
  const base = { topology, executor, observedConcurrentRoutes };

  if (observedConcurrentRoutes > 1) {
    return {
      ...base,
      mode: "contended",
      latencyGating: false,
      attestationContradicted: executor === "dedicated",
      evidence:
        `${observedConcurrentRoutes} route sessions were in flight on this executor during ` +
        `this route's window — its latency measures contention, not the server` +
        (executor === "dedicated" ? ` (and CONTRADICTS the declared dedicated executor)` : ""),
    };
  }
  if (executor === "shared") {
    return {
      ...base,
      mode: "contended",
      latencyGating: false,
      attestationContradicted: false,
      evidence:
        "executor declared shared — other work may run on this machine, so latency is advisory",
    };
  }
  if (topology === "parallel" && executor !== "dedicated") {
    return {
      ...base,
      mode: "contended",
      latencyGating: false,
      attestationContradicted: false,
      evidence:
        "parallel topology without a dedicated-executor attestation — isolation is unproven, " +
        "so latency cannot gate",
    };
  }
  return {
    ...base,
    mode: "isolated",
    latencyGating: true,
    attestationContradicted: false,
    evidence:
      `sole route session in flight on this executor (observed concurrency 1, topology ${topology}, ` +
      `executor ${executor})`,
  };
}

/**
 * The fail-closed isolation value a route report carries until the
 * orchestrator stamps what it observed, and the value the acceptance bar
 * substitutes for a receipt that recorded none. Never gating.
 */
export const UNPROVEN_ISOLATION: CorpusRouteIsolation = {
  topology: "serial",
  executor: "unattested",
  mode: "contended",
  observedConcurrentRoutes: 0,
  latencyGating: false,
  attestationContradicted: false,
  evidence: "isolation was never recorded for this route — latency cannot gate (fail-closed)",
};

/**
 * Peak-concurrency tracker: what the orchestrator OBSERVES, per route.
 *
 * Every time a session starts, every session already in flight has its peak
 * raised — so a route that was alone for its first second and shared the box
 * for the rest is recorded as contended, not isolated.
 */
export class RouteConcurrencyTracker {
  private readonly running = new Set<string>();
  private readonly peaks = new Map<string, number>();

  /** Mark a route session started; returns its release callback. */
  start(route: string): () => void {
    this.running.add(route);
    this.refresh();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.running.delete(route);
    };
  }

  private refresh(): void {
    const inFlight = this.running.size;
    for (const route of this.running) {
      this.peaks.set(route, Math.max(this.peaks.get(route) ?? 0, inFlight));
    }
  }

  /** Peak concurrent sessions observed while `route` was running. */
  peakFor(route: string): number {
    return this.peaks.get(route) ?? 0;
  }
}
