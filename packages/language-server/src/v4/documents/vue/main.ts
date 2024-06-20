import { SourceMapConsumer } from "source-map-js";
import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";

import { ParseScriptContext } from "@verter/core";

export class VueDocument implements TextDocument {
  //   static fromTextDocument(doc: TextDocument, shouldParse = false) {
  //     const vuedoc = new VueDocument(
  //       doc,
  //       TextDocument.create(uriToVerterVirtual(doc.uri), "tsx", doc.version, "")
  //     );
  //     if (shouldParse) {
  //       vuedoc.parse();
  //     }
  //     return vuedoc;
  //   }

  //   static create(uri: string, content: string, shouldParse = false) {
  //     const doc = TextDocument.create(uri, "vue", -1, content);
  //     return VueDocument.fromTextDocument(doc, shouldParse);
  //   }

  //   private constructor(
  //     private _doc: TextDocument,
  //     private _compiledDoc: TextDocument
  //   ) {
  //     this.overrideDoc(this._doc);
  //   }

  languageId = "vue";

  uri: string;
  version: number;
  lineCount: number;

  getText(range?: Range): string {
    throw new Error("Method not implemented.");
  }
  positionAt(offset: number): Position {
    throw new Error("Method not implemented.");
  }
  offsetAt(position: Position): number {
    throw new Error("Method not implemented.");
  }

  private _context: ParseScriptContext;
  get context(): ParseScriptContext {
    return this._context;
  }
}
