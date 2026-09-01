import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Shared schema-2 ledger fixture builder for githubctl tests. Mirrors the
// canonical serializer's shape (tests never write the live ledger).

export function ledgerText({
  implemented = [],
  pending = [],
  locators = {},
  messages = {},
  dates = {},
  issues = [],
  trains = [],
} = {}) {
  const records = new Map();
  for (const id of pending) records.set(id, `{ status = "pending" }`);
  for (const id of implemented) {
    const message = messages[id] ?? `test locator ${id}`;
    const date = dates[id] ?? "2026-08-28T00:00:00+00:00";
    const pr = locators[id];
    const fields = [
      `status = "implemented"`,
      `commit_message = ${JSON.stringify(message)}`,
      `commit_date = ${JSON.stringify(date)}`,
      ...(pr == null ? [] : [`pull_request = ${pr}`]),
    ];
    records.set(id, `{ ${fields.join(", ")} }`);
  }
  const lines = ["schema = 2", "", "[implementation]"];
  for (const id of [...records.keys()].sort()) lines.push(`"${id}" = ${records.get(id)}`);
  for (const row of issues) {
    lines.push(
      "",
      "[[github_issue]]",
      `node_id = ${JSON.stringify(row.node_id)}`,
      `gh_issue = ${row.gh_issue}`,
      `sync_to_github = ${row.sync_to_github === true}`,
    );
  }
  for (const row of trains) {
    lines.push(
      "",
      "[[github_train_issue]]",
      `train = ${JSON.stringify(row.train)}`,
      `gh_issue = ${row.gh_issue}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

export function writeLedgerFixture(prefix, options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  const file = path.join(dir, "implemented.toml");
  fs.writeFileSync(file, ledgerText(options));
  return file;
}
