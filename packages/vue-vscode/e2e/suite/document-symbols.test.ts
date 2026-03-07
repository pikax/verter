import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  getDocumentSymbols,
  FIXTURE_NAME,
} from "../helpers";

suite(`Document Symbols [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  test("DS1: has document symbols", async function () {
    const symbols = await getDocumentSymbols(doc.uri);
    console.log(`    Document symbols: ${symbols.length} symbol(s)`);

    expect(symbols.length, "should have at least one symbol").to.be.greaterThan(0);
  });

  test("DS2: contains expected binding names", async function () {
    const symbols = await getDocumentSymbols(doc.uri);

    // Flatten hierarchy: top-level symbols may be block containers (script, template)
    // with binding symbols nested as children.
    function collectNames(syms: (vscode.DocumentSymbol | vscode.SymbolInformation)[]): string[] {
      const result: string[] = [];
      for (const s of syms) {
        result.push(s.name);
        if ("children" in s && s.children) {
          result.push(...collectNames(s.children as vscode.DocumentSymbol[]));
        }
      }
      return result;
    }
    const names = collectNames(symbols);
    console.log(`    Symbol names: ${names.slice(0, 10).join(", ")}${names.length > 10 ? "..." : ""}`);

    // Script bindings should appear as document symbols
    expect(names, "should contain 'count'").to.include("count");
    expect(names, "should contain 'doubled'").to.include("doubled");
    expect(names, "should contain 'increment'").to.include("increment");
  });
});
