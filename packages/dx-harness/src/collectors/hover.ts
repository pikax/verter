/**
 * Hover collector (raw-LSP).
 *
 * Two dimensions: artifact parity (verter hover vs a baseline provider's hover on
 * the same emitted TSX, delegated to the shared `compareHover` comparator) and direct
 * Vue-surface invariants (the hover type label `contains`/`excludes`/`equals` a value
 * — e.g. a `@click` hover must NOT leak `onClick`).
 *
 * Synthetic regions are tolerated: verter deliberately avoids hovering synthetic TSX
 * positions to dodge provider crashes, so a CONTENTLESS verter hover in a synthetic
 * region is a healthy observation, never a presence-mismatch failure — even when a
 * baseline produced content there.
 */

import {
  classifyOracleHover,
  compareHover,
  stripUnstableDocs,
  type DifferentialOutcome,
  type GeneratedDocument,
  type ProviderInputs,
} from "../differential/index.js";
import type { NormalizedHover, ProviderName } from "../baseline/bridgeClient.js";
import { normalizeHover, type CanonicalHover } from "../normalize/index.js";
import type { HoverResponse } from "../normalize/lspTypes.js";
import type { EditStep, Probe } from "../scenario/index.js";
import {
  offsetToPosition,
  openDocument,
  sendTickChange,
  type CollectorLspClient,
} from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A baseline provider's hover for one sample (for parity). */
export interface HoverBaseline {
  readonly provider: string;
  readonly hover: NormalizedHover | null;
}

/** A direct Vue-surface assertion on the hover type label. */
export interface HoverInvariant {
  readonly id?: string;
  readonly assertion: "contains" | "excludes" | "equals";
  readonly value: string;
}

/**
 * A curated-oracle (vue semantic validity) comparison: verter's hover at the `.vue`
 * anchor classified against a hand-authored `.ts` gold-standard's hover via the SHARED
 * {@link classifyOracleHover}. Distinct from artifact parity ({@link HoverBaseline}),
 * which compares against a baseline on verter's OWN emitted TSX.
 */
export interface HoverOracle {
  /** The oracle probe identity (mapping policy `none`, `vueSemanticValidity` dimension). */
  readonly probe: Probe;
  /** The oracle providers' hover for the mirrored `.ts` anchor. */
  readonly providers: ProviderInputs<NormalizedHover | null>;
  /** Type tokens the intended Vue semantics require in verter's stripped label. */
  readonly requiredSnippets?: readonly string[];
  /** When set, verter is compared only against this oracle provider. */
  readonly authoritativeProvider?: ProviderName;
}

/** The pure inputs to one hover-sample classification. */
export interface HoverSampleInput {
  readonly key: CollectorEventKey;
  /** Verter's normalized hover (`null` = no hover). */
  readonly verter: CanonicalHover | null;
  /** Whether the probe position lands in a synthetic TSX region (a contentless hover is tolerated). */
  readonly syntheticRegion?: boolean;
  /** An optional baseline provider's hover for verter-vs-baseline parity. */
  readonly baseline?: HoverBaseline;
  /** The emitted-TSX converter, enabling generated-space range parity when both ranges exist. */
  readonly document?: GeneratedDocument;
  /** Substrings that must appear in verter's type label (enforced directly when no baseline). */
  readonly requiredSnippets?: readonly string[];
  /** Direct Vue-surface invariants on the type label. */
  readonly invariants?: readonly HoverInvariant[];
  /** An optional curated-oracle (vue semantic validity) comparison against a `.ts` gold standard. */
  readonly oracle?: HoverOracle;
}

/** Provenance for the curated-oracle (vue semantic validity) hover findings. */
const ORACLE_PROVENANCE: SignalProvenance = {
  detectedBy: "rawLsp",
  note: "curated .ts oracle — vue semantic validity",
};

/**
 * Map one shared-oracle outcome into a `hover_vue_semantic_validity` event. A
 * `divergence` is the ONLY verter-fault outcome (the curated `.ts` gold standard
 * disagreed with verter's `.vue` hover); agreement and every refusal/baseline-
 * disagreement outcome are recorded as `ok` observations, never failures.
 */
function oracleHoverEvent(outcome: DifferentialOutcome, key: CollectorEventKey): CollectorEvent {
  if (outcome.kind === "divergence") {
    return collectorEvent({
      collector: "hover",
      signal: "hover_vue_semantic_validity",
      ok: false,
      severity: "userVisible",
      provenance: ORACLE_PROVENANCE,
      key,
      detail: `verter hover diverged from the curated oracle: ${outcome.detail}`,
      data: {
        kind: outcome.kind,
        class: outcome.class,
        provider: outcome.provider,
        verterValue: outcome.verterValue,
        baselineValue: outcome.baselineValue,
      },
    });
  }
  const detail =
    outcome.kind === "agreement"
      ? "verter hover agrees with the curated oracle"
      : `curated oracle not compared (${outcome.kind})`;
  return collectorEvent({
    collector: "hover",
    signal: "hover_vue_semantic_validity",
    ok: true,
    severity: "userVisible",
    provenance: ORACLE_PROVENANCE,
    key,
    detail,
    data: { kind: outcome.kind },
  });
}

/** The stripped, stable type label of a hover (`""` for a contentless / absent hover). */
function hoverLabel(hover: CanonicalHover | null): string {
  return hover === null ? "" : stripUnstableDocs(hover.contents);
}

/**
 * The FULL normalized hover surface (`""` for a contentless / absent hover). Direct
 * Vue-surface invariants assert against this, NOT the doc-stripped label: the label
 * stripper drops leading `@`-tag lines (the JSDoc convention), which would erase a
 * legitimate `@click` / `@touchmove.stop` the invariant is checking for.
 */
function hoverSurface(hover: CanonicalHover | null): string {
  return hover === null ? "" : hover.contents;
}

/** Whether an invariant holds against a type label. */
function invariantHolds(invariant: HoverInvariant, label: string): boolean {
  switch (invariant.assertion) {
    case "contains":
      return label.includes(invariant.value);
    case "excludes":
      return !label.includes(invariant.value);
    case "equals":
      return label === invariant.value;
  }
}

/**
 * Classify one hover sample into events. A contentless verter hover in a synthetic
 * region is tolerated; otherwise baseline parity (via `compareHover`), required
 * snippets, and direct Vue-surface invariants each contribute findings.
 */
export function classifyHoverSample(input: HoverSampleInput): CollectorEvent[] {
  const { key, verter, baseline, invariants, requiredSnippets } = input;
  const events: CollectorEvent[] = [];
  const label = hoverLabel(verter);
  const contentless = label === "";

  // Synthetic-region tolerance: verter avoids synthetic TSX positions on purpose, so a
  // contentless hover there is healthy — never a miss, even if the baseline has content.
  if (input.syntheticRegion === true && contentless) {
    events.push(
      collectorEvent({
        collector: "hover",
        signal: "hover_synthetic_region_tolerated",
        ok: true,
        severity: "userVisible",
        provenance: {
          detectedBy: "rawLsp",
          note: "synthetic TSX region — verter avoids it deliberately",
        },
        key,
        detail: "contentless hover tolerated in a synthetic TSX region",
      }),
    );
    return events;
  }

  // Baseline parity — delegated to the shared comparator.
  if (baseline !== undefined) {
    const findings = compareHover(verter, baseline.hover, {
      ...(requiredSnippets !== undefined ? { requiredSnippets } : {}),
      ...(input.document !== undefined ? { document: input.document } : {}),
    });
    if (findings.length === 0) {
      events.push(
        collectorEvent({
          collector: "hover",
          signal: "hover_parity",
          ok: true,
          severity: "userVisible",
          provenance: { detectedBy: "rawLsp", note: `baseline=${baseline.provider}` },
          key,
          detail: "verter hover agrees with the baseline type label",
        }),
      );
    }
    for (const finding of findings) {
      events.push(
        collectorEvent({
          collector: "hover",
          signal: "hover_parity",
          ok: false,
          severity: "userVisible",
          provenance: { detectedBy: "rawLsp", note: `baseline=${baseline.provider}` },
          key,
          detail: finding.detail,
          data: {
            class: finding.class,
            verterValue: finding.verterValue,
            baselineValue: finding.baselineValue,
          },
        }),
      );
    }
  } else if (requiredSnippets !== undefined) {
    // No baseline: enforce required snippets directly against the type label.
    for (const snippet of requiredSnippets) {
      if (label.includes(snippet)) continue;
      events.push(
        collectorEvent({
          collector: "hover",
          signal: "hover_required_snippet",
          ok: false,
          severity: "userVisible",
          provenance: RAW_LSP,
          key,
          detail: `required hover snippet "${snippet}" is absent from verter's type label`,
          data: { snippet },
        }),
      );
    }
  }

  // Direct Vue-surface invariants on the FULL hover surface (not the doc-stripped label).
  const surface = hoverSurface(verter);
  for (const invariant of invariants ?? []) {
    if (invariantHolds(invariant, surface)) continue;
    events.push(
      collectorEvent({
        collector: "hover",
        signal: "hover_invariant",
        ok: false,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: `hover invariant (${invariant.assertion} "${invariant.value}") violated`,
        data: {
          invariant: invariant.id ?? invariant.assertion,
          assertion: invariant.assertion,
          value: invariant.value,
        },
      }),
    );
  }

  // Curated oracle (vue semantic validity): verter's hover vs a `.ts` gold standard,
  // classified through the SHARED `classifyOracleHover` (the artifact-parity comparator
  // reused, not a second comparison) — agreement is `ok`, a divergence is user-visible.
  if (input.oracle !== undefined) {
    const outcomes = classifyOracleHover({
      probe: input.oracle.probe,
      verter,
      providers: input.oracle.providers,
      ...(input.oracle.requiredSnippets !== undefined
        ? { requiredSnippets: input.oracle.requiredSnippets }
        : {}),
      ...(input.oracle.authoritativeProvider !== undefined
        ? { authoritativeProvider: input.oracle.authoritativeProvider }
        : {}),
    });
    for (const outcome of outcomes) events.push(oracleHoverEvent(outcome, key));
  }

  // Every probe emits a keyed sample: when no judging input (baseline / required snippets
  // / invariants / curated oracle) produced an event, record the bare observation
  // honestly — a contentful hover is an `ok` `hover_observed`; a contentless (non-
  // synthetic) hover is its own `hover_contentless_observed`, recorded `ok` because with
  // no oracle to judge it against, the raw-LSP layer cannot prove it wrong.
  if (events.length === 0) {
    events.push(
      collectorEvent({
        collector: "hover",
        signal: contentless ? "hover_contentless_observed" : "hover_observed",
        ok: true,
        severity: "userVisible",
        provenance: RAW_LSP,
        key,
        detail: contentless
          ? "verter produced no hover content (no baseline / oracle to judge it against)"
          : "verter produced a hover type label",
      }),
    );
  }

  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectHover} run. */
export interface CollectHoverOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  /** A buffer initialized with the stripped `.vue` text and its anchor offsets. */
  readonly buffer: EditBuffer;
  /** Edits applied before the hover is sampled at the anchor. */
  readonly script?: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  readonly syntheticRegion?: boolean;
  readonly requiredSnippets?: readonly string[];
  readonly invariants?: readonly HoverInvariant[];
  readonly document?: GeneratedDocument;
  /**
   * An optional curated-oracle (vue semantic validity) comparison: verter's live
   * `.vue` hover is classified against a hand-authored `.ts` gold standard through the
   * SHARED {@link classifyOracleHover}, emitting a `hover_vue_semantic_validity`
   * outcome. Distinct from {@link baselineAt} (artifact parity on verter's own TSX).
   */
  readonly oracle?: HoverOracle;
  readonly requestTimeoutMs?: number;
  /** An optional baseline supplier evaluated at the settled anchor position. */
  readonly baselineAt?: (
    anchorOffset: number,
    version: number,
  ) => Promise<HoverBaseline | undefined>;
}

/**
 * Drive verter through the (optional) edit script, then sample hover at the settled
 * anchor and classify it. The anchor offset is read from the buffer AFTER the script
 * applies, so a hover lands on the post-edit position.
 */
export async function collectHover(options: CollectHoverOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const script = options.script ?? [];
  openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  await runEditScript(buffer, script, (tick) => {
    sendTickChange(client, uri, tick);
  });

  const anchorOffset = buffer.anchorOffset(options.anchor);
  const position = offsetToPosition(buffer.text, anchorOffset, client.positionEncoding);
  const raw = await client.sendRequest<HoverResponse>(
    "textDocument/hover",
    { textDocument: { uri }, position },
    options.requestTimeoutMs,
  );
  const verter = normalizeHover(raw);
  const baseline = await options.baselineAt?.(anchorOffset, buffer.version);
  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: script.length - 1,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: buffer.version,
    anchor: options.anchor,
  };
  for (const event of classifyHoverSample({
    key,
    verter,
    ...(options.syntheticRegion !== undefined ? { syntheticRegion: options.syntheticRegion } : {}),
    ...(baseline !== undefined ? { baseline } : {}),
    ...(options.document !== undefined ? { document: options.document } : {}),
    ...(options.requiredSnippets !== undefined
      ? { requiredSnippets: options.requiredSnippets }
      : {}),
    ...(options.invariants !== undefined ? { invariants: options.invariants } : {}),
    ...(options.oracle !== undefined ? { oracle: options.oracle } : {}),
  })) {
    sink.emit(event);
  }
}
