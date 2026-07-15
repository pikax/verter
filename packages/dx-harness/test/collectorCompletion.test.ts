import { describe, expect, it } from "vitest";

import {
  classifyCompletionSample,
  completionTriggerCharacters,
  triggerContextForChar,
  type CompletionSampleInput,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import { normalizeCompletion } from "../src/index.js";
import type { NormalizedCompletionItem } from "../src/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 1,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "completion-after-dot",
  version: 3,
  anchor: "cursor",
};

const base = (over: Partial<CompletionSampleInput> = {}): CompletionSampleInput => ({
  key,
  verter: normalizeCompletion([{ label: "name", kind: 10 }]),
  ...over,
});

function baselineItems(...labels: string[]): NormalizedCompletionItem[] {
  return labels.map((label) => ({ label, kind: "Property" }) as NormalizedCompletionItem);
}

describe("completionTriggerCharacters — read from server capabilities, never hardcoded", () => {
  it("returns the advertised completionProvider.triggerCharacters", () => {
    const capabilities = {
      completionProvider: { resolveProvider: true, triggerCharacters: [".", "@", "<"] },
    };
    expect(completionTriggerCharacters(capabilities)).toEqual([".", "@", "<"]);
  });

  it("returns [] for absent/malformed capabilities rather than inventing triggers", () => {
    expect(completionTriggerCharacters(undefined)).toEqual([]);
    expect(completionTriggerCharacters({})).toEqual([]);
    expect(completionTriggerCharacters({ completionProvider: {} })).toEqual([]);
  });
});

describe("triggerContextForChar — trigger kind from the typed character + advertised triggers", () => {
  it("a typed trigger character yields TriggerCharacter (kind 2) with the character", () => {
    expect(triggerContextForChar(".", [".", "@"])).toEqual({
      triggerKind: 2,
      triggerCharacter: ".",
    });
  });

  it("a non-trigger character yields Invoked (kind 1)", () => {
    expect(triggerContextForChar("x", [".", "@"])).toEqual({ triggerKind: 1 });
  });
});

describe("classifyCompletionSample — no_suggestions_collapse is a raw-LSP candidate (escalates on ext-host)", () => {
  it("does NOT flag a POPULATED sample (the good case)", () => {
    const events = classifyCompletionSample(base());
    const collapse = events.find((e) => e.signal === "no_suggestions_collapse");
    expect(collapse?.ok).toBe(true);
    expect(events.every((e) => e.ok)).toBe(true);
  });

  it("flags an EMPTY mid-typing sample as a candidate that escalates to user-visible on ext-host confirm", () => {
    const events = classifyCompletionSample(base({ verter: normalizeCompletion([]) }));
    const collapse = events.find((e) => e.signal === "no_suggestions_collapse");
    expect(collapse).toBeDefined();
    expect(collapse?.ok).toBe(false);
    expect(collapse?.severity).toBe("candidate");
    expect(collapse?.provenance.detectedBy).toBe("rawLsp");
    expect(collapse?.provenance.confirmedBy).toBe("extensionHost");
    expect(collapse?.provenance.escalatesTo).toBe("userVisible");
  });

  it("records the baseline non-emptiness when verter collapses but the baseline has suggestions", () => {
    const events = classifyCompletionSample(
      base({
        verter: normalizeCompletion([]),
        baseline: {
          provider: "tsgo",
          completion: { items: baselineItems("a", "b"), isIncomplete: false },
        },
      }),
    );
    const collapse = events.find((e) => e.signal === "no_suggestions_collapse");
    expect(collapse?.ok).toBe(false);
    expect((collapse?.data as { baselineLabelCount?: number }).baselineLabelCount).toBe(2);
  });

  it("drives the shared completion comparator for parity: a baseline label missing from verter is a user-visible finding", () => {
    const events = classifyCompletionSample(
      base({
        verter: normalizeCompletion([{ label: "name", kind: 10 }]),
        baseline: {
          provider: "tsgo",
          completion: { items: baselineItems("name", "value"), isIncomplete: false },
        },
      }),
    );
    const parity = events.filter((e) => e.signal === "completion_parity" && !e.ok);
    expect(parity.length).toBeGreaterThan(0);
    expect(parity.some((e) => e.severity === "userVisible")).toBe(true);
    expect(parity.some((e) => (e.data as { class?: string }).class === "missingLabel")).toBe(true);
    // The populated verter set is not a collapse.
    expect(events.find((e) => e.signal === "no_suggestions_collapse")?.ok).toBe(true);
  });

  it("emits an OK completion_parity event when verter agrees with the baseline set", () => {
    // Mirrors hover_parity / definition_parity / diagnostics_parity: a faithful baseline
    // is an assertable positive, not merely the absence of a divergence.
    const events = classifyCompletionSample(
      base({
        verter: normalizeCompletion([{ label: "name", kind: 10 }]),
        baseline: {
          provider: "tsgo",
          completion: { items: baselineItems("name"), isIncomplete: false },
        },
      }),
    );
    const parity = events.filter((e) => e.signal === "completion_parity");
    expect(parity).toHaveLength(1);
    expect(parity[0].ok).toBe(true);
    expect(events.find((e) => e.signal === "no_suggestions_collapse")?.ok).toBe(true);
  });

  it("does NOT emit an OK completion_parity when verter collapses against a populated baseline", () => {
    // A verter collapse is the `no_suggestions_collapse` signal, never a parity-ok.
    const events = classifyCompletionSample(
      base({
        verter: normalizeCompletion([]),
        baseline: {
          provider: "tsgo",
          completion: { items: baselineItems("a", "b"), isIncomplete: false },
        },
      }),
    );
    expect(events.some((e) => e.signal === "completion_parity" && e.ok)).toBe(false);
    expect(events.find((e) => e.signal === "no_suggestions_collapse")?.ok).toBe(false);
  });

  it("does NOT flag an empty completion after a DELETION as a collapse (expected; ok:true)", () => {
    const events = classifyCompletionSample(
      base({ verter: normalizeCompletion([]), mutation: "deletion" }),
    );
    const collapse = events.find((e) => e.signal === "no_suggestions_collapse");
    expect(collapse?.ok).toBe(true);
    expect(collapse?.detail.toLowerCase()).toContain("deletion");
    expect((collapse?.data as { mutation?: string }).mutation).toBe("deletion");
  });

  it("flags an empty completion mid-typing (insertion) as a candidate collapse (ok:false, mid-typing detail)", () => {
    const events = classifyCompletionSample(
      base({ verter: normalizeCompletion([]), mutation: "insertion" }),
    );
    const collapse = events.find((e) => e.signal === "no_suggestions_collapse");
    expect(collapse?.ok).toBe(false);
    expect(collapse?.detail).toContain("mid-typing");
  });

  it("flags a required label that verter omits (no baseline)", () => {
    const events = classifyCompletionSample(base({ requiredLabels: ["name", "missingOne"] }));
    const missing = events.filter(
      (e) => !e.ok && (e.data as { label?: string }).label === "missingOne",
    );
    expect(missing).toHaveLength(1);
    expect(missing[0].severity).toBe("userVisible");
    // The present required label does NOT flag.
    expect(events.some((e) => !e.ok && (e.data as { label?: string }).label === "name")).toBe(
      false,
    );
  });
});
