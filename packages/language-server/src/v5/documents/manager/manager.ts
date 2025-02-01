import {
  Connection,
  Disposable,
  TextDocuments,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VerterDocument, VueDocument } from "../verter/index.js";
import type { IScriptSnapshot } from "typescript";
import { readFileSync, existsSync } from "node:fs";

import {
  isVueDocument,
  isVueFile,
  isVueSubDocument,
  pathToUri,
  toVueParentDocument,
  uriToPath,
} from "../utils.js";

export type VersionScriptSnapshot = IScriptSnapshot & { version: number };

export class DocumentManager implements Disposable {
  private _dispose: Disposable | null = null;

  readonly textDocuments: TextDocuments<VerterDocument>;
  readonly _files = new Map<string, VerterDocument>();

  constructor() {
    this.textDocuments = new TextDocuments({
      create: (uri, languageId, version, content): VerterDocument => {
        let cached = this._files.get(uri);

        if (cached) {
          if (isVueDocument(cached)) {
            return cached.update(content, version);
          } else {
            return cached.update(content, version);
          }
        }
        return this.createDocument(uri, languageId, content, version);
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
    this._dispose = this.textDocuments.listen(connection);
    return this._dispose;
  }

  protected createDocument(
    uri: string,
    languageId: string,
    content: string,
    version: number = 1
  ) {
    const filepath = uriToPath(uri);
    if (isVueFile(uri)) {
      const doc = VueDocument.create(uri, content, version);
      this._files.set(uri, doc);
      this._files.set(filepath, doc);
      return doc;
    } else {
      const doc = VerterDocument.createDoc(uri, languageId, version, content);
      this._files.set(uri, doc);
      this._files.set(filepath, doc);
      return doc;
    }
  }

  fileExists(filepath: string) {
    if (isVueSubDocument(filepath)) {
      filepath = toVueParentDocument(filepath);
    }

    if (this._files.has(filepath)) {
      return true;
    }

    return existsSync(filepath);
  }

  readFile(filepath: string, encoding: BufferEncoding = "utf-8") {
    let d = this._files.get(filepath);
    if (!d) {
      const c = readFileSync(filepath, { encoding });
      const uri = pathToUri(filepath);

      if (isVueFile(filepath)) {
        d = VueDocument.create(uri, c, 0);
      } else {
        d = VerterDocument.createDoc(
          uri,
          filepath.split(".").pop() ?? "ts",
          0,
          c
        );
      }
      this._files.set(filepath, d);
      this._files.set(uri, d);
      return c;
    }

    return d.getText();
  }

  getDocument(filename: string) {
    if (isVueSubDocument(filename)) {
      filename = toVueParentDocument(filename);
    }
    let d = this._files.get(filename);
    if (!d) {
      if (this.fileExists(filename)) {
        this.readFile(filename);

        d = this._files.get(filename);
      }
    }
    return d;
  }
}
