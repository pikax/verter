import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const schema = JSON.parse(
  readFileSync(new URL("../../docs/arch/cli/ownership-schema.v1.json", import.meta.url)),
);

const RELEASE_WORKFLOWS = ["release.yml", "release-ide.yml"];
export const releaseWorkflowText = () =>
  RELEASE_WORKFLOWS.map((name) =>
    readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), "utf8"),
  ).join("\n");

// The schema pins rows it already knows; only the workflows reveal a newly shipped binary.
function releaseBuiltEntries(workflowText) {
  const entries = new Set();
  for (const [line] of workflowText.matchAll(/^.*cargo (?:zig)?build\b.*$/gm)) {
    for (const [, pkg] of line.matchAll(/(?:--package|-p) (\S+)/g)) {
      entries.add(`crates/${pkg}/src/main.rs`);
    }
  }
  assert.ok(entries.size > 0, "no native build found in the release workflows");
  return [...entries];
}

const byId = (rows) => [...rows].sort((a, b) => a.id.localeCompare(b.id));
export function validate(contract, workflowText = releaseWorkflowText()) {
  assert.equal(contract.schemaVersion, 1);
  assert.equal(contract.package, "@verter/cli");
  assert.equal(contract.binary, "verter");
  for (const { id, owner, converter } of contract.inventory) {
    if (!converter) continue;
    const retiring = contract.owners.find((o) => o.id === owner);
    assert.ok(
      owner !== converter && retiring?.path.includes(converter),
      `${id}: a wrapper cannot be retired by the node that creates it`,
    );
  }
  assert.deepEqual(byId(contract.owners), byId(schema.owners), "unknown or pathless owner");
  assert.deepEqual(
    byId(
      contract.inventory.map(({ id, owner, kind, converter }) => ({
        id,
        owner,
        kind,
        ...(converter && { converter }),
      })),
    ),
    byId(schema.inventory),
    "incomplete or conflicting inventory",
  );
  for (const row of contract.inventory) {
    assert.ok(typeof row.source === "string" && row.source.length > 0);
    const owner = contract.owners.find((o) => o.id === row.owner);
    assert.ok(
      owner.targets.some((t) => row.successor === t || row.successor.startsWith(`${t}/`)),
      `pathless assignment: ${row.id}`,
    );
  }
  for (const entry of releaseBuiltEntries(workflowText)) {
    assert.ok(
      contract.inventory.some((row) => row.source === entry),
      `release-built native entry has no inventory row: ${entry}`,
    );
  }
  assert.deepEqual(
    contract.commands.map(({ name, owner, effect, compilerDisposition }) => ({
      name,
      owner,
      effect,
      compilerDisposition,
    })),
    schema.commands,
    "command semantics/order",
  );
  for (const command of contract.commands) {
    assert.equal(command.owner, contract.inventory.find((r) => r.id === command.name).owner);
    assert.equal(command.availability, "planned");
    assert.ok(typeof command.semantics === "string" && command.semantics.length > 0);
  }
}
