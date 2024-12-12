import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";

export class VueDocument implements TextDocument {
  //   static fromFilepath(
  //     filepath: string,
  //     content: string | (() => string),
  //     shouldParse = false
  //   ) {
  //     return VueDocument.fromUri(pathToUri(filepath), content, shouldParse);
  //   }
  //   static fromUri(
  //     uri: string,
  //     content: string | (() => string),
  //     shouldParse = false
  //   ) {
  //     const isString = typeof content === "string";
  //     const doc = TextDocument.create(
  //       uri,
  //       "vue",
  //       -2,
  //       isString ? content : "/* VERTER CONTENT NOT LOADED */\n"
  //     );
  //     return VueDocument.fromTextDocument(
  //       doc,
  //       shouldParse,
  //       isString ? undefined : content
  //     );
  //   }

  //   static fromTextDocument(
  //     doc: TextDocument,
  //     shouldParse = false,
  //     loadDocumentContent?: () => string
  //   ) {
  //     const vuedoc = new VueDocument(doc, loadDocumentContent);
  //     if (shouldParse) {
  //       vuedoc.syncVersion();
  //     }
  //     return vuedoc;
  //   }

  //   get bundleDoc() {
  //     return this.subDocuments.bundle;
  //   }
  //   private constructor(
  //     private _doc: TextDocument,
  //     private _loadDocumentContent?: () => string
  //   ) {
  //     this.subDocuments.bundle = new VueSubDocument(
  //       this,
  //       processors.bundle,
  //       "bundle"
  //     );
  //     this.subDocuments.script = new VueSubDocument(
  //       this,
  //       processors.script,
  //       "script"
  //     );
  //     this.subDocuments.template = new VueSubDocument(
  //       this,
  //       processors.template,
  //       "template"
  //     );
  //   }

  uri: string;
  languageId: string;
  version: number;
  getText(range?: Range): string {
    throw new Error("Method not implemented.");
  }
  positionAt(offset: number): Position {
    throw new Error("Method not implemented.");
  }
  offsetAt(position: Position): number {
    throw new Error("Method not implemented.");
  }
  lineCount: number;
}
