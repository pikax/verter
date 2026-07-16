/**
 * Regenerates ISSUES.md table rows for every ISSUE-* referenced under e2e/
 * (excluding unit/spec test dummies).
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const e2eRoot = path.resolve(__dirname, "..");
const re = /\bISSUE-[\w-]+\b/g;
const found = new Set();

function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "out-test") continue;
      walk(full);
    } else if (
      /\.(ts|md|js)$/.test(entry.name) &&
      entry.name !== "ISSUES.md" &&
      !/\.unit\.test\./.test(entry.name) &&
      !/\.spec\./.test(entry.name)
    ) {
      const text = fs.readFileSync(full, "utf8");
      let m;
      while ((m = re.exec(text))) found.add(m[0]);
    }
  }
}

walk(e2eRoot);
const ids = [...found].sort();

const header = `# E2E parity issues

Open gaps found while plumbing VS Code / LSP parity coverage for Vue and Svelte.
Every \`ISSUE-*\` referenced from E2E sources (except unit tests) MUST appear as a table row.

Status: \`open\` · \`partial\` · \`fixed\`

| ID | Area | Status | Symptom | Expected | Notes |
|---|---|---|---|---|---|
`;

const rows = ids
  .map(
    (id) =>
      `| ${id} | triage | open | Referenced by E2E sources; classify as product vs test defect on first red | Discriminating product or test fix | Hard-fail suite (no catch-all skip) |`,
  )
  .join("\n");

const footer = `

## Hardening rules

1. **No catch-all skip**: \`failParityGap\` throws \`PRODUCT_GAP\` / \`TEST_DEFECT\` — never \`context.skip()\`.
2. **Matrix hard-fails**: every accepted matrix ID is release-required.
3. **Fixture-scoped discovery**: specialty fixtures only load matching suite globs (\`fixtureSuiteMap.ts\`).
4. **Failure detail**: run summary includes \`failedTests[]\` with message + stack.
5. **Ledger completeness**: \`issueLedger.unit.test.ts\` fails if any ISSUE-* is missing from this file.
6. **Svelte clean diagnostics**: do **not** mask TS7026 with permissive ambient JSX in the required clean gate (see ISSUE-svelte-jsx-intrinsics).
7. **Public vs testing surface**: non-test imports must not expose script-setup internals (negative public-type tests).

## Regenerating

\`\`\`bash
node packages/vue-vscode/e2e/scripts/gen-issues-ledger.mjs
\`\`\`
`;

fs.writeFileSync(path.join(e2eRoot, "ISSUES.md"), header + rows + footer);
console.log(`wrote ISSUES.md with ${ids.length} rows`);
