/**
 * The inventory of a node:test suite: the cases it declares, counted without
 * running any of them.
 *
 * node:test has no listing mode, so a clean run of a suite could otherwise be
 * judged only on its own summary — and a suite that silently stops reaching
 * some of its registrations reports a smaller, perfectly green block. Cargo's
 * runners are held to their own listing of the selection; this gives a
 * node:test selection the same footing. Preloaded with `node --import` in
 * front of the suite files, it resolves `node:test` to a registrar that
 * counts every case the suite's module code declares and runs none of their
 * bodies, then prints one line when the process exits.
 *
 * What it counts is what the runner's own summary counts: every declared case
 * — including those reached through the suite's own registration wrappers —
 * with the ones declared skipped or todo counted again as skipped. A case a
 * test body registers at run time (a subtest) is not declared by module code,
 * so a suite using them has no inventory this can produce and its clean run
 * is refused rather than trusted.
 */

import fs from "node:fs";
import { registerHooks } from "node:module";
import process from "node:process";

/** The one line this preload prints; `parseInventory` reads it back. */
export const INVENTORY_PREFIX = "node-test inventory:";

const REGISTRAR = Symbol.for("closure.node-test-inventory");
const tally = { declared: 0, skipped: 0 };
globalThis[REGISTRAR] = tally;

// The replacement `node:test`. Declaration shapes mirror the real API:
// `test(name?, options?, fn?)`, the `.skip`/`.todo`/`.only` variants, and
// `describe` bodies, which declare their children when they run.
const REGISTRAR_SOURCE = `
const tally = globalThis[Symbol.for("closure.node-test-inventory")];
let skipping = 0;
const optionsOf = (args) => args.find((arg) => arg !== null && typeof arg === "object") ?? {};
const declare = (forced) => (...args) => {
  const options = optionsOf(args);
  tally.declared += 1;
  if (forced || skipping > 0 || options.skip || options.todo) tally.skipped += 1;
};
const group = (forced) => (...args) => {
  const options = optionsOf(args);
  const body = args.find((arg) => typeof arg === "function");
  const skip = forced || Boolean(options.skip || options.todo);
  if (skip) skipping += 1;
  try {
    body?.();
  } finally {
    if (skip) skipping -= 1;
  }
};
const test = declare(false);
test.skip = declare(true);
test.todo = declare(true);
test.only = declare(false);
const describe = group(false);
describe.skip = group(true);
describe.todo = group(true);
describe.only = group(false);
const hook = () => {};
const mock = new Proxy({}, { get: () => () => {} });
export default test;
export { test, test as it, describe, describe as suite, hook as before, hook as after, hook as beforeEach, hook as afterEach, mock };
`;
const REGISTRAR_URL = `data:text/javascript,${encodeURIComponent(REGISTRAR_SOURCE)}`;

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "node:test" || specifier === "test")
      return { url: REGISTRAR_URL, format: "module", shortCircuit: true };
    return nextResolve(specifier, context);
  },
});

// Written synchronously on exit: an asynchronous stdout write is not
// guaranteed to reach a pipe before the process ends.
process.on("exit", () => {
  fs.writeSync(1, `${INVENTORY_PREFIX} declared ${tally.declared} skipped ${tally.skipped}\n`);
});
