/**
 * The E2E suite's deadline hierarchy.
 *
 * A polling helper is a CHILD deadline of the mocha test that awaits it. When a
 * child budget is greater than or equal to its parent's, the parent kills the
 * child first and the run reports `Timeout of Nms exceeded` — never the assertion
 * the test wrote. That is not a slower failure, it is a DIFFERENT failure: a test
 * whose body branches on "the provider returned nothing here" can never reach that
 * branch, so a legitimately empty result is indistinguishable from a hang.
 *
 * Both numbers live here so the relationship is stated once and can be checked,
 * rather than restated as unrelated literals in `suite/index.ts` and `helpers.ts`.
 */

/**
 * The mocha timeout every suite runs under (`e2e/suite/index.ts`).
 *
 * 30s, not 15s, because COMPOSITION is the normal shape here: nearly every test
 * opens a document (one readiness wait) and then performs one feature wait. At
 * 15s only ONE budget could ever be spent, so the second was structurally unable
 * to reach its own deadline — an inversion by construction rather than by
 * mistake. Deriving the budget from the timeout instead would recreate the
 * problem, so {@link DEFAULT_POLL_BUDGET_MS} is chosen for a SINGLE wait and this
 * is chosen to hold two of them plus the margin.
 */
export const SUITE_TIMEOUT_MS = 30_000;

/**
 * Headroom between a polling helper's budget and the suite timeout.
 *
 * The helper must still be able to run its final poll, return, and let the test
 * body execute its assertions and `console.log` before the parent fires.
 */
export const POLL_BUDGET_MARGIN_MS = 3_000;

/**
 * The default budget for one polling wait.
 *
 * Chosen for a SINGLE wait, deliberately NOT derived from
 * {@link SUITE_TIMEOUT_MS}: a derived budget consumes the whole deadline, so the
 * second wait in any composition has nothing left. Two of these plus the margin
 * fit inside the suite timeout, which is the shape almost every test has.
 */
export const DEFAULT_POLL_BUDGET_MS = 12_000;

/**
 * Whether a polling helper's budget leaves the awaiting test room to report its
 * own verdict.
 *
 * The relation is what matters, not either number alone: a 30 000 ms wait is
 * correct inside a test that called `this.timeout(60_000)` and wrong inside one
 * that did not.
 */
export function pollDeadlineFits(childBudgetMs: number, parentTimeoutMs: number): boolean {
  return childBudgetMs <= parentTimeoutMs - POLL_BUDGET_MARGIN_MS;
}

/**
 * Interval between `textDocument/references` polls while the workspace-symbol
 * frontier is still converging.
 *
 * Deliberately slow, and the slowness is load-bearing. Measured on
 * `single-project@tsgo` (macOS, debug LSP), same tree and binary, only this
 * number changed:
 *
 * | interval | references answered |
 * | -------- | ------------------- |
 * | 200 ms   | 3 of 10 runs        |
 * | 2000 ms  | 7 of 7 runs         |
 *
 * A 5 Hz poll keeps the frontier pinned at `activated 24/37 carriers` for the
 * whole budget; a 0.5 Hz poll lets it settle in about four to six seconds. Each
 * refusal re-signals every expected source through the scanner lock
 * (`verter_lsp::server::provider_state::signal_frontier_scanner_priority`), so
 * polling faster than the frontier advances appears to starve the background
 * publication that would advance it.
 *
 * That rate sensitivity is a PRODUCT observation, not something this harness
 * can fix, and this constant does not make it go away — it only keeps the E2E
 * suite from being the thing that triggers it.
 */
export const REFERENCES_POLL_INTERVAL_MS = 2_000;

/** A test that awaits a hover and then a completion needs room for both. */
export const IMPORTED_PROPS_TEST_TIMEOUT_MS = 30_000;

/** The editor-owned acceptance's `suiteSetup` awaits three settles in series. */
export const EDITOR_OWNED_SETUP_TIMEOUT_MS = 60_000;

/** The startup-benchmark SUITE's own declared deadline (`startupBenchmark.test.ts`). */
export const STARTUP_SUITE_TIMEOUT_MS = 90_000;

/** The out-of-tree acceptance's `suiteSetup` waits on a second, cold toolchain. */
export const OUT_OF_TREE_SETUP_TIMEOUT_MS = 90_000;

/** The deadline the mocha root `beforeAll` must carry to hold its three waits. */
export const ROOT_HOOK_TIMEOUT_MS = 90_000;

/**
 * One waiting helper's default budget, and the test deadline it presumes.
 *
 * `parentTimeoutMs` is the claim that makes `budgetMs` legal. Most helpers are
 * awaited by an ordinary test and must fit under {@link SUITE_TIMEOUT_MS}; a few
 * are awaited only from hooks that raise their own deadline, and those say so
 * here — with a `reason`, because "this one is allowed to be bigger" is exactly
 * the sentence that needs evidence attached.
 */
export interface PollBudgetSpec {
  readonly budgetMs: number;
  /**
   * The SMALLEST deadline this budget is declared to run under — a lower bound,
   * not an equality, because a shared budget legitimately runs under many
   * different parents (`waitForFileReady` is awaited by 30s tests, 60s suites and
   * 90s hooks alike).
   *
   * It is load-bearing and CHECKED: at runtime `pollBudget` refuses a runnable
   * whose real deadline is BELOW this. A claim of 60s on a site that actually
   * runs under 30s is the shape that let an inverted composition pass the
   * sequence check — the total fitted the claim while the second wait could not
   * reach its deadline — so the claim being wrong is itself a failure.
   */
  readonly parentTimeoutMs: number;
  readonly reason?: string;
}

/**
 * Every default polling budget in the E2E harness, in one place.
 *
 * Each helper takes its default FROM here rather than spelling a literal, so a
 * budget and the invariant that checks it cannot drift: changing a number here
 * changes the helper, and `timeouts.unit.test.ts` fails if the new number does
 * not fit the deadline it claims.
 *
 * WHAT THIS COVERS, exactly, and HOW THE COUNT WAS DERIVED so the next reader can
 * re-run it rather than trust it. A polling SITE is either (a) a function whose
 * signature declares a budget default (`timeoutMs = <expr>`) or (b) an inline loop
 * bounded by a wall clock (`Date.now() + <expr>`, `Date.now() - start < <expr>`).
 * An inline bound that is simply the enclosing function's own `timeoutMs` is that
 * function's loop, already counted under (a), not a second site. `lib/fixtureLock`
 * is excluded: it guards a cross-process file lock, not a Mocha-surface poll.
 *
 * By that method, over `suite/**`, `lib/**` and `helpers.ts`: 15 default-budget
 * functions and 11 inline loops — 26 sites, all 26 registry-backed. Two earlier
 * counts in this file were asserted rather than derived and were both wrong; this
 * one states its method so the next one need not be taken on trust.
 *
 * WHAT IT DOES NOT COVER, and cannot:
 *
 * 1. A brand-new helper that spells a fresh literal and never registers. Only a
 *    name-keyed scanner over the source would see that, and this repo does not
 *    land scanners as guards.
 * 2. Replacing a registered helper's `pollBudget(...)` call with a literal while
 *    leaving its registry entry intact. The unit test reads the registry, so the
 *    entry would still look correct. Same reason: only a scanner sees it.
 * 3. A CALL SITE passing its own `timeoutMs`. A helper cannot see the deadline of
 *    the test awaiting it, so an override above that test's own timeout is
 *    invisible here. Those are fixed individually and re-audited by hand.
 * 4. A DECLARED SEQUENCE that nothing binds. Every sequence today is bound with
 *    `this.timeout(sequenceParent(...))`, which makes its claimed parent the
 *    deadline actually in force; a future sequence that is declared and never
 *    bound would have its total checked against a parent no runnable carries.
 *    Detecting that means scanning the suites for the binding call — the same
 *    forbidden shape as (1) and (2). ACCEPTED.
 * 6. EXACT-parent checking for a budget with a single owner. `activationHeartbeat`
 *    is evaluated from exactly one site, so its claim could be an equality rather
 *    than a floor — a strictly stronger check for that shape. Deliberately NOT
 *    implemented: it needs a per-entry mode, and the rule that decides which
 *    entries qualify is "how many sites evaluate it", which only the same
 *    forbidden scan can answer. RECORDED, not adopted.
 *
 * 5. A `parentTimeoutMs` that UNDERSTATES. The runtime check is a lower bound, so
 *    a claim smaller than reality passes. That direction is harmless — it only
 *    reserves less headroom than exists — and an equality is not available,
 *    because a shared budget legitimately runs under many different parents.
 *    ACCEPTED.
 *
 * Saying "every waiting helper is covered" would be the same overclaim this
 * harness exists to remove, so the boundary is written down instead.
 */
export const POLL_BUDGETS = {
  // ── helpers.ts ───────────────────────────────────────────────
  // These two come in PAIRS, and the split is the point. `ensureFixtureWarm` and
  // `ensureTypeProviderSynced` are memoised, so only the FIRST caller reaches the
  // wait — but any suite may call them, and a suiteSetup with no `this.timeout`
  // runs under the ordinary deadline. A single registry entry claiming the root
  // hook's 90s would be a claim ordinary suites falsify the moment they are the
  // first caller, and "the root hook always runs first" is true by execution
  // order, not by construction. So the large budget is `root*`, evaluated ONLY
  // where `suite/index.ts` passes it explicitly, and the default every other
  // caller gets is sized for the deadline they actually carry.
  rootExtensionReady: {
    budgetMs: 45_000,
    parentTimeoutMs: ROOT_HOOK_TIMEOUT_MS,
    reason:
      "passed explicitly by the mocha root `beforeAll`, whose deadline is derived from the " +
      "`rootBeforeAll` sequence; activation plus the first LSP start legitimately outruns any " +
      "single test's deadline, and no ordinary caller can evaluate this entry",
  },
  rootTypeProviderSync: {
    budgetMs: 30_000,
    parentTimeoutMs: ROOT_HOOK_TIMEOUT_MS,
    reason:
      "passed explicitly by the same root `beforeAll`: the provider handshake is a suite-level " +
      "precondition, not work done inside any one test, and no ordinary caller can evaluate it",
  },
  waitForExtensionReady: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForTypeProviderSync: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForFileReady: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForOnTypeReady: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForDiagnostics: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForNoDiagnosticsMatching: {
    budgetMs: DEFAULT_POLL_BUDGET_MS,
    parentTimeoutMs: SUITE_TIMEOUT_MS,
  },
  waitForDiagnosticsSettled: { budgetMs: 5_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  measureHover: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForHoverMatching: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  waitForCompletionsMatching: {
    budgetMs: DEFAULT_POLL_BUDGET_MS,
    parentTimeoutMs: SUITE_TIMEOUT_MS,
  },
  waitForCodeActionsMatching: {
    budgetMs: DEFAULT_POLL_BUDGET_MS,
    parentTimeoutMs: SUITE_TIMEOUT_MS,
  },
  waitForReferences: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  revealDefinition: { budgetMs: 10_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  editorOwnedProjectHover: {
    budgetMs: 20_000,
    parentTimeoutMs: 60_000,
    reason:
      "the editor-owned suite declares `this.timeout(60_000)`, which every test it creates " +
      "inherits; the 20s budget was reachable all along and was lowered on the refuted premise",
  },
  startupBenchmarkTiming: {
    budgetMs: 20_000,
    parentTimeoutMs: STARTUP_SUITE_TIMEOUT_MS,
    reason:
      "the startup benchmark waits for the FIRST typed completion of the whole run, and runs " +
      "under its own suite's declared 90s — NOT the root hook's, which happens to be the same " +
      "number today and is exactly the coincidence that hid two wrong parents",
  },
  decorationSettle: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  // Restored budgets. Each of these was lowered on the false premise that an
  // override above a 15s parent was unreachable; Mocha copies a SUITE-level
  // `this.timeout` into every hook and test declared after it
  // (`Suite.prototype.addTest` / `_createHook`), so these sites always had the
  // larger parent their suite declares, and the headroom was real.
  decorationSettleSlow: { budgetMs: 20_000, parentTimeoutMs: 30_000 },
  editorOwnedDiagnostics: {
    budgetMs: 30_000,
    parentTimeoutMs: 60_000,
    reason:
      "the editor-owned suite declares `this.timeout(60_000)`, which Mocha copies into every hook " +
      "and test it then creates; this wait is for a typed error from an editor-owned project",
  },
  editorOwnedSettle: {
    budgetMs: 10_000,
    parentTimeoutMs: 60_000,
    reason:
      "same suite-level 60s deadline; a diagnostics SETTLE needs longer than a first-arrival wait " +
      "because it must observe quiescence, not just one publication",
  },
  editorOwnedJsxSettle: {
    budgetMs: 20_000,
    parentTimeoutMs: 60_000,
    reason:
      "same suite-level 60s deadline; the carrier-owned JSX environment settles after a full " +
      "project load, which is the slowest settle in that acceptance",
  },
  outOfTreeSettle: {
    budgetMs: 10_000,
    parentTimeoutMs: OUT_OF_TREE_SETUP_TIMEOUT_MS,
    reason:
      "the out-of-tree suite declares `this.timeout(90_000)`; this settle follows a cold second " +
      "toolchain and shares that hook's deadline with it",
  },
  barrelDefinitionSettle: { budgetMs: DEFAULT_POLL_BUDGET_MS, parentTimeoutMs: SUITE_TIMEOUT_MS },
  // Its test lowers ITSELF to 15s — a parent SMALLER than the suite default. The
  // registry claimed the 30s suite parent, which would have accepted any budget up
  // to 27s while Mocha killed at 15s. The current 12s fits, but fitting by luck is
  // not the property under check.
  activationHeartbeat: { budgetMs: 12_000, parentTimeoutMs: 15_000 },
  // The standalone verter-mcp child starts in parallel with the LSP; by the
  // time `isLspReady()` holds it is usually already announced, but a debug
  // binary on a loaded CI runner can trail the (provider-independent) LSP
  // ready line, so the two MCP activation tests poll the log instead of
  // reading it once.
  activationMcpReady: { budgetMs: 20_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  diagnosticsUnresolvedRetry: { budgetMs: 20_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  diagnosticsClearRetry: { budgetMs: 20_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  confidenceProbe: { budgetMs: 10_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  externalFileChangeSettle: {
    budgetMs: 15_000,
    parentTimeoutMs: 60_000,
    reason:
      "the external-file-changes suite declares `this.timeout(60_000)`; these waits span a real " +
      "on-disk write and the watcher round-trip that follows it, not just a provider query",
  },
  outOfTreeStrictDiagnostic: {
    budgetMs: 60_000,
    parentTimeoutMs: OUT_OF_TREE_SETUP_TIMEOUT_MS,
    reason:
      "the out-of-tree acceptance waits for TS2322 from the PACKAGE's own strict config through " +
      "its own compiler — a cold second toolchain, and waiting on that specific code (not on any " +
      "diagnostic) is what makes a fail-closed run time out here instead of passing on an empty set",
  },
  // ── lib/** ───────────────────────────────────────────────────
  frameworkContractSettle: { budgetMs: 10_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  parityHarnessSettle: { budgetMs: 12_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
  completionContractSettle: { budgetMs: 8_000, parentTimeoutMs: SUITE_TIMEOUT_MS },
} as const satisfies Record<string, PollBudgetSpec>;

export type PollBudgetName = keyof typeof POLL_BUDGETS;

/**
 * Budgets awaited ONE AFTER ANOTHER under a single parent deadline.
 *
 * Checking each child against a fresh parent is not enough when they run in
 * series: the root `beforeAll` awaits activation, then provider sync, then file
 * readiness, and every one of those was individually "under 60 seconds" while
 * their sum was 87. A late first wait leaves the second unable to reach its own
 * deadline at all, and the run reports a hook timeout naming none of them.
 *
 * So a sequence declares its members and the parent that must hold all of them.
 * `timeouts.unit.test.ts` checks the SUM, not the maximum.
 */
export interface PollSequenceSpec {
  readonly members: readonly PollBudgetName[];
  readonly parentTimeoutMs: number;
  readonly reason: string;
}

export const POLL_SEQUENCES = {
  rootBeforeAll: {
    members: ["rootExtensionReady", "rootTypeProviderSync", "waitForFileReady"],
    parentTimeoutMs: ROOT_HOOK_TIMEOUT_MS,
    reason:
      "`suite/index.ts` root beforeAll awaits ensureFixtureWarm (extension ready), then " +
      "ensureTypeProviderSynced (provider sync), then openReadyCached (file ready) — in series, " +
      "under ONE deadline. At 60s the second and third could be killed before reaching their own.",
  },
  importedPropsHoverThenCompletion: {
    members: ["waitForHoverMatching", "waitForCompletionsMatching"],
    parentTimeoutMs: IMPORTED_PROPS_TEST_TIMEOUT_MS,
    reason:
      "one test awaits an imported-prop hover and then a member completion on the same " +
      "document; both are cold on first touch, so the test raises its own deadline to hold both.",
  },
  editorOwnedProjectSetup: {
    members: ["editorOwnedDiagnostics", "editorOwnedSettle", "editorOwnedSettle"],
    parentTimeoutMs: EDITOR_OWNED_SETUP_TIMEOUT_MS,
    reason:
      "the acceptance's suiteSetup waits for a typed error on the component, then settles that " +
      "document, then settles a second one — three waits, in series, under the hook's one deadline",
  },
  outOfTreeMonorepoSetup: {
    members: ["outOfTreeStrictDiagnostic", "outOfTreeSettle"],
    parentTimeoutMs: OUT_OF_TREE_SETUP_TIMEOUT_MS,
    reason:
      "the out-of-tree suiteSetup waits for the package's own compiler to produce TS2322 and then " +
      "settles the document, in series under one hook deadline",
  },
  editorOwnedCarrierHovers: {
    members: ["editorOwnedProjectHover", "editorOwnedProjectHover"],
    parentTimeoutMs: 60_000,
    reason:
      "the carrier-hover test waits on the component and then on its consumer, in series, under " +
      "the editor-owned suite's inherited 60s deadline",
  },
  editorOwnedJsxEnvironment: {
    members: ["editorOwnedJsxSettle"],
    parentTimeoutMs: 60_000,
    reason:
      "one wait, declared so its raised parent is checked against what runs beside it — which is " +
      "nothing else, and saying so is the point",
  },
  externalFileChangeRoundTrip: {
    members: ["externalFileChangeSettle", "externalFileChangeSettle"],
    parentTimeoutMs: 60_000,
    reason:
      "the delete and modify tests each wait twice: once for the watcher to notice the on-disk " +
      "change and once for diagnostics to settle after it, in series",
  },
  startupBenchmarkFirstCompletion: {
    members: ["startupBenchmarkTiming"],
    parentTimeoutMs: STARTUP_SUITE_TIMEOUT_MS,
    reason:
      "one wait for the run's first typed completion, declared so its raised parent is checked " +
      "rather than assumed",
  },
  hoverSlotOutletRoundTrip: {
    members: ["waitForFileReady", "waitForHoverMatching", "waitForFileReady"],
    parentTimeoutMs: 45_000,
    reason:
      "the slot-outlet case opens a second carrier, waits for the native contribution on it, then " +
      "reopens the suite's own document — three waits in series, and the last one exists so the " +
      "cases after it do not inherit a different open document",
  },
  hoverSlotNameRoundTrip: {
    members: ["waitForFileReady", "measureHover", "waitForFileReady"],
    parentTimeoutMs: 45_000,
    reason:
      "same open-hover-reopen shape as the slot-outlet case, on the slot NAME attribute value",
  },
  hoverVSlotLocalsAndMembers: {
    members: [
      "waitForFileReady",
      "waitForHoverMatching",
      "waitForHoverMatching",
      "measureHover",
      "measureHover",
    ],
    parentTimeoutMs: 70_000,
    reason:
      "the v-slot case opens a carrier and then probes four positions — the slot local and its " +
      "member, each first waited for and then measured — all in series in one test",
  },
} as const satisfies Record<string, PollSequenceSpec>;

export type PollSequenceName = keyof typeof POLL_SEQUENCES;

/** The deadline a sequence's parent must carry. */
export function sequenceParent(name: PollSequenceName): number {
  return POLL_SEQUENCES[name].parentTimeoutMs;
}

/** Total budget a sequence consumes if every member runs to its deadline. */
export function pollSequenceTotalMs(name: PollSequenceName): number {
  return POLL_SEQUENCES[name].members.reduce((sum, member) => sum + pollBudget(member), 0);
}

/** Whether a sequence's members all fit, in series, under its declared parent. */
export function pollSequenceFits(name: PollSequenceName): boolean {
  return pollDeadlineFits(pollSequenceTotalMs(name), POLL_SEQUENCES[name].parentTimeoutMs);
}

// ── Verifying the parent, rather than believing it ──────────────
//
// Every `parentTimeoutMs` above is a CLAIM about a deadline declared somewhere
// else, and three of them were simply wrong: two named the suite default while
// their site inherited or lowered to something else, and one named the right
// number bound to the wrong owner. A wrong claim is worse than no claim, because
// the guard then launders it.
//
// The claim cannot be checked statically without parsing `suite/**` for `suite(`,
// `test(` and `this.timeout(` — a name-keyed scanner over the source, which this
// repo does not land as a guard. So it is not inferred: it is OBSERVED. Mocha
// knows each runnable's real deadline, the suite runner hands that runnable to
// this module, and `pollBudget` compares the budget it is about to hand out
// against the deadline actually in force. A parent claim that disagrees with
// Mocha fails the run at the call site, naming both numbers.

let currentRunnable: (() => { title: string; timeout: () => number } | undefined) | undefined;

/** Let the suite runner tell this module which runnable is executing. */
export function setRunnableAccessor(
  accessor: (() => { title: string; timeout: () => number } | undefined) | undefined,
): void {
  currentRunnable = accessor;
}

/**
 * The default budget for a registered waiting helper, checked against the
 * deadline actually in force.
 *
 * Outside a run (unit tests, tooling) there is no runnable and the check is
 * skipped — the static invariants in `timeouts.unit.test.ts` cover that case.
 */
export function pollBudget(name: PollBudgetName): number {
  const spec = POLL_BUDGETS[name];
  const runnable = currentRunnable?.();
  if (runnable) {
    const actual = runnable.timeout();
    if (actual > 0 && actual < spec.parentTimeoutMs) {
      // The CLAIM is wrong, whether or not this particular budget still fits.
      // A claim larger than reality is what launders an inverted composition:
      // the sequence total is checked against the claim, so a 40s total "fits"
      // a claimed 60s parent while the runnable really has 30s and the second
      // wait can never reach its deadline.
      const containing = Object.entries(POLL_SEQUENCES)
        .filter(([, sequence]) => (sequence.members as readonly string[]).includes(name))
        .map(([sequenceName]) => sequenceName);
      throw new Error(
        `poll budget "${name}" declares it runs under at least ${spec.parentTimeoutMs}ms, but ` +
          `"${runnable.title}" is running under ${actual}ms. Everything checked against that ` +
          `claim was measured against a deadline that does not exist${
            containing.length > 0
              ? `, including ${containing.length > 1 ? "sequences" : "sequence"} ` +
                containing.map((sequenceName) => `"${sequenceName}"`).join(", ")
              : ""
          }.`,
      );
    }
    if (actual > 0 && !pollDeadlineFits(spec.budgetMs, actual)) {
      throw new Error(
        `poll budget "${name}" is ${spec.budgetMs}ms and the registry claims a ` +
          `${spec.parentTimeoutMs}ms parent, but "${runnable.title}" is running under ` +
          `${actual}ms — the budget cannot reach its own deadline, so a failure here would ` +
          "report a Mocha timeout instead of whatever the test was measuring",
      );
    }
  }
  return spec.budgetMs;
}
