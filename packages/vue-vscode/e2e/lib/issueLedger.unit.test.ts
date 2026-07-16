import { describe, expect, it } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import {
  collectIssueIdsFromDir,
  collectIssueIdsFromLedger,
  collectIssueIdsFromText,
  diffIssueLedger,
  renderMissingIssueRows,
} from "./issueLedger";

const E2E_ROOT = path.resolve(__dirname, "..");

describe("issueLedger", () => {
  it("extracts ISSUE ids from text", () => {
    expect(
      collectIssueIdsFromText('failProduct("x", "ISSUE-sample-one", "d"); ISSUE-sample-two'),
    ).toEqual(["ISSUE-sample-one", "ISSUE-sample-two"]);
  });

  it("diffIssueLedger reports missing and orphans", () => {
    const gap = diffIssueLedger(
      ["ISSUE-sample-one", "ISSUE-sample-two"],
      ["ISSUE-sample-one", "ISSUE-sample-three"],
    );
    expect(gap.missingFromLedger).toEqual(["ISSUE-sample-two"]);
    expect(gap.orphanInLedger).toEqual(["ISSUE-sample-three"]);
  });

  it("every ISSUE-* referenced under e2e/ has a row in ISSUES.md", () => {
    const issuesPath = path.join(E2E_ROOT, "ISSUES.md");
    expect(fs.existsSync(issuesPath), "ISSUES.md must exist").toBe(true);
    const referenced = collectIssueIdsFromDir(E2E_ROOT);
    const ledger = collectIssueIdsFromLedger(fs.readFileSync(issuesPath, "utf8"));
    const gap = diffIssueLedger(referenced, ledger);
    if (gap.missingFromLedger.length > 0) {
      // Helpful dump for regenerating the ledger.
      // eslint-disable-next-line no-console
      console.error("Missing ISSUES.md rows:\n" + renderMissingIssueRows(gap.missingFromLedger));
    }
    expect(
      gap.missingFromLedger,
      `missing ledger rows (${gap.missingFromLedger.length}): ${gap.missingFromLedger.join(", ")}`,
    ).toEqual([]);
  });
});
