import { expect } from "chai";
import * as vscode from "vscode";
import {
  FIXTURE_NAME,
  findPosition,
  getAppVuePath,
  hoverText,
  measureHover,
  openReadyCached,
  sleep,
} from "../helpers";

/**
 * Hover-provenance E2E smoke test. Plan §3 Commit 9 (F7 squash):
 * "E2E under `test:e2e`, NOT part of CLAUDE gate; manual verification".
 *
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
 * 3. Requests a hover on a binding twice; the second hover should
 *    either carry the enriched markdown section OR (on slower
 *    machines / cold caches) degrade gracefully to the legacy
 *    payload without throwing.
 * 4. Asserts the hover response is well-formed regardless of which
 *    branch the LSP chose.
 *
 * This is a smoke test — NOT a gate. It confirms the setting
 * plumbing, the hover round-trip, and the absence of crashes. Full
 * provenance-output validation lives in the Rust unit tests under
 * `crates/verter_lsp/src/features/hover_provenance.rs`.
 */
suite(`Hover Provenance [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    this.timeout(60_000);
    doc = await openReadyCached(getAppVuePath());
  });

  suiteTeardown(async function () {
    const config = vscode.workspace.getConfiguration("verter.hover");
    await config.update("provenance", undefined, vscode.ConfigurationTarget.Workspace);
  });

  test("provenance setting toggles enriched hover without crashing", async function () {
    this.timeout(60_000);

    // Locate a simple template binding position.
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    // Enable the provenance opt-in via the workspace configuration.
    const config = vscode.workspace.getConfiguration("verter.hover");
    await config.update("provenance", true, vscode.ConfigurationTarget.Workspace);
    // Give the LSP a moment to observe the configuration change.
    await sleep(200);

    // First hover — legacy payload expected (per plan §3 Commit 9
    // "legacy payload immediately; background task to compute the
    // enriched payload").
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
