import { describe, expect, it } from "vitest";

import {
  collectorEvent,
  foldOutcome,
  baselineDisagreement,
  skipped,
  type CollectorEvent,
  type CollectorEventKey,
  type CollectorSignal,
  type Divergence,
  type Probe,
  type Severity,
} from "../src/index.js";
import {
  buildBaselineManifest,
  buildSummary,
  computeFindingFingerprint,
  reduceFindings,
  renderFindingsMarkdown,
  renderJunitXml,
  type BenignDivergenceAllowlist,
  type ScenarioIndex,
  type SituatedOutcome,
} from "../src/report/index.js";

// ── fixtures ───────────────────────────────────────────────────────────────────

const SCENARIOS: ScenarioIndex = {
  "member-access": {
    fixture: "minimal-member-access",
    probes: {
      "p-complete": { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" },
      "p-hover": { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" },
      "p-diag": { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" },
    },
  },
};

function key(overrides: Partial<CollectorEventKey> = {}): CollectorEventKey {
  return {
    scenario: "member-access",
    editStepIndex: 0,
    driver: "rawLsp",
    provider: "tsgo-rust",
    probe: "p-complete",
    version: 1,
    anchor: "cursor",
    ...overrides,
  };
}

function event(
  signal: CollectorSignal,
  ok: boolean,
  severity: Severity,
  k: CollectorEventKey,
  detail: string,
  data?: unknown,
): CollectorEvent {
  return collectorEvent({
    collector: "completion",
    signal,
    ok,
    severity,
    provenance: { detectedBy: k.driver },
    key: k,
    detail,
    ...(data !== undefined ? { data } : {}),
  });
}

function probe(overrides: Partial<Probe> = {}): Probe {
  return {
    id: "p-x",
    method: "completion",
    anchor: "cursor",
    mappingPolicy: "strict",
    confidence: "high",
    dimension: "artifactParity",
    requiresSourceMap: true,
    requiredDrivers: ["rawLsp", "tsgo"],
    capabilityRequirements: [],
    ...overrides,
  };
}

// ── fingerprint ─────────────────────────────────────────────────────────────────

describe("computeFindingFingerprint — content-addressed, stable, discriminating", () => {
  const base = {
    scenario: "member-access",
    signal: "completion_parity" as const,
    divergenceKind: "missingLabel" as const,
    semanticDimension: "artifactParity" as const,
    expected: "tsgo: [foo, bar]",
    actual: "verter: [foo]",
    rootCauseHint: null,
  };

  it("is a 64-char sha256 hex digest", () => {
    expect(computeFindingFingerprint(base)).toMatch(/^[0-9a-f]{64}$/);
  });

  it("is identical for EOL-only differences in expected/actual/rootCauseHint", () => {
    const crlf = { ...base, expected: "tsgo:\r\n[foo, bar]", actual: "verter:\r\n[foo]" };
    const lf = { ...base, expected: "tsgo:\n[foo, bar]", actual: "verter:\n[foo]" };
    expect(computeFindingFingerprint(crlf)).toBe(computeFindingFingerprint(lf));
  });

  it("changes when ANY tuple field changes (scenario/signal/divergence/dimension/expected/actual/hint)", () => {
    const fp = computeFindingFingerprint(base);
    expect(computeFindingFingerprint({ ...base, scenario: "other" })).not.toBe(fp);
    expect(computeFindingFingerprint({ ...base, signal: "hover_parity" })).not.toBe(fp);
    expect(computeFindingFingerprint({ ...base, divergenceKind: "extraLabel" })).not.toBe(fp);
    expect(
      computeFindingFingerprint({ ...base, semanticDimension: "vueSemanticValidity" }),
    ).not.toBe(fp);
    expect(computeFindingFingerprint({ ...base, actual: "verter: [foo, bar]" })).not.toBe(fp);
    expect(computeFindingFingerprint({ ...base, rootCauseHint: "mapping failed" })).not.toBe(fp);
  });
});

describe("reduceFindings — fingerprint excludes volatile run data", () => {
  it("two findings differing ONLY in version/editStep/latency/event-id share a fingerprint", () => {
    const div = { class: "missingLabel", verterValue: ["foo"], baselineValue: ["foo", "bar"] };
    const runA = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "completion_parity",
            false,
            "userVisible",
            key({ version: 1, editStepIndex: 2 }),
            "verter missing label bar",
            { ...div, latencyMs: 12 },
          ),
        },
      ],
    });
    const runB = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "completion_parity",
            false,
            "userVisible",
            key({ version: 99, editStepIndex: 88 }),
            "verter missing label bar",
            { ...div, latencyMs: 9999 },
          ),
        },
      ],
    });
    expect(runA.findings).toHaveLength(1);
    expect(runB.findings).toHaveLength(1);
    expect(runA.findings[0].fingerprint).toBe(runB.findings[0].fingerprint);
    // the divergence class travelled from the event payload into the finding.
    expect(runA.findings[0].divergence).toBe("missingLabel");
  });
});

describe("reduceFindings — fingerprint excludes per-run temp paths and measured detail", () => {
  // A definition divergence whose structured value carries the materialized
  // workspace's random temp-root uri — the per-run-volatile path the fingerprint
  // must NOT hash. `file` lets a genuinely-different target be expressed.
  const definitionFinding = (root: string, file = "src/Foo.vue") => {
    const uri = `file://${root}/${file}`;
    return reduceFindings({
      scenarios: SCENARIOS,
      workspaceRoot: root,
      events: [
        {
          event: event(
            "definition_parity",
            false,
            "userVisible",
            key({ probe: "p-complete" }),
            `verter resolved a definition in ${uri} the baseline did not`,
            {
              class: "verterOnly",
              verterValue: {
                uri,
                range: { start: { line: 3, character: 2 }, end: { line: 3, character: 5 } },
              },
            },
          ),
        },
      ],
    }).findings[0];
  };

  it("a missing/wrong definition fingerprints identically across per-run temp roots", () => {
    const a = definitionFinding("/tmp/dx-ws-AAAAAA");
    const b = definitionFinding("/tmp/dx-ws-BBBBBB");
    expect(a.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    // the random temp segment differs, the logical target does not → one id.
    expect(a.fingerprint).toBe(b.fingerprint);
    // but a genuinely different target file is still a different finding.
    const other = definitionFinding("/tmp/dx-ws-AAAAAA", "src/Bar.vue");
    expect(other.fingerprint).not.toBe(a.fingerprint);
  });

  // A server-error finding falls back to free-text `detail`; the raw log line is
  // timestamp-bearing, so the fingerprint must key on the failure category, not the line.
  const serverErrorFinding = (timestamp: string, message: string) => {
    const line = `${timestamp} ERROR ${message}`;
    return reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "server_error",
            false,
            "userVisible",
            key({ probe: "p-diag" }),
            `server logged an error: ${line}`,
            { line },
          ),
        },
      ],
    }).findings[0];
  };

  it("a server error fingerprints identically across log timestamps", () => {
    const a = serverErrorFinding("2024-01-01T00:00:00.000Z", "verter_session: load failed");
    const b = serverErrorFinding("2025-12-31T23:59:59.999Z", "verter_session: load failed");
    expect(a.fingerprint).toBe(b.fingerprint);
    // a different failure category is still a different finding.
    const c = serverErrorFinding("2024-01-01T00:00:00.000Z", "verter_session: panic in resolver");
    expect(c.fingerprint).not.toBe(a.fingerprint);
  });

  // A churn breach falls back to `detail`, which embeds the measured delta; the
  // fingerprint must key on the scope + breach, never the volatile delta magnitude.
  const churnFinding = (delta: number) =>
    reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "churn_steady_state_delta",
            false,
            "userVisible",
            key({ probe: "p-diag" }),
            `steady-state compile delta ${delta} (threshold 3)`,
            { scope: "steadyStateQuiescedEdit", delta, attributable: true },
          ),
        },
      ],
    }).findings[0];

  it("a churn breach fingerprints identically across delta magnitudes", () => {
    expect(churnFinding(5).fingerprint).toBe(churnFinding(99999).fingerprint);
  });
});

// ── severity ladder ─────────────────────────────────────────────────────────────

describe("reduceFindings — S0–S4 severity ladder", () => {
  it("maps a 3-level `critical` event to S0", () => {
    const { findings } = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event("server_error", false, "critical", key({ probe: "p-diag" }), "server died"),
        },
      ],
    });
    expect(findings[0].severity).toBe("S0");
  });

  it("maps a diagnostics false-red (verterOnly) to S1 and a default-range to S2", () => {
    const { findings } = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "diagnostics_parity",
            false,
            "userVisible",
            key({ probe: "p-diag" }),
            "verter emitted a diagnostic the baseline did not",
            { class: "verterOnly" },
          ),
        },
        {
          event: event(
            "diagnostics_default_range",
            false,
            "userVisible",
            key({ probe: "p-diag", anchor: "decl" }),
            "diagnostic collapsed to (0,0)",
          ),
        },
      ],
    });
    const bySignal = Object.fromEntries(findings.map((f) => [f.signal, f.severity]));
    expect(bySignal.diagnostics_parity).toBe("S1");
    expect(bySignal.diagnostics_default_range).toBe("S2");
  });

  it("escalates a No-Suggestions collapse from S2 (raw-LSP) to S1 (extension-host confirmed)", () => {
    const raw = reduceFindings({
      scenarios: SCENARIOS,
      events: [{ event: event("no_suggestions_collapse", false, "candidate", key(), "empty") }],
    });
    const ext = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "no_suggestions_collapse",
            false,
            "candidate",
            key({ driver: "extensionHost" }),
            "empty",
          ),
        },
      ],
    });
    expect(raw.findings[0].severity).toBe("S2");
    expect(ext.findings[0].severity).toBe("S1");
  });

  it("maps a latency breach to S3 and a tsgo-vs-tsserver baseline disagreement to S4", () => {
    const latency = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        { event: event("latency_breach", false, "userVisible", key(), "p95 over threshold") },
      ],
    });
    const disagreement: SituatedOutcome = {
      scenario: "member-access",
      driver: "rawLsp",
      outcome: baselineDisagreement(
        probe({ id: "p-complete" }),
        ["tsgo", "tsserver"],
        [{ class: "missingLabel", detail: "tsgo and tsserver disagree" }],
      ),
    };
    const { findings } = reduceFindings({ scenarios: SCENARIOS, outcomes: [disagreement] });
    expect(latency.findings[0].severity).toBe("S3");
    expect(findings[0].severity).toBe("S4");
    expect(findings[0].findingKind).toBe("providerDisagreement");
  });
});

// ── dedupe ──────────────────────────────────────────────────────────────────────

describe("reduceFindings — dedupe", () => {
  it("collapses a repeated-keystroke burst into one finding with worst severity + first/last ids", () => {
    const burst: CollectorEvent[] = [
      event("no_suggestions_collapse", false, "candidate", key({ editStepIndex: 1 }), "empty@1"),
      event("no_suggestions_collapse", false, "candidate", key({ editStepIndex: 2 }), "empty@2"),
      // a later keystroke is observed at the extension-host (escalates the SAME collapse).
      event(
        "no_suggestions_collapse",
        false,
        "candidate",
        key({ editStepIndex: 3, driver: "extensionHost" }),
        "empty@3",
      ),
    ];
    const { findings } = reduceFindings({
      scenarios: SCENARIOS,
      events: burst.map((e) => ({ event: e })),
    });
    expect(findings).toHaveLength(1);
    expect(findings[0].events.count).toBe(3);
    expect(findings[0].events.first).not.toBe(findings[0].events.last);
    // worst severity across the collapsed keystrokes wins (extension-host confirmed → S1).
    expect(findings[0].severity).toBe("S1");
  });

  it("keeps a provider disagreement SEPARATE from a verter divergence of the same class", () => {
    const verterDivergence = event(
      "completion_parity",
      false,
      "userVisible",
      key(),
      "verter missing label bar",
      { class: "missingLabel", verterValue: ["foo"], baselineValue: ["foo", "bar"] },
    );
    const providerDisagreement: SituatedOutcome = {
      scenario: "member-access",
      driver: "rawLsp",
      outcome: baselineDisagreement(
        probe({ id: "p-complete" }),
        ["tsgo", "tsserver"],
        [{ class: "missingLabel", detail: "providers disagree on bar" }],
      ),
    };
    const { findings } = reduceFindings({
      scenarios: SCENARIOS,
      events: [{ event: verterDivergence }],
      outcomes: [providerDisagreement],
    });
    expect(findings).toHaveLength(2);
    const kinds = findings.map((f) => f.findingKind).sort();
    expect(kinds).toEqual(["providerDisagreement", "verterDefect"]);
  });
});

// ── allowlist ─────────────────────────────────────────────────────────────────

describe("reduceFindings — benign-divergence allowlist", () => {
  const falseRed = (): CollectorEvent =>
    event(
      "diagnostics_parity",
      false,
      "userVisible",
      key({ probe: "p-diag" }),
      "verter emitted a diagnostic the baseline did not",
      { class: "verterOnly" },
    );

  it("reclassifies a matched divergence to S4 and records the hit (never silently dropped)", () => {
    const allowlist: BenignDivergenceAllowlist = {
      version: 1,
      entries: [
        {
          id: "known-benign-diag",
          reason: "transitional diagnostic, known benign",
          match: {
            fixture: "minimal-member-access",
            scenario: "member-access",
            signal: "diagnostics_parity",
            divergenceKind: "verterOnly",
            semanticDimension: "artifactParity",
          },
        },
      ],
    };
    const { findings, allowlistHits } = reduceFindings({
      scenarios: SCENARIOS,
      events: [{ event: falseRed() }],
      allowlist,
    });
    expect(findings).toHaveLength(1);
    expect(findings[0].severity).toBe("S4");
    expect(findings[0].allowlisted?.entryId).toBe("known-benign-diag");
    expect(allowlistHits).toHaveLength(1);
    expect(allowlistHits[0].entryId).toBe("known-benign-diag");
  });

  it("leaves an unmatched divergence at its real severity (S1)", () => {
    const allowlist: BenignDivergenceAllowlist = {
      version: 1,
      entries: [{ id: "other", reason: "x", match: { scenario: "different-scenario" } }],
    };
    const { findings, allowlistHits } = reduceFindings({
      scenarios: SCENARIOS,
      events: [{ event: falseRed() }],
      allowlist,
    });
    expect(findings[0].severity).toBe("S1");
    expect(findings[0].allowlisted).toBeUndefined();
    expect(allowlistHits).toHaveLength(0);
  });

  it("never benignifies an S0: a matching entry still fails junit and counts in failures", () => {
    // An allowlist that WOULD match the crash by its dedupe key.
    const allowlist: BenignDivergenceAllowlist = {
      version: 1,
      entries: [
        {
          id: "would-suppress-crash",
          reason: "someone tried to allowlist a server death",
          match: { fixture: "minimal-member-access", scenario: "member-access" },
        },
      ],
    };
    const { findings, allowlistHits } = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        // a 3-level `critical` event is the S0 crash/hang floor.
        {
          event: event("server_error", false, "critical", key({ probe: "p-diag" }), "server died"),
        },
      ],
      allowlist,
    });
    expect(findings).toHaveLength(1);
    expect(findings[0].severity).toBe("S0");
    // the allowlist match is ignored ENTIRELY for an S0 — no annotation, no hit.
    expect(findings[0].allowlisted).toBeUndefined();
    expect(allowlistHits).toHaveLength(0);
    // end-to-end: the crash emits a junit <failure> and counts under totals.failures.
    expect(renderJunitXml(findings)).toMatch(/<failure\b/);
    const summary = buildSummary({
      findings,
      baselineRan: { probes: 0, probeIds: [] },
      allowlistHits,
      allowlistVersion: 1,
    });
    expect(summary.totals.failures).toBe(1);
    expect(summary.totals.allowlisted).toBe(0);
  });
});

// ── baseline-ran / manifest ─────────────────────────────────────────────────────

describe("reduceFindings — baseline execution accounting", () => {
  it("counts distinct probes a baseline executed, across passing AND failing observations", () => {
    const agreementOutcome: SituatedOutcome = {
      scenario: "member-access",
      driver: "rawLsp",
      outcome: foldOutcome(probe({ id: "p-hover", method: "hover" }), "tsgo", []),
    };
    const okParity = event("completion_parity", true, "userVisible", key(), "agrees");
    const { baselineRan } = reduceFindings({
      scenarios: SCENARIOS,
      events: [{ event: okParity }],
      outcomes: [agreementOutcome],
    });
    expect(baselineRan.probeIds).toEqual(["p-complete", "p-hover"]);
    expect(baselineRan.probes).toBe(2);
  });

  it("buildBaselineManifest attributes ran probes per provider (both sides of a disagreement)", () => {
    const outcomes: SituatedOutcome[] = [
      {
        scenario: "member-access",
        driver: "rawLsp",
        outcome: foldOutcome(probe({ id: "p-complete" }), "tsgo", [
          { class: "missingLabel", detail: "x" } satisfies Divergence,
        ]),
      },
      {
        scenario: "member-access",
        driver: "rawLsp",
        outcome: baselineDisagreement(
          probe({ id: "p-hover", method: "hover" }),
          ["tsgo", "tsserver"],
          [{ class: "typeLabelMismatch", detail: "y" }],
        ),
      },
    ];
    const manifest = buildBaselineManifest(outcomes);
    expect(manifest.providers.tsgo.ranProbeIds).toEqual(["p-complete", "p-hover"]);
    expect(manifest.providers.tsserver.ranProbeIds).toEqual(["p-hover"]);
    expect(manifest.totalExecutions).toBe(3);
  });
});

// ── summary ─────────────────────────────────────────────────────────────────────

describe("buildSummary — counts by severity / dimension / signal", () => {
  it("tallies findings into a deterministic summary with the reconciliation metadata", () => {
    const { findings, baselineRan, allowlistHits } = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "diagnostics_parity",
            false,
            "userVisible",
            key({ probe: "p-diag" }),
            "false red",
            { class: "verterOnly" },
          ),
        },
        { event: event("latency_breach", false, "userVisible", key(), "slow") },
      ],
    });
    const summary = buildSummary({
      findings,
      baselineRan,
      allowlistHits,
      allowlistVersion: 1,
    });
    expect(summary.totals.findings).toBe(2);
    expect(summary.bySeverity).toEqual({ S0: 0, S1: 1, S2: 0, S3: 1, S4: 0 });
    expect(summary.byDimension.artifactParity).toBe(2);
    expect(summary.bySignal).toEqual({ diagnostics_parity: 1, latency_breach: 1 });
    // absent reconciliation defaults to the documented skipped-missing-file metadata.
    expect(summary.bugReportReconciliation.status).toBe("skipped_missing_file");
  });
});

// ── markdown ─────────────────────────────────────────────────────────────────────

describe("renderFindingsMarkdown — deterministic, fingerprint-bearing", () => {
  const build = () =>
    reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "completion_parity",
            false,
            "userVisible",
            key(),
            "verter missing label bar",
            { class: "missingLabel", verterValue: ["foo"], baselineValue: ["foo", "bar"] },
          ),
          rootCauseHint: "completion: position mapping failed",
        },
      ],
    }).findings;

  it("renders each finding with its fingerprint, scenario, severity, verter-vs-baseline, divergence, hint", () => {
    const findings = build();
    const md = renderFindingsMarkdown(findings);
    const fp = findings[0].fingerprint;
    expect(md).toContain("# Verter DX Findings");
    expect(md).toContain(fp);
    expect(md).toContain("member-access");
    expect(md).toContain("S2");
    expect(md).toContain("missingLabel");
    expect(md).toContain("completion: position mapping failed");
    // determinism: an INDEPENDENTLY-built finding set renders byte-identical markdown
    // (catches any timestamp / iteration-order nondeterminism).
    expect(renderFindingsMarkdown(build())).toBe(md);
  });

  it("renders a skip reason for a skipped outcome", () => {
    const skip: SituatedOutcome = {
      scenario: "member-access",
      driver: "rawLsp",
      outcome: skipped(probe({ id: "p-complete" }), { reason: "no tsserver baseline available" }),
    };
    const { findings } = reduceFindings({ scenarios: SCENARIOS, outcomes: [skip] });
    expect(findings[0].skipReason).toBe("no tsserver baseline available");
    expect(renderFindingsMarkdown(findings)).toContain("no tsserver baseline available");
  });

  it("renders an empty baseline behavior as a dash, not an empty code span", () => {
    const { findings } = reduceFindings({
      scenarios: SCENARIOS,
      events: [
        {
          event: event(
            "no_suggestions_collapse",
            false,
            "candidate",
            key(),
            "verter offered nothing",
          ),
        },
      ],
    });
    // this finding has no baseline side → an empty behavior string.
    expect(findings[0].baselineBehavior).toBe("");
    const md = renderFindingsMarkdown(findings);
    expect(md).toContain("- baseline: —");
    // the bare empty code span ``` `` ``` must never be rendered.
    expect(md).not.toContain("- baseline: ``");
  });
});
