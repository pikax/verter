/**
 * Cross-framework LSP extras: signature help, document highlights, type definition,
 * semantic tokens, formatting, extract/refactor product commands.
 *
 * Missing or unmapped capabilities fail with an ISSUES.md ID.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME } from "../../helpers";
import {
  documentHighlightsAt,
  ensureParityReady,
  openRelative,
  signatureHelpAt,
  failParityGap,
  typeDefinitionsAt,
  semanticTokensExist,
} from "../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

suite(`LSP extras [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("lsp.signature-help.script-call", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const anchor =
      fw === "vue"
        ? { file: "src/DailyBinding.vue", token: "renderDaily", occurrence: 0, caretOffset: 11 }
        : { file: "src/DailyBinding.svelte", token: "renderDaily", occurrence: 0, caretOffset: 11 };
    try {
      const help = await signatureHelpAt(anchor);
      if (!help || help.signatures.length === 0) {
        throw new Error("empty signature help");
      }
    } catch (err) {
      failParityGap(
        this,
        "lsp.signature-help.script-call",
        "ISSUE-lsp-signature-help",
        `Signature help unavailable or unmapped for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("lsp.document-highlights.binding", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const anchor =
      fw === "vue"
        ? { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 0 }
        : { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 0 };
    try {
      const highlights = await documentHighlightsAt(anchor);
      if (highlights.length < 2) {
        throw new Error(`expected multi-region highlights, got ${highlights.length}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "lsp.document-highlights.binding",
        "ISSUE-lsp-document-highlights",
        `Document highlights incomplete for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("lsp.type-definition.binding", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const anchor =
      fw === "vue"
        ? { file: "src/DailyBinding.vue", token: "dailyValue", occurrence: 0 }
        : { file: "src/DailyBinding.svelte", token: "dailyValue", occurrence: 0 };
    try {
      const locs = await typeDefinitionsAt(anchor);
      if (locs.length === 0) throw new Error("no type definition locations");
    } catch (err) {
      failParityGap(
        this,
        "lsp.type-definition.binding",
        "ISSUE-lsp-type-definition",
        `Type definition provider empty/unmapped for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("lsp.semantic-tokens.present", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const ok = await semanticTokensExist(file);
      if (!ok) throw new Error("no semantic tokens (or provider command unavailable)");
    } catch (err) {
      failParityGap(
        this,
        "lsp.semantic-tokens.present",
        "ISSUE-lsp-semantic-tokens",
        `Semantic tokens not observable for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("lsp.document-format.carrier", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    try {
      const doc = await openRelative(file);
      const edits = await vscode.commands.executeCommand<vscode.TextEdit[]>(
        "vscode.executeFormatDocumentProvider",
        doc.uri,
        { tabSize: 2, insertSpaces: true },
      );
      // Empty edits can mean already formatted OR no provider — require a provider response array.
      if (!Array.isArray(edits)) {
        throw new Error("format provider returned non-array");
      }
    } catch (err) {
      failParityGap(
        this,
        "lsp.document-format.carrier",
        "ISSUE-lsp-document-format",
        `Document formatting not available for ${fw}: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("product.extract-component.command", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    // Volar / Svelte Official expose extract-component; Verter may not yet.
    const commands = await vscode.commands.getCommands(true);
    const candidates = commands.filter(
      (c) =>
        /extract.*component/i.test(c) ||
        /verter\..*extract/i.test(c) ||
        /vue\.action\.extract/i.test(c),
    );
    if (candidates.length === 0) {
      failParityGap(
        this,
        "product.extract-component.command",
        "ISSUE-product-extract-component",
        "No extract-component command is registered (product gap vs official extensions)",
        "product-gap",
      );
    }
  });
});
