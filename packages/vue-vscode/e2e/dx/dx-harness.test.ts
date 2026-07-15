/**
 * Extension-host DX gate (real VS Code).
 *
 * The fidelity gate of the DX harness: it drives the SHIPPED extension + real
 * `verter-lsp` through the matching-generation startup gate, per-character typing,
 * the real completion accept path, and the log canary. It is env-gated — it runs only
 * under `VERTER_E2E_DX=1` (set by `dxLauncher.ts`), never in the fixture matrix or
 * default `pnpm test`.
 *
 * The run has TWO modes, set by the launcher:
 *  - MAIN (`VERTER_E2E_DX=1`): a WORKING provider drives startup/typing/accept. The
 *    full scenario handoff (`DX_HARNESS_*`) is REQUIRED — `suiteSetup` fails hard if
 *    any piece is missing, so a requested run cannot pass with its gates skipped.
 *  - CANARY (`VERTER_E2E_DX_CANARY=1`, the isolated `--canary` launch): the forced MCP
 *    config (`verter.mcp.enabled=true` + `verter.typeProvider=off`) makes the server emit
 *    its deterministic, provider-independent MCP-deprecation WARN; only the canary runs
 *    here, because it needs a different launch config from the main gates.
 *
 * All decision logic lives in the unit-tested pure cores; this file is the live VS
 * Code wiring + assertions and is exercised by the env-gated CI job.
 */
import * as assert from "assert";

import { expect } from "chai";
import * as vscode from "vscode";

import { getCompletionLabel, getCompletions, openVueFile, readTestLog, sleep } from "../helpers";
import { type DxScenario, validateCanaryPreconditions, validateDxScenario } from "./dxScenario";
import { typeChars } from "./dxTyping";
import {
  acceptCompletionInEditor,
  runLogCanary,
  scriptBlockText,
  waitForDxReady,
} from "./dxScenarioRunner";

const DX_ENABLED = process.env.VERTER_E2E_DX === "1";
const CANARY_MODE = process.env.VERTER_E2E_DX_CANARY === "1";
const MAIN_MODE = DX_ENABLED && !CANARY_MODE;

/** The validated scenario, populated by `suiteSetup` in MAIN mode (else unused). */
let scenario: DxScenario;

suite("DX extension-host gate", function () {
  suiteSetup(function () {
    // The ONE sanctioned skip: the DX gate was not requested at all.
    if (!DX_ENABLED) {
      this.skip();
      return;
    }
    // A requested run REQUIRES its preconditions. These throw (fail hard) rather than
    // skip, so a misconfigured run cannot report a vacuous green.
    if (CANARY_MODE) {
      validateCanaryPreconditions(process.env);
    } else {
      scenario = validateDxScenario(process.env);
    }
  });

  test("startup gate reaches a matching ready/sync generation then quiesces", async function () {
    if (!MAIN_MODE) {
      this.skip();
      return;
    }
    this.timeout(120_000);
    const doc = await openVueFile(scenario.entry);
    const { matchedGeneration } = await waitForDxReady(doc.uri);
    expect(matchedGeneration, "matched init generation").to.be.a("number");
    expect(matchedGeneration).to.be.greaterThanOrEqual(0);
  });

  test("per-character typing drives incremental completions", async function () {
    if (!MAIN_MODE) {
      this.skip();
      return;
    }
    this.timeout(120_000);
    const doc = await openVueFile(scenario.entry);
    await waitForDxReady(doc.uri);

    const editor = vscode.window.activeTextEditor;
    assert.ok(editor, "an active editor is required to type");
    assert.strictEqual(editor!.document.uri.toString(), doc.uri.toString());

    const anchorStart = findAnchor(doc, scenario.anchorText);
    assert.ok(anchorStart, `anchor text not found in ${scenario.entry}: ${scenario.anchorText}`);
    // The scenario contract types at the END of anchorText (see dxScenario.ts), mirroring
    // the accept path's translation; typing at the anchor START would drive completions at
    // the wrong source location.
    const typeStart = anchorStart!.translate(0, scenario.anchorText.length);
    assert.strictEqual(
      typeStart.character,
      anchorStart!.character + scenario.anchorText.length,
      "typing must start at the END of anchorText",
    );

    const perCharLabels: string[][] = [];
    await typeChars(editor!, typeStart, scenario.typeText, async ({ position }) => {
      const list = await getCompletions(doc.uri, position);
      perCharLabels.push((list?.items ?? []).map(getCompletionLabel));
    });

    // Discriminating: the characters were inserted AT the anchor end, so the document now
    // reads `<anchor><typed>` contiguously. Typing at the anchor start would instead yield
    // `<typed><anchor>`, failing this — the faithful guard for the typing location.
    expect(
      editor!.document.getText(),
      "typed text must land immediately AFTER the anchor",
    ).to.contain(scenario.anchorText + scenario.typeText);

    // Raw observation: one sample per typed character.
    expect(perCharLabels).to.have.lengthOf([...scenario.typeText].length);
    const last = perCharLabels[perCharLabels.length - 1] ?? [];
    expect(
      last,
      `completions after typing "${scenario.typeText}": ${JSON.stringify(last)}`,
    ).to.include(scenario.expectCompletion);
  });

  test("real accept path mutates both the document and the import (expected ranked first)", async function () {
    if (!MAIN_MODE) {
      this.skip();
      return;
    }
    this.timeout(120_000);
    const doc = await openVueFile(scenario.entry);
    await waitForDxReady(doc.uri);

    const editor = vscode.window.activeTextEditor;
    assert.ok(editor, "an active editor is required to accept a completion");

    const at = findAnchor(doc, scenario.acceptAnchor);
    assert.ok(at, `accept anchor not found in ${scenario.entry}: ${scenario.acceptAnchor}`);
    const cursor = at!.translate(0, scenario.acceptAnchor.length);
    editor!.selection = new vscode.Selection(cursor, cursor);

    // Ranked-first guard: the expected completion must be present AND rank first, so a
    // wrong first suggestion cannot satisfy the accept path.
    const list = await getCompletions(doc.uri, cursor);
    const labels = (list?.items ?? []).map(getCompletionLabel);
    expect(labels, `completions at accept anchor: ${JSON.stringify(labels)}`).to.include(
      scenario.acceptExpect,
    );
    expect(rankedFirstLabel(list), "expected completion ranks first").to.equal(
      scenario.acceptExpect,
    );

    const importBefore = scriptBlockText(editor!.document);
    const outcome = await acceptCompletionInEditor(editor!);

    // The real accept must have changed BOTH the document body and the import block.
    expect(outcome.accepted, "completion accept mutated document + import").to.equal(true);
    expect(outcome.docChanged).to.equal(true);
    expect(outcome.importChanged).to.equal(true);
    const importAfter = scriptBlockText(editor!.document);
    expect(importAfter).to.not.equal(importBefore);
    // The RIGHT suggestion landed: its label is in the new import block.
    expect(importAfter, "accepted import references the expected component").to.contain(
      scenario.acceptExpect,
    );
  });

  test("log canary: the forced MCP-deprecation server WARN is captured in VERTER_E2E_LOG_FILE", async function () {
    // Runs ONLY in the isolated canary launch — it forces the MCP config
    // (mcp.enabled=true + typeProvider=off), a different launch from the main gates.
    if (!CANARY_MODE) {
      this.skip();
      return;
    }
    this.timeout(60_000);

    // Activate the extension so it builds the server options (logging the `[buildServerOptions]`
    // proof line carrying `--mcp-port=0` + `--type-provider=off`) and starts the server, which
    // emits its MCP-deprecation WARN. Wait for the proof line, then let a captured WARN settle.
    await openVueFile("App.vue");
    await waitForLogContains("[buildServerOptions]", 30_000);
    await sleep(2_000);

    const verdict = runLogCanary();
    if (verdict.status === "ok") {
      expect(verdict.captured, "forced server WARN captured in the log file").to.equal(true);
      return;
    }
    if (verdict.status === "gated") {
      // The forced MCP WARN was emitted server-side, but the product log hook (log.* only)
      // does not capture server stderr (surfaced via append/appendLine). This is the canary
      // DOING ITS JOB: a VISIBLE, recorded signal to escalate the extension log-hook sign-off
      // — NOT a harness defect, and NOT to be masked by patching this test or extension.ts.
      assert.fail(`DX log canary GATED — product sign-off needed, do not mask: ${verdict.reason}`);
    }
    // `inconclusive`: the forcing itself did not take effect (missing `--mcp-port=0` launch
    // proof / config drift, or a stray WARN without proof). A LOUD failure naming the gap.
    assert.fail(
      `DX log canary INCONCLUSIVE — forcing did not take effect: ${verdict.reason ?? verdict.status}`,
    );
  });
});

/** Locate the start position of `needle` in the document, or `undefined`. */
function findAnchor(doc: vscode.TextDocument, needle: string): vscode.Position | undefined {
  const text = doc.getText();
  const offset = text.indexOf(needle);
  return offset >= 0 ? doc.positionAt(offset) : undefined;
}

/** The label of the completion ranked first (by `sortText`, label as tiebreak). */
function rankedFirstLabel(list: vscode.CompletionList | undefined): string | undefined {
  const items = [...(list?.items ?? [])];
  if (items.length === 0) return undefined;
  items.sort((a, b) =>
    (a.sortText ?? getCompletionLabel(a)).localeCompare(b.sortText ?? getCompletionLabel(b)),
  );
  return getCompletionLabel(items[0]);
}

/** Poll the captured extension log until it contains `needle`, or throw on timeout. */
async function waitForLogContains(needle: string, timeoutMs: number): Promise<void> {
  const start = Date.now();
  for (;;) {
    if (readTestLog().includes(needle)) return;
    if (Date.now() - start >= timeoutMs) {
      throw new Error(
        `timed out after ${timeoutMs}ms waiting for ${JSON.stringify(needle)} in the log`,
      );
    }
    await sleep(200);
  }
}
