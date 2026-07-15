import { describe, expect, it } from "vitest";

import {
  baselineDisagreement,
  collectorEvent,
  foldOutcome,
  skipped,
  type CollectorEvent,
  type CollectorEventKey,
  type CollectorSignal,
  type Probe,
  type Severity,
} from "../src/index.js";
import {
  buildBaselineManifest,
  buildSummary,
  reduceFindings,
  renderFindingsMarkdown,
  renderJunitXml,
  serializeBaselineManifest,
  serializeSummary,
  type BenignDivergenceAllowlist,
  type ScenarioIndex,
  type SituatedOutcome,
} from "../src/report/index.js";

// ── a FIXED, path-free input set (deterministic across runs and machines) ─────────
//
// These goldens pin the EXACT serialized bytes of all four durable artifacts over one
// fixed input, so any change to a serializer's format, field order, or content fails
// loudly. The fixed input deliberately spans the S0–S4 ladder, both dimensions, the
// XML-hostile-text escaping path, an empty-baseline em-dash, a recorded allowlist hit,
// a provider disagreement, and a skip. Regenerate by serializing this same input.

const SCENARIOS: ScenarioIndex = {
  "member-access": {
    fixture: "minimal-member-access",
    probes: {
      "p-diag": { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" },
      "p-complete": {
        mappingPolicy: "memberBoundaryFallback",
        confidence: "medium",
        dimension: "artifactParity",
      },
      "p-latency": { mappingPolicy: "none", confidence: "high", dimension: "artifactParity" },
    },
  },
  "vue-slots": {
    fixture: "slots-fixture",
    probes: {
      "p-hover": { mappingPolicy: "strict", confidence: "high", dimension: "vueSemanticValidity" },
    },
  },
};

function key(
  scenario: string,
  probe: string,
  overrides: Partial<CollectorEventKey> = {},
): CollectorEventKey {
  return {
    scenario,
    editStepIndex: 0,
    driver: "rawLsp",
    provider: "tsgo-rust",
    probe,
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
    collector: "diagnostics",
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
    id: "pb",
    method: "diagnostics",
    anchor: "decl",
    mappingPolicy: "strict",
    confidence: "high",
    dimension: "artifactParity",
    requiresSourceMap: true,
    requiredDrivers: ["rawLsp", "tsgo"],
    capabilityRequirements: [],
    ...overrides,
  };
}

const EVENTS = [
  // S0 crash — never benignified even though the allowlist below covers its scenario.
  {
    event: event("server_error", false, "critical", key("member-access", "p-diag"), "server died", {
      line: "ERROR resolver panicked",
    }),
  },
  // S1 false-red, with XML-hostile text and an empty baseline side.
  {
    event: event(
      "diagnostics_parity",
      false,
      "userVisible",
      key("member-access", "p-diag"),
      'verter emitted <Diagnostic> & "spurious"',
      { class: "verterOnly", verterValue: '<Diagnostic> & "spurious"' },
    ),
  },
  // S2 candidate collapse — empty baseline → an em-dash in markdown.
  {
    event: event(
      "no_suggestions_collapse",
      false,
      "candidate",
      key("member-access", "p-complete"),
      "verter offered nothing",
    ),
  },
  // S3 latency — allowlisted below → reclassified to S4 (recorded, never dropped).
  {
    event: event(
      "latency_breach",
      false,
      "userVisible",
      key("member-access", "p-latency"),
      "completion latency breached p95",
    ),
  },
  // S2 vue-semantic hover mismatch (the other dimension).
  {
    event: event(
      "hover_vue_semantic_validity",
      false,
      "userVisible",
      key("vue-slots", "p-hover"),
      "verter hover type label diverged from the oracle",
      { class: "typeLabelMismatch", verterValue: "string", baselineValue: "number" },
    ),
  },
];

const OUTCOMES: SituatedOutcome[] = [
  // a baseline-vs-baseline disagreement (provider disagreement, S4) on both providers.
  {
    scenario: "member-access",
    driver: "rawLsp",
    outcome: baselineDisagreement(
      probe({ id: "pb-disagree" }),
      ["tsgo", "tsserver"],
      [{ class: "severityMismatch", detail: "tsgo and tsserver disagree on severity" }],
    ),
  },
  // an agreement — not a finding, but a baseline execution the manifest records.
  {
    scenario: "member-access",
    driver: "rawLsp",
    outcome: foldOutcome(probe({ id: "pb-agree", method: "hover" }), "tsgo", []),
  },
  // a skipped probe (informational, emits a junit <skipped>).
  {
    scenario: "vue-slots",
    driver: "rawLsp",
    outcome: skipped(probe({ id: "pb-skip", dimension: "vueSemanticValidity" }), {
      reason: "no tsserver baseline available",
    }),
  },
];

const ALLOWLIST: BenignDivergenceAllowlist = {
  version: 1,
  entries: [
    // benignifies the S3 latency noise → S4; the S0 crash is never benignified.
    {
      id: "known-latency-noise",
      reason: "latency on the cold first sample is known-benign",
      match: { scenario: "member-access", signal: "latency_breach" },
    },
  ],
};

function fixedRun() {
  return reduceFindings({
    scenarios: SCENARIOS,
    events: EVENTS,
    outcomes: OUTCOMES,
    allowlist: ALLOWLIST,
  });
}

function fixedSummary() {
  const { findings, baselineRan, allowlistHits } = fixedRun();
  return buildSummary({ findings, baselineRan, allowlistHits, allowlistVersion: 1 });
}

// ── the pinned goldens (the exact serialized bytes of the fixed input) ────────────

const EXPECTED_SUMMARY = `{
  "totals": {
    "findings": 7,
    "failures": 4,
    "allowlisted": 1,
    "providerDisagreements": 1,
    "informational": 1
  },
  "bySeverity": {
    "S0": 1,
    "S1": 1,
    "S2": 2,
    "S3": 0,
    "S4": 3
  },
  "byDimension": {
    "artifactParity": 5,
    "vueSemanticValidity": 2
  },
  "bySignal": {
    "baseline_provider_disagreement": 1,
    "diagnostics_parity": 1,
    "hover_vue_semantic_validity": 1,
    "latency_breach": 1,
    "no_suggestions_collapse": 1,
    "probe_skipped": 1,
    "server_error": 1
  },
  "baselineRan": {
    "probes": 4,
    "probeIds": [
      "p-diag",
      "p-hover",
      "pb-agree",
      "pb-disagree"
    ]
  },
  "allowlist": {
    "version": 1,
    "hits": 1
  },
  "bugReportReconciliation": {
    "status": "skipped_missing_file"
  }
}
`;

const EXPECTED_FINDINGS_MD = `# Verter DX Findings

Total findings: 7
Severity: S0=1, S1=1, S2=2, S3=0, S4=3

## S0 — member-access / server_error

- fingerprint: \`6acfb1de221f9187d0b650f2b5a1371ae69745d74f540fc4baaa9d2e39c40da4\`
- fixture: minimal-member-access
- driver / provider: rawLsp / tsgo-rust
- dimension: artifactParity
- mapping policy / confidence: strict / high
- finding kind: verterDefect
- divergence: —
- verter: \`server died\`
- baseline: —
- root cause hint: —
- events: first=member-access/p-diag/server_error#0 last=member-access/p-diag/server_error#0 count=1
- baseline ran probe: —

## S1 — member-access / diagnostics_parity

- fingerprint: \`d10368bcc42980d2aa8c344ca2ead92ef01fa874443e87f713be4ccc93295ae5\`
- fixture: minimal-member-access
- driver / provider: rawLsp / tsgo-rust
- dimension: artifactParity
- mapping policy / confidence: strict / high
- finding kind: verterDefect
- divergence: verterOnly
- verter: \`<Diagnostic> & "spurious"\`
- baseline: —
- root cause hint: —
- events: first=member-access/p-diag/diagnostics_parity#1 last=member-access/p-diag/diagnostics_parity#1 count=1
- baseline ran probe: p-diag

## S2 — vue-slots / hover_vue_semantic_validity

- fingerprint: \`54dca6b41a89934e4de430c92a20a9d772051fa27fe730f40fa706c2cf8318f5\`
- fixture: slots-fixture
- driver / provider: rawLsp / tsgo-rust
- dimension: vueSemanticValidity
- mapping policy / confidence: strict / high
- finding kind: verterDefect
- divergence: typeLabelMismatch
- verter: \`string\`
- baseline: \`number\`
- root cause hint: —
- events: first=vue-slots/p-hover/hover_vue_semantic_validity#4 last=vue-slots/p-hover/hover_vue_semantic_validity#4 count=1
- baseline ran probe: p-hover

## S2 — member-access / no_suggestions_collapse

- fingerprint: \`9b7486a4087dd4f5b56c404357010ac409a4ed3edd5ec77a4b399785630090c0\`
- fixture: minimal-member-access
- driver / provider: rawLsp / tsgo-rust
- dimension: artifactParity
- mapping policy / confidence: memberBoundaryFallback / medium
- finding kind: verterDefect
- divergence: —
- verter: \`verter offered nothing\`
- baseline: —
- root cause hint: —
- events: first=member-access/p-complete/no_suggestions_collapse#2 last=member-access/p-complete/no_suggestions_collapse#2 count=1
- baseline ran probe: —

## S4 — member-access / latency_breach

- fingerprint: \`3aa60e1f61adc34c2d236db359f5c39325f0a11cbdf11e819f7ffbb4f045cd86\`
- fixture: minimal-member-access
- driver / provider: rawLsp / tsgo-rust
- dimension: artifactParity
- mapping policy / confidence: none / high
- finding kind: verterDefect
- divergence: —
- verter: \`completion latency breached p95\`
- baseline: —
- root cause hint: —
- events: first=member-access/p-latency/latency_breach#3 last=member-access/p-latency/latency_breach#3 count=1
- baseline ran probe: —
- allowlisted: known-latency-noise — latency on the cold first sample is known-benign

## S4 — vue-slots / probe_skipped

- fingerprint: \`7737e8215117555156852b6be23a421d2f2f2c78cbeeadfe51a921e5e9850098\`
- fixture: slots-fixture
- driver / provider: rawLsp / none
- dimension: vueSemanticValidity
- mapping policy / confidence: strict / high
- finding kind: informational
- divergence: —
- verter: \`no tsserver baseline available\`
- baseline: —
- root cause hint: —
- events: first=vue-slots/pb-skip/probe_skipped#7 last=vue-slots/pb-skip/probe_skipped#7 count=1
- baseline ran probe: —
- skip reason: no tsserver baseline available

## S4 — member-access / baseline_provider_disagreement

- fingerprint: \`b34036fe8df376370390193e48e4bcfc5849cc0af9b5c917deec4651d2bf06a2\`
- fixture: minimal-member-access
- driver / provider: rawLsp / tsgo+tsserver
- dimension: artifactParity
- mapping policy / confidence: strict / high
- finding kind: providerDisagreement
- divergence: severityMismatch
- verter: \`severityMismatch: tsgo and tsserver disagree on severity\`
- baseline: —
- root cause hint: —
- events: first=member-access/pb-disagree/baseline_provider_disagreement#5 last=member-access/pb-disagree/baseline_provider_disagreement#5 count=1
- baseline ran probe: pb-disagree

`;

const EXPECTED_JUNIT = `<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="verter-dx-harness" tests="7" failures="4" skipped="1">
<testsuite name="verter-dx-harness" tests="7" failures="4" skipped="1">
  <testcase name="member-access / server_error / 6acfb1de221f" classname="minimal-member-access.member-access">
    <failure message="S0 server_error: server died" type="S0">fingerprint: 6acfb1de221f9187d0b650f2b5a1371ae69745d74f540fc4baaa9d2e39c40da4
divergence: —
verter: server died
baseline: </failure>
  </testcase>
  <testcase name="member-access / diagnostics_parity / d10368bcc429" classname="minimal-member-access.member-access">
    <failure message="S1 diagnostics_parity: &lt;Diagnostic&gt; &amp; &quot;spurious&quot;" type="S1">fingerprint: d10368bcc42980d2aa8c344ca2ead92ef01fa874443e87f713be4ccc93295ae5
divergence: verterOnly
verter: &lt;Diagnostic&gt; &amp; &quot;spurious&quot;
baseline: </failure>
  </testcase>
  <testcase name="vue-slots / hover_vue_semantic_validity / 54dca6b41a89" classname="slots-fixture.vue-slots">
    <failure message="S2 hover_vue_semantic_validity: string" type="S2">fingerprint: 54dca6b41a89934e4de430c92a20a9d772051fa27fe730f40fa706c2cf8318f5
divergence: typeLabelMismatch
verter: string
baseline: number</failure>
  </testcase>
  <testcase name="member-access / no_suggestions_collapse / 9b7486a4087d" classname="minimal-member-access.member-access">
    <failure message="S2 no_suggestions_collapse: verter offered nothing" type="S2">fingerprint: 9b7486a4087dd4f5b56c404357010ac409a4ed3edd5ec77a4b399785630090c0
divergence: —
verter: verter offered nothing
baseline: </failure>
  </testcase>
  <testcase name="member-access / latency_breach / 3aa60e1f61ad" classname="minimal-member-access.member-access"/>
  <testcase name="vue-slots / probe_skipped / 7737e8215117" classname="slots-fixture.vue-slots">
    <skipped message="no tsserver baseline available"/>
  </testcase>
  <testcase name="member-access / baseline_provider_disagreement / b34036fe8df3" classname="minimal-member-access.member-access"/>
</testsuite>
</testsuites>
`;

const EXPECTED_MANIFEST = `{
  "providers": {
    "tsgo": {
      "ranProbeIds": [
        "pb-agree",
        "pb-disagree"
      ],
      "probeCount": 2
    },
    "tsserver": {
      "ranProbeIds": [
        "pb-disagree"
      ],
      "probeCount": 1
    }
  },
  "totalExecutions": 3,
  "distinctProbeIds": [
    "pb-agree",
    "pb-disagree"
  ]
}
`;

describe("report artifacts — byte-stable goldens over a fixed input", () => {
  it("dx-summary.json serializes byte-for-byte to the pinned golden", () => {
    expect(serializeSummary(fixedSummary())).toBe(EXPECTED_SUMMARY);
  });

  it("DX-FINDINGS.md renders byte-for-byte to the pinned golden", () => {
    expect(renderFindingsMarkdown(fixedRun().findings)).toBe(EXPECTED_FINDINGS_MD);
  });

  it("junit.xml renders byte-for-byte to the pinned golden", () => {
    expect(renderJunitXml(fixedRun().findings)).toBe(EXPECTED_JUNIT);
  });

  it("baseline-manifest.json serializes byte-for-byte to the pinned golden", () => {
    expect(serializeBaselineManifest(buildBaselineManifest(OUTCOMES))).toBe(EXPECTED_MANIFEST);
  });
});
