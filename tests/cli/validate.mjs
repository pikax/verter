import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const schema = JSON.parse(
  readFileSync(new URL("../../docs/arch/cli/ownership-schema.v1.json", import.meta.url)),
);

const byId = (rows) => [...rows].sort((a, b) => a.id.localeCompare(b.id));
export function validate(contract) {
  assert.equal(contract.schemaVersion, 1);
  assert.equal(contract.package, "@verter/cli");
  assert.equal(contract.binary, "verter");
  assert.deepEqual(byId(contract.owners), byId(schema.owners), "unknown or pathless owner");
  assert.deepEqual(
    byId(contract.inventory.map(({ id, owner, kind }) => ({ id, owner, kind }))),
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
