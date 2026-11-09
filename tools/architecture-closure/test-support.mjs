// Shared harness of the validator's tests: synthetic fixtures, and negative
// controls that prove their mutation applied before trusting a rejection.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { validateClosure } from "./validate.mjs";

const FIXTURES = new URL("./fixtures/", import.meta.url);

export const fixturePath = (name) => new URL(name, FIXTURES);
export const loadFixture = (name) => JSON.parse(readFileSync(fixturePath(name), "utf8"));

/** Removes the single row matching `predicate`, failing when there is none. */
export function dropRow(list, predicate) {
  const index = list.findIndex(predicate);
  assert.notEqual(index, -1, "the mutation's target row is not in the fixture");
  return list.splice(index, 1)[0];
}

/** The single row matching `predicate`, failing when there is none. */
export function findRow(list, predicate) {
  const row = list.find(predicate);
  assert.ok(row, "the mutation's target row is not in the fixture");
  return row;
}

/**
 * Applies `control.mutate` to fresh fixture copies, proves it applied through
 * `control.applied` and an input change, then requires exactly the control's
 * reason codes. An extra or missing code fails, so a control can never pass
 * through a different rule than the one it names.
 */
export function assertRejected(mode, base, control) {
  const inputs = structuredClone(base);
  const before = JSON.stringify(inputs);
  control.mutate(inputs);
  assert.notEqual(JSON.stringify(inputs), before, `${control.name}: the mutation changed nothing`);
  assert.equal(control.applied(inputs), true, `${control.name}: the mutation did not apply`);
  const decision = validateClosure({ mode, ...inputs });
  assert.equal(decision.ok, false, `${control.name}: the mutated input was accepted`);
  const codes = [...new Set(decision.errors.map((error) => error.code))].sort();
  assert.deepEqual(codes, [...control.expect].sort(), JSON.stringify(decision.errors, null, 2));
  return decision;
}
