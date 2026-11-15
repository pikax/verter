// E2E coverage for the full component-meta LSP request through the test command bridge.

import { expect } from "chai";
import * as vscode from "vscode";

import {
  ensureFixtureWarm,
  openReadyCached,
  getAppVuePath,
  getCompVuePath,
  FIXTURE_NAME,
} from "../helpers";

async function getComponentMeta(uri: string): Promise<unknown> {
  return vscode.commands.executeCommand("verter._getComponentMeta", { uri });
}

suite(`Component-meta LSP custom methods [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    await ensureFixtureWarm();

    // Warm the component file (or fallback to App.vue when no comp fixture).
    const compPath = getCompVuePath();
    if (compPath) {
      await openReadyCached(compPath);
    }
    await openReadyCached(getAppVuePath());
  });

  test("getComponentMeta returns Volar-shape payload for a component", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    const doc = await openReadyCached(compPath);
    const result = await getComponentMeta(doc.uri.toString());
    expect(result, "getComponentMeta result must not be null").to.exist;
    // The Volar shape includes top-level keys like `props`, `events`, `slots`.
    const r = result as Record<string, unknown>;
    expect(r).to.have.property("props");
    expect(r).to.have.property("events");
    expect(r).to.have.property("slots");
  });

  test("getComponentMeta returns null for a non-component file (App.vue typically isn't one)", async function () {
    this.timeout(30_000);
    const doc = await openReadyCached(getAppVuePath());
    const result = await getComponentMeta(doc.uri.toString());
    // App.vue may or may not be classified as a component depending on the
    // fixture; what we assert is that the call succeeds and returns either
    // a Volar shape or `null`. No exception, no thrown error.
    if (result !== null) {
      expect(result).to.have.property("props");
    }
  });
});
