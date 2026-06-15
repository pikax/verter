/**
 * The shared collector event substrate: the severity ladder, the per-event probe
 * key, the JSONL event shape, an in-memory sink, and deterministic serialization.
 *
 * Every signal collector folds its observations into {@link CollectorEvent}s and
 * pushes them to an {@link EventSink}; the report layer (built separately) reads the
 * emitted JSONL. This module is pure data + pure serialization — importing it does
 * no I/O and mutates no globals — so the whole event model is unit-testable without
 * spawning a language server.
 */

import type { RequiredDriver } from "../scenario/index.js";

// ── severity ladder ──────────────────────────────────────────────────────────

/**
 * The three-level severity ladder a collector finding carries, in descending
 * order of severity:
 *  - `critical` — a crash / hang / data-loss class failure (a request timed out,
 *    the server exited, a document desynced);
 *  - `userVisible` — a user-visible wrong/broken result (a wrong hover, a wrong
 *    definition target, a default-collapsed diagnostic, a latency breach);
 *  - `candidate` — a latent finding that stays a candidate at this driver until a
 *    higher-fidelity driver confirms it is user-visible (the No-Suggestions collapse
 *    is a raw-LSP candidate; the extension-host driver confirms it).
 *
 * The names are descriptive on purpose — the ladder is an ordered model, not a set
 * of opaque codes.
 */
export const SEVERITIES = ["critical", "userVisible", "candidate"] as const;
export type Severity = (typeof SEVERITIES)[number];

/** Total order over {@link Severity}: a lower rank is MORE severe. */
export const SEVERITY_RANK: Record<Severity, number> = {
  critical: 0,
  userVisible: 1,
  candidate: 2,
};

/** Whether `a` is at least as severe as `b` (i.e. ranks at or above it). */
export function atLeastAsSevere(a: Severity, b: Severity): boolean {
  return SEVERITY_RANK[a] <= SEVERITY_RANK[b];
}

export function isSeverity(value: unknown): value is Severity {
  return typeof value === "string" && (SEVERITIES as readonly string[]).includes(value);
}

// ── provenance ───────────────────────────────────────────────────────────────

/**
 * Why a finding carries the severity it does, and what would escalate it. A
 * raw-LSP collector records `detectedBy: "rawLsp"`; a candidate finding additionally
 * records the higher-fidelity driver that would confirm it user-visible
 * ({@link confirmedBy}) and the severity it escalates to on that confirmation
 * ({@link escalatesTo}). The No-Suggestions collapse is the canonical case: detected
 * raw-LSP as a `candidate`, confirmed by the extension-host driver as `userVisible`.
 */
export interface SignalProvenance {
  /** The driver that observed the signal. */
  readonly detectedBy: RequiredDriver;
  /** The higher-fidelity driver that would confirm a candidate finding, if any. */
  readonly confirmedBy?: RequiredDriver;
  /** The severity a candidate finding escalates to once {@link confirmedBy} confirms it. */
  readonly escalatesTo?: Severity;
  /** A short human note on the provenance. */
  readonly note?: string;
}

// ── probe key ────────────────────────────────────────────────────────────────

/** The collectors that emit events — one per signal dimension. */
export const COLLECTOR_NAMES = [
  "completion",
  "hover",
  "definition",
  "autoImport",
  "diagnostics",
  "churn",
  "latency",
  "logs",
  "recovery",
] as const;
export type CollectorName = (typeof COLLECTOR_NAMES)[number];

/**
 * The closed taxonomy of every signal a collector emits — the SINGLE source of
 * truth, grouped by owning collector in {@link COLLECTOR_NAMES} order. A
 * {@link CollectorEvent} carries one of these as its {@link CollectorEvent.signal},
 * so the compiler rejects a misspelled or unregistered signal before it can
 * serialize into the JSONL stream. Adding a new signal to a collector means adding
 * its literal HERE; the type error at the emit site is the reminder to register it.
 */
export const COLLECTOR_SIGNALS = [
  // completion
  "no_suggestions_collapse",
  "completion_parity",
  "completion_required_label",
  // hover
  "hover_vue_semantic_validity",
  "hover_synthetic_region_tolerated",
  "hover_parity",
  "hover_required_snippet",
  "hover_invariant",
  "hover_contentless_observed",
  "hover_observed",
  // definition
  "definition_parity",
  // autoImport
  "auto_import_empty_edit",
  "auto_import_wrong_text",
  "auto_import_not_introduced",
  "auto_import_applied",
  "auto_import_no_candidate",
  // diagnostics
  "diagnostics_vue_semantic_validity",
  "diagnostics_parity",
  "diagnostics_default_range",
  // churn
  "churn_attribution_uncertain",
  "churn_burst_aggregate",
  "churn_steady_state_delta",
  // latency
  "latency_breach",
  "latency_summary",
  // logs
  "mapping_root_cause_hint",
  "mapping_failure_benign",
  "server_error",
  "server_warn",
  // recovery
  "recovery_baseline_restored",
  "recovery_not_restored",
] as const;

/** One signal value from the closed {@link COLLECTOR_SIGNALS} taxonomy. */
export type CollectorSignal = (typeof COLLECTOR_SIGNALS)[number];

/**
 * The identity every event is keyed by: the scenario, the index of the edit script
 * operation that produced this sample ({@link editStepIndex}; negative means a
 * pre-edit baseline sample), the driver, the provider (the verter type-provider
 * backend the raw-LSP server runs with, or the baseline provider for a parity event),
 * the probe id, the live document version, and the named source anchor.
 */
export interface CollectorEventKey {
  readonly scenario: string;
  readonly editStepIndex: number;
  readonly driver: RequiredDriver;
  readonly provider: string;
  readonly probe: string;
  readonly version: number;
  readonly anchor: string;
}

// ── event ────────────────────────────────────────────────────────────────────

/**
 * One JSONL collector event. `ok` is the health flag: `true` is a healthy
 * observation (the signal did not trip), `false` is a flagged finding. `severity`
 * is the severity class of the SIGNAL — the level a tripped finding carries — so a
 * healthy sample and a tripped one for the same signal share a severity and differ
 * only in `ok`. `data` is the signal-specific payload (divergence findings, the
 * latency summary, the churn delta, …).
 */
export interface CollectorEvent {
  readonly collector: CollectorName;
  readonly signal: CollectorSignal;
  readonly ok: boolean;
  readonly severity: Severity;
  readonly provenance: SignalProvenance;
  readonly key: CollectorEventKey;
  readonly detail: string;
  readonly data?: unknown;
}

/** The fields of a {@link CollectorEvent} (the optional `data` may be omitted). */
export interface CollectorEventInput {
  readonly collector: CollectorName;
  readonly signal: CollectorSignal;
  readonly ok: boolean;
  readonly severity: Severity;
  readonly provenance: SignalProvenance;
  readonly key: CollectorEventKey;
  readonly detail: string;
  readonly data?: unknown;
}

/** Build a {@link CollectorEvent}, omitting `data` entirely when it is undefined. */
export function collectorEvent(input: CollectorEventInput): CollectorEvent {
  const base: CollectorEvent = {
    collector: input.collector,
    signal: input.signal,
    ok: input.ok,
    severity: input.severity,
    provenance: input.provenance,
    key: input.key,
    detail: input.detail,
  };
  return input.data !== undefined ? { ...base, data: input.data } : base;
}

// ── sink ─────────────────────────────────────────────────────────────────────

/** The destination a collector pushes events to. */
export interface EventSink {
  emit(event: CollectorEvent): void;
}

/**
 * An in-memory {@link EventSink} that retains every event in emission order. The
 * default sink for tests and for an in-process collection run; a file-backed JSONL
 * sink (the report layer's concern) implements the same interface.
 */
export class CollectingSink implements EventSink {
  readonly events: CollectorEvent[] = [];

  emit(event: CollectorEvent): void {
    this.events.push(event);
  }

  /** The flagged findings only (the events whose signal tripped). */
  get failures(): CollectorEvent[] {
    return this.events.filter((event) => !event.ok);
  }
}

// ── serialization ──────────────────────────────────────────────────────────────

/**
 * Serialize one event to a single JSONL line. Top-level fields are emitted in a
 * fixed order for readable, diff-stable output; the line never contains a newline
 * (the EOL-normalized `detail`/`data` are JSON-escaped), so {@link toJsonl} can
 * join records with `\n` unambiguously.
 */
export function serializeCollectorEvent(event: CollectorEvent): string {
  // A fixed key order — JSON.stringify preserves string-key insertion order — so the
  // line is stable across runs without sorting away the intended field order.
  const ordered: Record<string, unknown> = {
    collector: event.collector,
    signal: event.signal,
    ok: event.ok,
    severity: event.severity,
    provenance: event.provenance,
    key: event.key,
    detail: event.detail,
  };
  if (event.data !== undefined) ordered.data = event.data;
  return JSON.stringify(ordered);
}

/** Serialize a list of events to newline-delimited JSON (one record per line, no trailing newline). */
export function toJsonl(events: readonly CollectorEvent[]): string {
  return events.map(serializeCollectorEvent).join("\n");
}
