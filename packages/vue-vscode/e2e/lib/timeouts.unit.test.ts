import { describe, expect, it } from "vitest";
import {
  DEFAULT_POLL_BUDGET_MS,
  POLL_BUDGET_MARGIN_MS,
  POLL_BUDGETS,
  POLL_SEQUENCES,
  SUITE_TIMEOUT_MS,
  pollDeadlineFits,
  pollSequenceFits,
  pollSequenceTotalMs,
} from "./timeouts";

describe("E2E deadline hierarchy", () => {
  it("keeps the default poll budget strictly below the suite timeout", () => {
    expect(DEFAULT_POLL_BUDGET_MS).toBeLessThan(SUITE_TIMEOUT_MS);
    expect(POLL_BUDGET_MARGIN_MS).toBeGreaterThan(0);
    // TWO default waits plus the margin must fit. Composition — open a document,
    // then wait for a feature — is the normal shape, and a budget that consumes
    // the whole deadline leaves the second wait unable to reach its own.
    expect(pollDeadlineFits(2 * DEFAULT_POLL_BUDGET_MS, SUITE_TIMEOUT_MS)).toBe(true);
  });

  it("rejects a child budget that the suite timeout would kill first", () => {
    // The exact shape of the defect this encodes: `measureHover` polled for
    // 20_000ms under a 15_000ms mocha timeout, so every zero-result hover
    // reported `Timeout of 15000ms exceeded` instead of the test's own
    // assertion — and a test body branching on `hovers.length === 0` could
    // never reach that branch.
    expect(pollDeadlineFits(SUITE_TIMEOUT_MS + 5_000, SUITE_TIMEOUT_MS)).toBe(false);
    expect(pollDeadlineFits(SUITE_TIMEOUT_MS, SUITE_TIMEOUT_MS)).toBe(false);
    // Equal-to-parent is the shape that first produced this: 15_000 under 15_000
    // reported a bare Mocha timeout, never the assertion the test wrote.
    expect(pollDeadlineFits(15_000, 15_000)).toBe(false);
    expect(pollDeadlineFits(20_000, 15_000)).toBe(false);
    expect(pollDeadlineFits(DEFAULT_POLL_BUDGET_MS, SUITE_TIMEOUT_MS)).toBe(true);
  });

  it("accepts a raised parent deadline for helpers whose callers opt in", () => {
    // `waitForFileReady({ timeoutMs: 30_000 })` is legitimate inside a test that
    // called `this.timeout(60_000)`. The invariant is relative, not absolute.
    expect(pollDeadlineFits(30_000, 60_000)).toBe(true);
    expect(pollDeadlineFits(30_000, 15_000)).toBe(false);
    expect(pollDeadlineFits(45_000, 90_000)).toBe(true);
  });

  // ── The registry ────────────────────────────────────────────
  //
  // The pure relation above is necessary and was not sufficient: it held while
  // `waitForCompletionsMatching` sat at 20_000ms under a 15_000ms mocha timeout,
  // because nothing connected the relation to the numbers the helpers actually
  // used. Every helper now takes its default FROM `POLL_BUDGETS`, so these cases
  // read the real budgets rather than a restatement of them.

  it("keeps every registered budget inside the deadline it claims", () => {
    for (const [name, spec] of Object.entries(POLL_BUDGETS)) {
      expect(
        pollDeadlineFits(spec.budgetMs, spec.parentTimeoutMs),
        `${name}: a ${spec.budgetMs}ms budget cannot fit under a ${spec.parentTimeoutMs}ms parent`,
      ).toBe(true);
    }
  });

  it("requires a stated reason before a budget may outgrow the suite timeout", () => {
    // "This one is allowed to be bigger" is exactly the claim that needs its
    // evidence attached, so the escape hatch costs a sentence naming the hook
    // that raises the deadline.
    for (const [name, spec] of Object.entries(POLL_BUDGETS)) {
      if (spec.parentTimeoutMs <= SUITE_TIMEOUT_MS) {
        expect(spec.reason, `${name} runs under the ordinary suite deadline`).toBeUndefined();
        continue;
      }
      expect(spec.reason, `${name} claims a raised parent deadline and must say why`).toBeTruthy();
      expect((spec.reason ?? "").length, `${name}'s reason must be a sentence`).toBeGreaterThan(40);
    }
  });

  it("keeps the ordinary case ordinary", () => {
    // Every exemption is named here. If this list grows, a helper has been
    // excused from the hierarchy, and that has to be a decision someone made
    // rather than a default someone raised. All three belong to a suite-level
    // hook that awaits a cold toolchain, and each appears in a sequence that
    // checks its parent against everything awaited beside it.
    const raised = Object.entries(POLL_BUDGETS).filter(
      ([, spec]) => spec.parentTimeoutMs > SUITE_TIMEOUT_MS,
    );
    expect(raised.map(([name]) => name).sort()).toEqual([
      "editorOwnedDiagnostics",
      "editorOwnedJsxSettle",
      "editorOwnedProjectHover",
      "editorOwnedSettle",
      "externalFileChangeSettle",
      "outOfTreeSettle",
      "outOfTreeStrictDiagnostic",
      "rootExtensionReady",
      "rootTypeProviderSync",
      "startupBenchmarkTiming",
    ]);
  });

  // ── Sequences ───────────────────────────────────────────────
  //
  // Individually-fitting budgets are not enough when they run one after another.
  // The root hook awaited 45s + 30s + 12s under a single 60s deadline: every
  // member "fit", the sum did not, and a late first wait killed the rest.

  it("fits every sequence's members under ONE parent, summed not maxed", () => {
    for (const [name, spec] of Object.entries(POLL_SEQUENCES)) {
      const total = pollSequenceTotalMs(name as keyof typeof POLL_SEQUENCES);
      expect(
        pollSequenceFits(name as keyof typeof POLL_SEQUENCES),
        `${name}: ${spec.members.join(" + ")} = ${total}ms cannot run in series under a ` +
          `${spec.parentTimeoutMs}ms parent`,
      ).toBe(true);
      // The check must be a SUM. `>= largest` did NOT say that: a maximum EQUALS
      // the largest, so a degraded implementation passed it — and because every
      // member also fits the parent on its own, `pollSequenceFits` above stayed
      // green too, leaving the control proving nothing. A multi-member sequence's
      // total must STRICTLY exceed its largest member; only summation does that.
      const largest = Math.max(...spec.members.map((member) => POLL_BUDGETS[member].budgetMs));
      if (spec.members.length > 1) {
        expect(
          total,
          `${name} totals ${total}ms across ${spec.members.length} members whose largest is ` +
            `${largest}ms — a total that merely equals the largest is a maximum, not a sum`,
        ).toBeGreaterThan(largest);
      } else {
        expect(total, `${name} has a single member and must equal it`).toBe(largest);
      }
    }
  });

  it("makes every sequence say why its parent is raised", () => {
    for (const [name, spec] of Object.entries(POLL_SEQUENCES)) {
      expect(spec.reason, `${name} must state why it needs its own deadline`).toBeTruthy();
      expect(spec.reason.length, `${name}'s reason must be a sentence`).toBeGreaterThan(40);
      expect(spec.members.length, `${name} must name the budgets it holds`).toBeGreaterThan(0);
    }
  });

  it("holds every exempt budget inside a sequence that accounts for it", () => {
    // An exemption is only legitimate as part of a series someone modelled. A
    // budget claiming a raised parent while belonging to no sequence is a budget
    // whose parent nobody checked.
    const sequenced = new Set(Object.values(POLL_SEQUENCES).flatMap((spec) => spec.members));
    for (const [name, spec] of Object.entries(POLL_BUDGETS)) {
      if (spec.parentTimeoutMs <= SUITE_TIMEOUT_MS) continue;
      expect(
        sequenced.has(name as keyof typeof POLL_BUDGETS),
        `${name} claims a raised parent but appears in no sequence, so nothing checks that ` +
          "parent against everything else awaited beside it",
      ).toBe(true);
    }
  });
});
