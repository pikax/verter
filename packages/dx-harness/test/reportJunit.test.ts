import { describe, expect, it } from "vitest";

import {
  baselineDisagreement,
  collectorEvent,
  skipped,
  type CollectorEventKey,
  type Probe,
} from "../src/index.js";
import {
  JUNIT_FILENAME,
  reduceFindings,
  renderJunitXml,
  type ScenarioIndex,
  type SituatedOutcome,
} from "../src/report/index.js";

const SCENARIOS: ScenarioIndex = {
  "member-access": {
    fixture: "minimal-member-access",
    probes: {
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
    probe: "p-diag",
    version: 1,
    anchor: "decl",
    ...overrides,
  };
}

function probe(overrides: Partial<Probe> = {}): Probe {
  return {
    id: "p-diag",
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

function buildFindings() {
  const falseRed = collectorEvent({
    collector: "diagnostics",
    signal: "diagnostics_parity",
    ok: false,
    severity: "userVisible",
    provenance: { detectedBy: "rawLsp" },
    key: key(),
    // XML-hostile behavior text exercises attribute + text escaping.
    detail: 'verter emitted <Diagnostic> & "spurious" error',
    data: { class: "verterOnly", verterValue: '<Diagnostic> & "spurious"' },
  });
  const disagreement: SituatedOutcome = {
    scenario: "member-access",
    driver: "rawLsp",
    outcome: baselineDisagreement(
      probe(),
      ["tsgo", "tsserver"],
      [{ class: "severityMismatch", detail: "providers disagree" }],
    ),
  };
  const skip: SituatedOutcome = {
    scenario: "member-access",
    driver: "rawLsp",
    outcome: skipped(probe(), { reason: "no tsserver baseline <available>" }),
  };
  return reduceFindings({
    scenarios: SCENARIOS,
    events: [{ event: falseRed }],
    outcomes: [disagreement, skip],
  }).findings;
}

describe("report/junit — JUnit XML emission", () => {
  it("emits a well-formed testsuite with one testcase per finding", () => {
    const xml = renderJunitXml(buildFindings());
    expect(xml.startsWith('<?xml version="1.0" encoding="UTF-8"?>')).toBe(true);
    expect(xml).toContain("<testsuites");
    expect(xml).toContain("<testsuite ");
    // three findings: the S1 false-red, the S4 provider disagreement, the skip.
    expect(xml).toMatch(/<testsuite [^>]*tests="3"/);
    expect((xml.match(/<testcase\b/g) ?? []).length).toBe(3);
  });

  it("fails S0–S2 unallowlisted findings, skips skipReason findings, passes the rest", () => {
    const xml = renderJunitXml(buildFindings());
    // the false-red is S1 → exactly one <failure>.
    expect((xml.match(/<failure\b/g) ?? []).length).toBe(1);
    expect(xml).toMatch(/<testsuite [^>]*failures="1"/);
    // the skip → exactly one <skipped>.
    expect((xml.match(/<skipped\b/g) ?? []).length).toBe(1);
    expect(xml).toMatch(/<testsuite [^>]*skipped="1"/);
    // the S4 provider disagreement is neither a failure nor a skip.
  });

  it("escapes XML special characters in attributes and text", () => {
    const xml = renderJunitXml(buildFindings());
    // raw, unescaped hostile sequences must not survive.
    expect(xml).not.toContain('<Diagnostic> & "spurious"');
    expect(xml).toContain("&lt;Diagnostic&gt;");
    expect(xml).toContain("&amp;");
    // the skip message's angle brackets are escaped too.
    expect(xml).toContain("no tsserver baseline &lt;available&gt;");
    // every bare ampersand is part of an entity (no naked & survives).
    expect(/&(?!(amp|lt|gt|quot|apos);)/.test(xml)).toBe(false);
  });

  it("is deterministic and exposes the canonical filename", () => {
    expect(renderJunitXml(buildFindings())).toBe(renderJunitXml(buildFindings()));
    expect(JUNIT_FILENAME).toBe("junit.xml");
  });

  it("emits a valid empty suite for zero findings", () => {
    const xml = renderJunitXml([]);
    expect(xml).toMatch(/<testsuite [^>]*tests="0"[^>]*failures="0"[^>]*skipped="0"/);
    expect((xml.match(/<testcase\b/g) ?? []).length).toBe(0);
  });
});
