/**
 * Issue-ledger completeness: every ISSUE-* referenced from e2e sources must
 * appear as a table row in ISSUES.md.
 */
import * as fs from "node:fs";
import * as path from "node:path";

const ISSUE_RE = /\bISSUE-[\w-]+\b/g;

export function collectIssueIdsFromText(text: string): string[] {
  const found = new Set<string>();
  for (const m of text.matchAll(ISSUE_RE)) {
    found.add(m[0]);
  }
  return [...found].sort();
}

export function collectIssueIdsFromDir(root: string): string[] {
  const found = new Set<string>();
  const walk = (dir: string) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "node_modules" || entry.name === "out-test") continue;
        walk(full);
      } else if (
        /\.(ts|md|js)$/.test(entry.name) &&
        entry.name !== "ISSUES.md" &&
        // Unit tests may use dummy ISSUE-* tokens for parsing checks only.
        !/\.unit\.test\.(ts|js)$/.test(entry.name) &&
        !/\.spec\.(ts|js)$/.test(entry.name)
      ) {
        for (const id of collectIssueIdsFromText(fs.readFileSync(full, "utf8"))) {
          found.add(id);
        }
      }
    }
  };
  walk(root);
  return [...found].sort();
}

/** ISSUE ids that have a markdown table row `| ISSUE-… |` in ISSUES.md. */
export function collectIssueIdsFromLedger(issuesMd: string): string[] {
  const found = new Set<string>();
  for (const line of issuesMd.split(/\r?\n/)) {
    const m = line.match(/^\|\s*(ISSUE-[\w-]+)\s*\|/);
    if (m) found.add(m[1]);
  }
  return [...found].sort();
}

export interface LedgerGap {
  readonly missingFromLedger: string[];
  readonly orphanInLedger: string[];
}

export function diffIssueLedger(referenced: string[], inLedger: string[]): LedgerGap {
  const ref = new Set(referenced);
  const led = new Set(inLedger);
  return {
    missingFromLedger: referenced.filter((id) => !led.has(id)),
    orphanInLedger: inLedger.filter((id) => !ref.has(id)),
  };
}

/**
 * Build markdown table rows for missing issue IDs with an open triage status.
 */
export function renderMissingIssueRows(ids: string[]): string {
  if (ids.length === 0) return "";
  const lines = ids.map(
    (id) =>
      `| ${id} | triage | open | Referenced by E2E sources; row auto-generated during hardening | Discriminating product or test fix | See source references |`,
  );
  return lines.join("\n");
}
