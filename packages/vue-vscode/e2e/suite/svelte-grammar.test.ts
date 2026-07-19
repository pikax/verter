import * as fs from "fs";
import * as path from "path";
import { expect } from "chai";
import * as vscode from "vscode";
import {
  ensureTypeProviderSynced,
  isLspReady,
  openVueFile,
  waitForFileReady,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";
import { pollUntil, semanticTokensExist } from "../lib/parityHarness";

// Svelte syntax-colorization packaging: a `.svelte` document opens under the
// `svelte` language id, and the RUNNING Verter extension contributes both the
// TextMate grammar (source.svelte, with embedded TS/JS + CSS/SCSS/LESS) and
// the svelte language configuration — colorization parity with `.vue`.
//
// The manifest assertions run for every fixture (they interrogate the live
// extension host, not the workspace). The document assertions need the
// committed Svelte fixtures, which exist only in `single-project`.

const SVELTE_CHILD = "src/SvelteChild.svelte";

interface ContributedGrammar {
  language?: string;
  scopeName: string;
  path: string;
  embeddedLanguages?: Record<string, string>;
}

interface ContributedLanguage {
  id: string;
  extensions?: string[];
  configuration?: string;
}

function verterExtension(): vscode.Extension<unknown> {
  const ext = vscode.extensions.all.find(
    (e) =>
      Array.isArray(
        (e.packageJSON as { contributes?: { languages?: ContributedLanguage[] } }).contributes
          ?.languages,
      ) &&
      (
        e.packageJSON as { contributes: { languages: ContributedLanguage[] } }
      ).contributes.languages.some((l) => l.id === "svelte"),
  );
  expect(ext, "an extension contributing the svelte language must be active").to.not.be.undefined;
  return ext!;
}

suite(`Svelte grammar packaging [${FIXTURE_NAME}]`, function () {
  test("extension contributes a source.svelte TextMate grammar with an on-disk grammar file", () => {
    const ext = verterExtension();
    const contributes = (
      ext.packageJSON as {
        contributes?: { grammars?: ContributedGrammar[] };
      }
    ).contributes;
    const grammar = (contributes?.grammars ?? []).find((g) => g.language === "svelte");
    expect(grammar, "the svelte language must have a contributed grammar").to.not.be.undefined;
    expect(grammar!.scopeName).to.equal("source.svelte");
    const grammarPath = path.join(ext.extensionPath, grammar!.path);
    expect(fs.existsSync(grammarPath), `grammar file must exist: ${grammarPath}`).to.be.true;
    const parsed = JSON.parse(fs.readFileSync(grammarPath, "utf8")) as {
      scopeName?: string;
      patterns?: unknown[];
    };
    expect(parsed.scopeName).to.equal("source.svelte");
    expect(parsed.patterns ?? []).to.have.length.greaterThan(0);
    // Embedded language wiring: script + style regions map onto real languages.
    const embedded = grammar!.embeddedLanguages ?? {};
    expect(embedded["source.ts"]).to.equal("typescript");
    expect(embedded["source.js"]).to.equal("javascript");
    expect(embedded["source.css"]).to.equal("css");
    expect(embedded["source.css.scss"]).to.equal("scss");
    expect(embedded["source.css.less"]).to.equal("less");
  });

  test("extension contributes a svelte language configuration (comments/brackets/auto-closing)", () => {
    const ext = verterExtension();
    const contributes = (
      ext.packageJSON as {
        contributes?: { languages?: ContributedLanguage[] };
      }
    ).contributes;
    const lang = (contributes?.languages ?? []).find((l) => l.id === "svelte");
    expect(lang?.configuration, "svelte must declare a language configuration").to.not.be.undefined;
    const configPath = path.join(ext.extensionPath, lang!.configuration!);
    expect(fs.existsSync(configPath), `language configuration must exist: ${configPath}`).to.be
      .true;
    const config = JSON.parse(fs.readFileSync(configPath, "utf8")) as {
      comments?: { blockComment?: string[] };
      brackets?: unknown[];
      autoClosingPairs?: unknown[];
    };
    expect(config.comments?.blockComment).to.deep.equal(["<!--", "-->"]);
    expect(config.brackets ?? []).to.have.length.greaterThan(0);
    expect(config.autoClosingPairs ?? []).to.have.length.greaterThan(0);
  });

  test("the vue grammar contribution is untouched (negative: no regression)", () => {
    const ext = verterExtension();
    const contributes = (
      ext.packageJSON as {
        contributes?: { grammars?: ContributedGrammar[] };
      }
    ).contributes;
    const vueGrammar = (contributes?.grammars ?? []).find((g) => g.language === "vue");
    expect(vueGrammar, "vue grammar must still be contributed").to.not.be.undefined;
    expect(vueGrammar!.scopeName).to.equal("source.vue");
  });

  test("a .svelte document opens with the svelte language id", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const doc = await openVueFile(SVELTE_CHILD);
    expect(doc.languageId).to.equal("svelte");
  });

  test("LSP semantic tokens still layer on top of the TextMate grammar", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    this.timeout(60_000);
    expect(isLspReady(), "LSP must be ready").to.be.true;
    await ensureTypeProviderSynced();
    const doc = await openVueFile(SVELTE_CHILD);
    await waitForFileReady(doc);
    // Semantic tokens are additive (VS Code layers them ABOVE TextMate scopes).
    // Where the product serves `.svelte` semantic-token DATA (the shared-tsgo
    // lane), hard-assert a non-empty legend AND non-empty token data via the
    // shared hard predicate (`semanticTokensExist`) — an absent/empty provider
    // result FAILS this test instead of degrading to a log line.
    //
    // tsserver / managed-tsgo do not produce `.svelte` token data yet — the
    // ledgered product gap ISSUE-lsp-semantic-tokens (e2e/ISSUES.md), owned and
    // hard-failed by the parity suite (`lsp.semantic-tokens.present`). On those
    // routes this packaging suite still asserts the discriminating packaging
    // fact it owns: the provider REGISTRATION (legend) survives the grammar
    // contribution.
    if (TYPE_PROVIDER === "shared-tsgo") {
      const present = await pollUntil(
        "svelte semantic tokens (non-empty legend + data)",
        () => semanticTokensExist(SVELTE_CHILD),
        (ok) => ok,
        20_000,
      );
      expect(present, "semantic tokens must be present (non-empty legend + non-empty data)").to.be
        .true;
    } else {
      const legend = await vscode.commands.executeCommand<vscode.SemanticTokensLegend | undefined>(
        "vscode.provideDocumentSemanticTokensLegend",
        doc.uri,
      );
      expect(
        legend,
        "a semantic-token provider (legend) must stay registered for .svelte documents",
      ).to.not.be.undefined;
      expect(legend!.tokenTypes, "legend must be non-empty").to.have.length.greaterThan(0);
      console.log(
        `    token DATA not asserted on ${TYPE_PROVIDER}: ledgered gap ISSUE-lsp-semantic-tokens`,
      );
    }
  });
});
