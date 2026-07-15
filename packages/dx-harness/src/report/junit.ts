/**
 * The `junit.xml` emitter: a CI-consumable projection of the deduped finding set.
 *
 * One `<testsuite>` holding one `<testcase>` per finding. A finding the gate treats
 * as a failure (an unallowlisted S0–S2) emits a `<failure>`; a skipped probe emits a
 * `<skipped>`; everything else (S3/S4, allowlisted, informational) is a passing
 * testcase. All attribute and text values are XML-escaped, so a finding's
 * behavior/detail text — which routinely contains `<`, `&`, and quotes — can never
 * break the document.
 */

import { writeFileSync } from "node:fs";

import { isFailingSeverity, type DxFinding } from "./findings.js";

/** The canonical on-disk name of the JUnit report. */
export const JUNIT_FILENAME = "junit.xml";

/** Options for {@link renderJunitXml}. */
export interface JunitOptions {
  /** The `<testsuite>`/`<testsuites>` name attribute. */
  readonly suiteName?: string;
}

/** Escape the five XML predefined entities so a value is safe in both attributes and text. */
function escapeXml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

/** Whether the finding is a gate failure: an unallowlisted S0–S2. */
function isFailure(finding: DxFinding): boolean {
  return finding.allowlisted === undefined && isFailingSeverity(finding.severity);
}

/** The stable, human-meaningful testcase name. */
function testcaseName(finding: DxFinding): string {
  return `${finding.scenario} / ${finding.signal} / ${finding.fingerprint.slice(0, 12)}`;
}

/** The body lines of one `<testcase>`. */
function renderTestcase(finding: DxFinding): string[] {
  const name = escapeXml(testcaseName(finding));
  const classname = escapeXml(`${finding.fixture}.${finding.scenario}`);
  const open = `  <testcase name="${name}" classname="${classname}">`;
  const close = "  </testcase>";
  if (finding.skipReason !== undefined) {
    return [open, `    <skipped message="${escapeXml(finding.skipReason)}"/>`, close];
  }
  if (isFailure(finding)) {
    const message = escapeXml(`${finding.severity} ${finding.signal}: ${finding.verterBehavior}`);
    const body = escapeXml(
      [
        `fingerprint: ${finding.fingerprint}`,
        `divergence: ${finding.divergence ?? "—"}`,
        `verter: ${finding.verterBehavior}`,
        `baseline: ${finding.baselineBehavior}`,
        finding.rootCauseHint !== null ? `root cause: ${finding.rootCauseHint}` : "",
      ]
        .filter((line) => line.length > 0)
        .join("\n"),
    );
    return [
      open,
      `    <failure message="${message}" type="${finding.severity}">${body}</failure>`,
      close,
    ];
  }
  return [`  <testcase name="${name}" classname="${classname}"/>`];
}

/**
 * Render the deduped findings to a JUnit XML document. Findings are emitted in the
 * order given (the reducer already sorts them deterministically by severity then
 * fingerprint), so the output is reproducible.
 */
export function renderJunitXml(findings: readonly DxFinding[], options: JunitOptions = {}): string {
  const suiteName = options.suiteName ?? "verter-dx-harness";
  const failures = findings.filter(isFailure).length;
  const skips = findings.filter((finding) => finding.skipReason !== undefined).length;
  const lines: string[] = ['<?xml version="1.0" encoding="UTF-8"?>'];
  const suiteAttrs = `name="${escapeXml(suiteName)}" tests="${findings.length}" failures="${failures}" skipped="${skips}"`;
  lines.push(`<testsuites ${suiteAttrs}>`);
  lines.push(`<testsuite ${suiteAttrs}>`);
  for (const finding of findings) lines.push(...renderTestcase(finding));
  lines.push("</testsuite>");
  lines.push("</testsuites>");
  return `${lines.join("\n")}\n`;
}

/** Write the deterministic `junit.xml`. */
export function writeJunitXml(
  filePath: string,
  findings: readonly DxFinding[],
  options: JunitOptions = {},
): void {
  writeFileSync(filePath, renderJunitXml(findings, options), "utf8");
}
