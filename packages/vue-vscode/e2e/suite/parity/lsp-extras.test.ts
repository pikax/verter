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
  semanticTokenAt,
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

  test("lsp.semantic-tokens.kinds", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const file = fw === "vue" ? "src/DailyBinding.vue" : "src/DailyBinding.svelte";
    // The same identifier in an equivalent `.ts` file gets these token-type
    // NAMES under VS Code's TypeScript semantic highlighting — the carrier
    // must match name-for-name (Verter's legend may be a subset of TS's).
    // `data.length > 0` is NOT asserted anywhere: existence cannot see the
    // wrong-kind defect class (provider-legend indices forwarded unmapped).
    const expectations: ReadonlyArray<{ token: string; occurrence: number; expected: string }> = [
      // Script-side declarations.
      { token: "DailyValue", occurrence: 0, expected: "interface" },
      { token: "dailyValue", occurrence: 0, expected: "variable" },
      { token: "renderDaily", occurrence: 0, expected: "function" },
    ];
    try {
      for (const { token, occurrence, expected } of expectations) {
        const resolved = await semanticTokenAt({ file, token, occurrence });
        if (!resolved) {
          throw new Error(`no semantic token covers ${file}#${token}[${occurrence}]`);
        }
        if (resolved.tokenType !== expected) {
          throw new Error(
            `${file}#${token}[${occurrence}] highlighted as \`${resolved.tokenType}\` ` +
              `(modifiers: ${resolved.modifiers.join(",") || "none"}), expected \`${expected}\``,
          );
        }
      }
      // Modifier half: the const/let binding's declaration must carry the
      // `declaration` modifier — a type-only remap that forwards raw modifier
      // bitsets ships wrong `static`/`readonly`/`async` styling.
      const declaration = await semanticTokenAt({ file, token: "dailyValue", occurrence: 0 });
      if (!declaration?.modifiers.includes("declaration")) {
        throw new Error(
          `dailyValue declaration lacks the \`declaration\` modifier ` +
            `(got: ${declaration?.modifiers.join(",") || "none"})`,
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "lsp.semantic-tokens.kinds",
        "ISSUE-lsp-semantic-tokens",
        `Semantic token kinds wrong or unobservable for ${fw}: ${String(err)}`,
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
