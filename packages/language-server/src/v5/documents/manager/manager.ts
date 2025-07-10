import {
  Connection,
  Disposable,
  TextDocuments,
  TextEdit,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import {
  TypescriptDocument,
  VerterDocument,
  VueDocument,
} from "../verter/index.js";
import type { IScriptSnapshot } from "typescript";
import { readFileSync, existsSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { extname } from "node:path";

import {
  isVerterVirtual,
  isVueDocument,
  isVueFile,
  isVueSubDocument,
  normalisePath,
  pathToUri,
  toVueParentDocument,
  uriToPath,
} from "../utils.js";
import { FileNotificationChange } from "@verter/language-shared";

export type VersionScriptSnapshot = IScriptSnapshot & { version: number };

export class DocumentManager implements Disposable {
  private _dispose: Disposable | null = null;

  readonly textDocuments: TextDocuments<VerterDocument>;
  readonly _files = new Map<string, VerterDocument>();
  readonly _fileExistsMap = new Map<string, boolean>();

  private _currentConnection: Connection | undefined;

  get connection() {
    return this._currentConnection;
  }

  constructor() {
    this.textDocuments = new TextDocuments({
      create: (uri, languageId, version, content): VerterDocument => {
        this.handleFileChange(uri, "create");
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
        // this.handleFileChange(document.uri, "update");

        // return TextDocument.update(document, changes, version);
        return document.update(changes, version);
      },
    });
  }

  dispose(): void {
    return this._dispose?.dispose();
  }

  listen(connection: Connection) {
    if (this._currentConnection) {
      this._dispose?.dispose();
      this._currentConnection = undefined;
    }
    this._dispose = this.textDocuments.listen(connection);
    this._currentConnection = connection;

    return this._dispose;
  }

  protected createDocument(
    uri: string,
    languageId: string,
    content: string,
    version: number = 1
  ) {
    console.log("createDocument doc", uri);
    const filepath = uriToPath(uri);
    if (isVueFile(uri)) {
      const doc = VueDocument.create(uri, content, version);
      this._files.set(uri, doc);
      this._files.set(filepath, doc);
      this._fileExistsMap.set(filepath, true);
      return doc;
    } else {
      const doc = TypescriptDocument.create(
        uri,
        languageId as any,
        version,
        content
      );
      this._files.set(uri, doc);
      this._files.set(filepath, doc);
      this._fileExistsMap.set(filepath, true);
      return doc;
    }
  }

  fileExists(filepath: string) {
    // normalise path
    filepath = uriToPath(filepath);

    if (isVueSubDocument(filepath)) {
      filepath = toVueParentDocument(filepath);
    }

    if (this._files.has(filepath)) {
      return true;
    }

    let cached = this._fileExistsMap.get(filepath);
    if (cached === undefined) {
      // normalise path
      cached = existsSync(uriToPath(filepath));
      this._fileExistsMap.set(filepath, cached);
    }

    // if (filepath.indexOf("vue/runtime-dom") >= 0) {
    //   const eee = existsSync(filepath);
    //   console.log("eeer", eee);
    //   debugger;
    // }
    return cached;
  }

  readFile(filepath: string, encoding: BufferEncoding = "utf-8") {
    // if (isVerterVirtual(filepath)) {
    // normalise path
    filepath = uriToPath(filepath);

    // }
    let d = this._files.get(filepath);
    if (!d) {
      console.log("reading file sync", filepath);
      const c = readFileSync(filepath, { encoding });
      const uri = pathToUri(filepath);

      if (isVueFile(filepath)) {
        d = VueDocument.create(uri, c, 0);
      } else {
        d = TypescriptDocument.create(
          uri,
          filepath.split(".").pop() ?? ("ts" as any),
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
    // if (isVerterVirtual(filename)) {
    filename = uriToPath(filename);
    // }
    let d = this._files.get(filename);
    if (!d) {
      if (this.fileExists(filename)) {
        this.readFile(filename);

        d = this._files.get(filename);
      }
    }
    return d;
  }

  handleFileChange(uri: string, type: FileNotificationChange) {
    const path = normalisePath(uri);
    uri = pathToUri(path);

    switch (type) {
      case "create": {
        this._fileExistsMap.set(path, true);
        break;
      }
      case "update": {
        const doc = this.getDocument(path);
        if (!doc) return;
        return readFile(path, { encoding: "utf-8" }).then((c) => {
          doc?.update(c);
        });
      }
      case "delete": {
        this._fileExistsMap.set(path, false);
        this._fileExistsMap.set(uri, false);
        this._files.delete(path);
        this._files.delete(uri);
        break;
      }
    }
  }

  applyFileChanges(
    uri: string,
    changes: Array<{
      text: string;
      range: {
        start: {
          line: number;
          character: number;
        };
        end: {
          line: number;
          character: number;
        };
      };
    }>
  ) {
    const path = normalisePath(uri);
    uri = pathToUri(path);

    const doc = this.getDocument(uri);
    if (doc) {
      // doc.applyEdits(changes.map((x) => TextEdit.replace(x.range, x.text)));
      doc.update(changes);
    } else {
      // file does not exits
      console.log("file does not exits", uri, path);
    }
  }
}
