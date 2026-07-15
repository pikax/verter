/**
 * Required-emit-payload honesty at the NATIVE binding level.
 *
 * An imported property-style emit and a composite call-signature emit
 * publish their REAL payload tuples: the normalized emit payload source
 * is the authority (an imported `save: [id: number]` publishes its
 * faithful closed tuple), and a call-signature payload richer than the
 * closed element vocabulary replays through the callable-params route
 * (`(e: 'save', value: Row)` publishes `[value: Row]`). Neither class
 * surfaces a typed failure or a fabricated `unknown` any more. An
 * AUTHORED `unknown` payload stays a valid success.
 */

import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";

const fixtureDir = join(__dirname, "fixtures");

afterAll(() => {
  shutdownMetaRuntime();
});

async function getChecker() {
  return createCheckerByJson(fixtureDir, {
    compilerOptions: { strict: true },
    include: ["**/*.vue", "**/*.ts"],
  });
}

describe("required emit payload honesty (native)", () => {
  test("an imported property-style emit payload publishes the real tuple", async () => {
    const checker = await getChecker();
    // The imported `ImportedEmits { save: [id: number] }` payload is the
    // faithful normalized closed tuple — the emit payload authority (see
    // docs/arch/stage10-b6-p4b-debt-rows.md DEBT ROW #1, CLOSED). The
    // native query completes and the payload is the REAL `[id: number]`
    // tuple, never a typed failure and never a fabricated `unknown`.
    const meta = await checker.getComponentMeta(join(fixtureDir, "RequiredEmitImported.vue"));
    const save = meta._verter!.events.find((event) => event.name === "save");
    expect(save, "the imported property-style event publishes").toBeDefined();
    expect(save!.payload).toEqual({
      kind: "tuple",
      elements: [{ kind: "primitive", name: "number" }],
      labels: ["id"],
    });
  });

  test("a composite call-signature emit payload publishes the real tuple", async () => {
    const checker = await getChecker();
    // The cross-file `Events { (e: 'save', value: Row): void }` payload
    // replays through the callable-params route: the published tuple keeps
    // the shallow resolvable `Row` reference — never a typed failure,
    // never a semantic miss, never a fabricated `unknown`.
    const meta = await checker.getComponentMeta(join(fixtureDir, "RequiredEmitComposite.vue"));
    const save = meta._verter!.events.find((event) => event.name === "save");
    expect(save, "the cross-file call-signature event publishes").toBeDefined();
    expect(save!.payload).toEqual({
      kind: "tuple",
      elements: [{ kind: "ref", name: "Row" }],
      labels: ["value"],
    });
    expect(JSON.stringify(save!.payload)).not.toContain("unknown");
    expect(JSON.stringify(save!.payload)).not.toContain("semanticMiss");
  });

  test("an authored unknown emit payload stays a valid success", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(join(fixtureDir, "AuthoredUnknownEmit.vue"));
    const save = meta.events.find((event) => event.name === "save");
    expect(save, "the authored-unknown event publishes").toBeDefined();
    // The authored payload tuple survives: the event signature carries the
    // author's own `unknown` element — a present success, not a failure.
    expect(save?.signature ?? save?.type ?? "").toContain("unknown");
  });
});
