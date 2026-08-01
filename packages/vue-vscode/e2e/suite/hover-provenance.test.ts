import { expect } from "chai";
import * as vscode from "vscode";
import { pollBudget, sequenceParent } from "../lib/timeouts";
import {
  FIXTURE_NAME,
  findPosition,
  getAppVuePath,
  hoverText,
  logMark,
  measureHover,
  openReadyCached,
  readTestLog,
  sleep,
} from "../helpers";

/**
 * The enrichment itself is expensive (the LSP runs an
 * `AuditedRequest` through the full semantic pipeline on a
 * background task), so the first hover returns the legacy payload.
 * The provenance-enriched payload is cached and returned on the
 * NEXT hover at the same `(canonical_id, position)`.
 *
 * This test:
 *
 * 1. Opens the fixture `App.vue` and waits for the LSP to settle.
 * 2. Flips `verter.hover.provenance` → `true`.
 * 3. Requires the configuration-driven restart to initialize the replacement
 *    server with provenance enabled.
 * 4. Requests a hover on a binding twice; the second hover should
 *    either carry the enriched markdown section OR (on slower
 *    machines / cold caches) degrade gracefully to the legacy
 *    payload without throwing.
 * 5. Asserts the hover response is well-formed regardless of which
 *    branch the LSP chose.
 *
 * The restart assertion enforces the client setting plumbing. Full
 * provenance-output validation lives in the Rust unit tests under
 * `crates/verter_lsp/src/features/hover_provenance.rs`.
 */
suite(`Hover Provenance [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;
  let provenanceWasEnabled = false;

  async function waitForRestartLog(mark: number, expected: string): Promise<void> {
    const deadline = Date.now() + pollBudget("hoverProvenanceRestart");
    while (Date.now() < deadline) {
      const restartedLog = readTestLog().slice(mark);
      if (restartedLog.includes(expected) && restartedLog.includes("Verter ready")) {
        return;
      }
      await sleep(200);
    }
    const restartedLog = readTestLog().slice(mark);
    expect(restartedLog, `replacement server should log "${expected}"`).to.include(expected);
    expect(restartedLog, "replacement server should publish readiness").to.include("Verter ready");
  }

  suiteSetup(async function () {
    this.timeout(sequenceParent("hoverProvenanceSetup"));
    doc = await openReadyCached(getAppVuePath());
    const config = vscode.workspace.getConfiguration("verter.hover");
    if (config.get<boolean>("provenance", false)) {
      const mark = logMark();
      await config.update("provenance", undefined, vscode.ConfigurationTarget.Workspace);
      await waitForRestartLog(mark, "hover provenance: disabled (default)");
    }
  });

  suiteTeardown(async function () {
    if (!provenanceWasEnabled) return;
    const config = vscode.workspace.getConfiguration("verter.hover");
    const mark = logMark();
    await config.update("provenance", undefined, vscode.ConfigurationTarget.Workspace);
    await waitForRestartLog(mark, "hover provenance: disabled (default)");
    provenanceWasEnabled = false;
  });

  test("restart adopts the changed provenance setting and hover remains typed", async function () {
    this.timeout(sequenceParent("hoverProvenanceRestartRoundTrip"));

    // Locate a simple template binding position.
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    // Enable the provenance opt-in via the workspace configuration.
    const config = vscode.workspace.getConfiguration("verter.hover");
    const mark = logMark();
    await config.update("provenance", true, vscode.ConfigurationTarget.Workspace);
    provenanceWasEnabled = true;
    await waitForRestartLog(mark, "hover provenance: enabled");

    // The first request returns the legacy payload while enrichment is computed
    // in the background.
    const first = await measureHover(doc.uri, pos);
    expect(first.hovers.length, "first hover should produce a payload").to.be.greaterThan(0);

    // Allow the background enrichment to land in the LRU cache.
    await sleep(1000);

    // Second hover — returns the cached enriched payload OR (if the
    // background task is still in flight) the legacy payload again.
    // Either is valid; the test guards against crashes and
    // malformed responses.
    const second = await measureHover(doc.uri, pos);
    expect(second.hovers.length, "second hover should produce a payload").to.be.greaterThan(0);

    const content = hoverText(second.hovers[0]);
    // Regardless of branch, the payload must not degrade to `any`
    // or the fallback component shell.
    expect(content, "hover content must not degrade to any").to.not.match(/:\s*any\b/);
    expect(content, "hover content must not degrade to fallback shell").to.not.include(
      "DefineComponent<{}, {}>",
    );

    // Optional marker check: if the enriched branch took over, the
    // rendered markdown includes a "Provenance" section heading or
    // a `[shared-load]` marker. We assert only the *presence of
    // typed content*; the exact format is covered by Rust unit
    // tests in hover_provenance.rs.
    expect(content, "hover must mention the bound identifier").to.include("count");
  });
});
