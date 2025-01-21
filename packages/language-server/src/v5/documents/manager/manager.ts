import { Connection, Disposable, TextDocuments } from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VerterDocument, VueDocument } from "../verter/index.js";
import type { IScriptSnapshot } from "typescript";

import {
  isVueDocument,
  isVueFile,
  isVueSubDocument,
  uriToPath,
} from "../utils.js";

export type VersionScriptSnapshot = IScriptSnapshot & { version: number };

export class DocumentManager implements Disposable {
  private _dispose: Disposable | null = null;

  readonly _textDocuments: TextDocuments<VerterDocument>;

  readonly _files = new Map<string, VerterDocument>();
  readonly _snapshots = new Map<string, VersionScriptSnapshot>();

  constructor() {
    this._textDocuments = new TextDocuments({
      create: (uri, languageId, version, content): VerterDocument => {
        const filepath = uriToPath(uri);
        let cached = this._files.get(filepath);

        if (cached) {
          if (isVueDocument(cached)) {
            return cached.update(content, version);
          } else {
            return cached.update(content, version);
          }
        }
        if(isVueFile(uri)) {
          const doc = VueDocument.create(uri, content, version);
          this._files.set(filepath, doc);
          return doc;
        } else {
          const doc = VerterDocument.createDoc(uri, languageId, version, content);
          this._files.set(filepath, doc);
          return doc;
        }
      },
      update: (document, changes, version) => {
        console.log("update", document.uri, version);
        // return TextDocument.update(document, changes, version);
        return document.update(changes, version);
      },
    });
  }

  dispose(): void {

    return this._dispose?.dispose();
  }


  listen(connection: Connection) {
    this._dispose = this._textDocuments.listen(connection);

    return this._dispose;
  }
  fileExists(filepath: string) {
    return false
  }

  readFile(filepath: string, encoding: string = "utf-8") {}
}
