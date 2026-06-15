/**
 * Completion collector (raw-LSP).
 *
 * Samples completion AFTER EVERY CHARACTER of a burst edit so the No-Suggestions
 * collapse — verter returning an empty set mid-typing — is visible across the typing
 * trajectory. The collapse is a raw-LSP CANDIDATE: the raw-LSP layer cannot prove the
 * collapse reached the user, so it is recorded at the `candidate` severity with the
 * provenance that the extension-host driver confirms it user-visible (escalating it to
 * `userVisible`). Trigger characters come from the server's advertised
 * `completionProvider.triggerCharacters` — read from the initialize result, never
 * hardcoded.
 *
 * Baseline parity (when a provider's set is supplied) is delegated to the shared
 * `compareCompletion` comparator — the collector classifies the resulting findings'
 * severities and emits events; it does not re-implement the comparison.
 */

import { compareCompletion, type BaselineCompletion } from "../differential/index.js";
import { normalizeCompletion, type CanonicalCompletionList } from "../normalize/index.js";
import type { CompletionResponse } from "../normalize/lspTypes.js";
import type { EditStep } from "../scenario/index.js";
import {
  type CollectorLspClient,
  openDocument,
  sendTickChange,
  tickCursorPosition,
} from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

/** The collapse signal's provenance: a raw-LSP candidate the extension-host confirms. */
const COLLAPSE_PROVENANCE: SignalProvenance = {
  detectedBy: "rawLsp",
  confirmedBy: "extensionHost",
  escalatesTo: "userVisible",
  note: "an empty completion mid-typing is a candidate until the extension-host driver confirms it is user-visible",
};

/** A baseline provider's completion set for one sample (for parity). */
export interface CompletionBaseline {
  readonly provider: string;
  readonly completion: BaselineCompletion;
}

/** The pure inputs to one completion-sample classification. */
export interface CompletionSampleInput {
  readonly key: CollectorEventKey;
  /** Verter's normalized completion set at this sample. */
  readonly verter: CanonicalCompletionList;
  /** An optional baseline provider's set for verter-vs-baseline parity. */
  readonly baseline?: CompletionBaseline;
  /** Scenario-required labels that must appear regardless of the baseline. */
  readonly requiredLabels?: readonly string[];
  /**
   * The buffer mutation that produced this sample. The No-Suggestions collapse is a
   * mid-typing (`insertion`) phenomenon — an empty set after a `deletion` is expected,
   * not a collapse. Defaults to `insertion`.
   */
  readonly mutation?: "insertion" | "deletion";
}

/**
 * The advertised completion trigger characters, read from a server's
 * `completionProvider.triggerCharacters`. Returns `[]` for absent/malformed
 * capabilities — the harness reads the server's triggers, it never invents them.
 */
export function completionTriggerCharacters(serverCapabilities: unknown): string[] {
  const triggers = (
    serverCapabilities as
      | { completionProvider?: { triggerCharacters?: unknown } }
      | null
      | undefined
  )?.completionProvider?.triggerCharacters;
  if (!Array.isArray(triggers)) return [];
  return triggers.filter((trigger): trigger is string => typeof trigger === "string");
}

/** An LSP `CompletionContext`: a typed trigger character vs an invoked completion. */
export type CompletionTriggerContext =
  | { readonly triggerKind: 1 }
  | { readonly triggerKind: 2; readonly triggerCharacter: string };

/**
 * Build the completion context for a typed character: a server-advertised trigger
 * character yields `TriggerCharacter` (kind 2) carrying the character; anything else
 * yields `Invoked` (kind 1).
 */
export function triggerContextForChar(
  char: string,
  triggers: readonly string[],
): CompletionTriggerContext {
  return triggers.includes(char) ? { triggerKind: 2, triggerCharacter: char } : { triggerKind: 1 };
}

/**
 * Classify one completion sample into events. Always emits the per-sample
 * No-Suggestions collapse trajectory event (a `candidate`; `ok` is whether verter
 * returned anything). When a baseline is present, the shared `compareCompletion`
 * comparator drives the parity findings (the collapse class is folded into the
 * dedicated collapse event above, never double-counted). With no baseline, the
 * scenario-required labels are still enforced directly.
 */
export function classifyCompletionSample(input: CompletionSampleInput): CollectorEvent[] {
  const { key, verter, baseline, requiredLabels } = input;
  const events: CollectorEvent[] = [];
  const verterEmpty = verter.items.length === 0;
  const baselineLabelCount = baseline?.completion.items.length;
  const mutation = input.mutation ?? "insertion";

  // A No-Suggestions collapse is a mid-typing (insertion) phenomenon: an empty set after
  // a DELETION is expected (the user removed the trigger), so it is not flagged.
  const isCollapse = verterEmpty && mutation === "insertion";
  const collapseDetail = !verterEmpty
    ? "verter returned a non-empty completion set"
    : mutation === "deletion"
      ? "verter returned no completions after a deletion (expected, not a collapse)"
      : baselineLabelCount !== undefined && baselineLabelCount > 0
        ? `verter returned no completions where the baseline (${baseline?.provider}) returned ${baselineLabelCount}`
        : "verter returned no completions mid-typing";
  events.push(
    collectorEvent({
      collector: "completion",
      signal: "no_suggestions_collapse",
      ok: !isCollapse,
      severity: "candidate",
      provenance: COLLAPSE_PROVENANCE,
      key,
      detail: collapseDetail,
      data: baselineLabelCount !== undefined ? { mutation, baselineLabelCount } : { mutation },
    }),
  );

  // Baseline parity: the shared comparator is the comparison authority.
  if (baseline !== undefined) {
    const findings = compareCompletion(
      verter,
      baseline.completion,
      requiredLabels !== undefined ? { requiredLabels } : {},
    );
    const provenance: SignalProvenance = {
      detectedBy: "rawLsp",
      note: `baseline=${baseline.provider}`,
    };
    let divergent = false;
    for (const finding of findings) {
      if (finding.class === "noSuggestionsCollapse") continue; // covered by the collapse event above
      divergent = true;
      events.push(
        collectorEvent({
          collector: "completion",
          signal: "completion_parity",
          ok: false,
          severity: "userVisible",
          provenance,
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
    // Agreement: verter offered a non-empty set the baseline parity comparator did not
    // fault. Emit the explicit ok event (mirroring hover_parity / definition_parity /
    // diagnostics_parity), so a faithful baseline is an assertable positive, not merely
    // the absence of a divergence. A verter collapse is never a parity-ok — it is the
    // `no_suggestions_collapse` event above.
    if (!divergent && !verterEmpty) {
      events.push(
        collectorEvent({
          collector: "completion",
          signal: "completion_parity",
          ok: true,
          severity: "userVisible",
          provenance,
          key,
          detail: "verter completion agrees with the baseline set",
        }),
      );
    }
    return events;
  }

  // No baseline: still enforce scenario-required labels against verter's set.
  if (requiredLabels !== undefined) {
    const present = new Set(verter.items.map((item) => item.label));
    for (const label of requiredLabels) {
      if (present.has(label)) continue;
      events.push(
        collectorEvent({
          collector: "completion",
          signal: "completion_required_label",
          ok: false,
          severity: "userVisible",
          provenance: { detectedBy: "rawLsp" },
          key,
          detail: `required completion label "${label}" is absent from verter's set`,
          data: { label },
        }),
      );
    }
  }
  return events;
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectCompletion} run. */
export interface CollectCompletionOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  /** The open `.vue` document URI. */
  readonly uri: string;
  /** The document language id (`vue`). */
  readonly languageId?: string;
  /** A buffer initialized with the stripped `.vue` text and its anchor offsets. */
  readonly buffer: EditBuffer;
  /** The edit script to type (burst steps sample per character). */
  readonly script: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  /** The verter type-provider backend the server runs with (recorded as the provider). */
  readonly provider: string;
  readonly requiredLabels?: readonly string[];
  readonly requestTimeoutMs?: number;
  /** An optional per-sample baseline supplier (omitted for a verter-only raw-LSP run). */
  readonly baselineAt?: (
    cursorOffset: number,
    version: number,
  ) => Promise<CompletionBaseline | undefined>;
}

/**
 * Drive verter through the edit script, sampling completion after every tick and
 * emitting collapse / parity events. The completion context per sample is derived from
 * the just-typed character against the server's advertised triggers.
 */
export async function collectCompletion(options: CollectCompletionOptions): Promise<void> {
  const { client, sink, uri, buffer, script } = options;
  const triggers = completionTriggerCharacters(client.serverCapabilities);
  openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  await runEditScript(buffer, script, async (tick) => {
    sendTickChange(client, uri, tick);
    const context = triggerContextForChar(tick.change.text, triggers);
    const position = tickCursorPosition(client, tick);
    const raw = await client.sendRequest<CompletionResponse>(
      "textDocument/completion",
      { textDocument: { uri }, position, context },
      options.requestTimeoutMs,
    );
    const verter = normalizeCompletion(raw);
    const baseline = await options.baselineAt?.(tick.cursor, tick.version);
    const key: CollectorEventKey = {
      scenario: options.scenario,
      editStepIndex: tick.editStepIndex,
      driver: "rawLsp",
      provider: options.provider,
      probe: options.probe,
      version: tick.version,
      anchor: options.anchor,
    };
    for (const event of classifyCompletionSample({
      key,
      verter,
      // A tick whose change inserts no text is a deletion; an empty set there is expected.
      mutation: tick.change.text === "" ? "deletion" : "insertion",
      ...(baseline !== undefined ? { baseline } : {}),
      ...(options.requiredLabels !== undefined ? { requiredLabels: options.requiredLabels } : {}),
    })) {
      sink.emit(event);
    }
  });
}
