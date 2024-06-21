import {
  TextDocuments,
  Disposable,
  Connection,
  TextDocumentChangeEvent,
  Position,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "../vue/index.js";
import { isVerterVirtual, isVueFile, pathToUri, uriToPath } from "../utils.js";
import ts from "typescript";
import {
  isFileInVueBlock,
  retrieveVueFileFromBlockUri,
} from "../../processor/utils.js";

export class DocumentManager {
  //   readonly _textDocuments = new TextDocuments(TextDocument);
  //   readonly _textDocuments = new TextDocuments({
  //     create(uri: string, languageId: string, version: number, content: string) {
  //       return TextDocument.create(uri, languageId, version, content);
  //     },
  //     update(document, changes, version) {
  //       // todo add update for vue document

  //       return TextDocument.update(document, changes, version);
  //     },
  //   });
  readonly _textDocuments: TextDocuments<TextDocument>;
  #disposable: Disposable | undefined;

  files = new Map<string, VueDocument | TextDocument>();
  snapshots = new Map<string, ts.IScriptSnapshot & { version: number }>();

  constructor() {
    this.fileExists = this.fileExists.bind(this);
    this.readFile = this.readFile.bind(this);

    this._textDocuments = new TextDocuments({
      create: (
        uri: string,
        languageId: string,
        version: number,
        content: string
      ) => {
        const filepath = uriToPath(uri);
        let cached = this.files.get(filepath);
        if (cached) {
          if (isVueDocument(cached)) {
            return cached.override(content, version);
          } else {
            return TextDocument.update(
              cached,
              [
                {
                  text: content,
                  range: {
                    start: cached.positionAt(0),
                    end: cached.positionAt(cached.getText().length),
                  },
                },
              ],
              version
            );
          }
        }
        cached = TextDocument.create(uri, languageId, version, content);
        this.files.set(filepath, cached);
        return cached;
      },
      update: (document, changes, version) => {
        const filepath = uriToPath(document.uri);
        const cached = this.files.get(filepath);
        if (cached) {
          if (isVueDocument(cached)) {
            return cached.update(changes, version);
          } else {
            return TextDocument.update(cached, changes, version);
          }
        }
        return TextDocument.update(document, changes, version);
      },
    });
  }

  private _onDocumentOpenCb = [] as Array<(doc: VueDocument) => void>;

  onDocumentOpen(callback: (doc: VueDocument) => void) {
    this._onDocumentOpenCb.push(callback);
  }

  public listen(connection: Connection) {
    const dispose = this._textDocuments.listen(connection);

    this._textDocuments.onDidOpen((e) => {
      console.log("opened", e.document.uri);

      const vueDoc = this.files.get(uriToPath(e.document.uri));
      if (isVueDocument(vueDoc)) {
        this._onDocumentOpenCb.forEach((cb) => cb(vueDoc));
      }

      // update cache if needed
      //   this.updateDoc(e.document);
    });

    this._textDocuments.onDidChangeContent((e) => {
      if (e.document.languageId !== "vue") return;
    });

    return (this.#disposable = dispose);
  }

  /**
   * Updates cache documents
   * @param document
   */
  protected updateDoc(document: TextDocument) {
    const filepath = uriToPath(document.uri);
    const cachedDoc = this.files.get(filepath);
    if (
      document.version !== cachedDoc?.version ||
      document.getText() !== cachedDoc?.getText()
    ) {
      if (isVueFile(filepath)) {
        if (cachedDoc) {
          (cachedDoc as VueDocument).overrideDoc(document);
        } else {
          this.files.set(filepath, VueDocument.fromTextDocument(document));
        }
      } else {
        this.files.set(filepath, document);
      }
    }
  }

  /**
   * This will preload the document
   * the preload will not load the full document
   * but will be used for checking if the document exists or not
   * @param uri
   * @param text
   */
  preloadDocument(filepath: string, text = "") {
    let doc: VueDocument | TextDocument = this.files.get(filepath);
    if (doc) {
      console.log("Document already exists", filepath);
      return doc;
    }
    if (isVueFile(filepath)) {
      doc = VueDocument.fromFilepath(filepath, text);
    } else {
      doc = TextDocument.create(filepath, "typescript", -2, text);
    }

    // console.log("[verter] preloaded document", filepath);
    this.files.set(filepath, doc);
    return doc;
  }

  fileExists(filepath: string) {
    if (isFileInVueBlock(filepath)) {
      filepath = retrieveVueFileFromBlockUri(filepath);
    }
    if (this.files.has(filepath)) {
      return true;
    }

    const exists = ts.sys.fileExists(filepath);
    if (exists) {
      this.preloadDocument(filepath);
      return true;
    }
    return false;
  }

  readFile(filepath: string, encoding: string = "utf-8") {
    let doc = this.files.get(filepath);
    if (doc && doc.version > -2) {
      return doc.getText();
    }
    doc = this._textDocuments.get(pathToUri(filepath));
    if (!doc) {
      doc = this.files.get(filepath);
      if (doc && doc.version === -2) {
        const text = ts.sys.readFile(filepath, encoding);
        TextDocument.update(doc, [{ text }], 0);
        return text;
      }
    }
    if (doc) {
      if (isVueFile(filepath)) {
        doc = VueDocument.fromTextDocument(doc);
      }
      this.files.set(filepath, doc);
      return doc.getText();
    }

    const text = ts.sys.readFile(filepath, encoding);
    this.preloadDocument(filepath, text);
    return text;
  }

  public getSnapshotIfExists(
    filename: string
  ): (ts.IScriptSnapshot & { version: number }) | undefined {
    // if (isVerterVirtual(filename)) {
    //   const vueDockPath = uriToPath(
    //     isFileInVueBlock(filename)
    //       ? retrieveVueFileFromBlockUri(filename)
    //       : filename
    //   );

    //   const vueDoc = this.files.get(vueDockPath);
    //   if (isVueDocument(vueDoc)) {
    //     const doc = vueDoc.getDocument(filename);

    //     return vueDoc.getSnapshot();
    //   }
    // }

    // if (isVerterVirtual(filename)) {
    //   console.log('isVerterVirtual', filename)
    // }
    const uri = isVerterVirtual(filename) ? filename : pathToUri(filename);
    let doc = this.files.get(
      isFileInVueBlock(uri)
        ? uriToPath(retrieveVueFileFromBlockUri(uri))
        : filename
    );
    if (!doc) {
      return undefined;
    }
    let snap = this.snapshots.get(uri);
    if (snap && snap.version === doc.version) {
      return snap;
    }

    let content = isVueDocument(doc)
      ? doc.getTextFromFile(uri)
      : this.readFile(filename);

    snap = Object.assign(ts.ScriptSnapshot.fromString(content), {
      version: doc.version,
    });
    this.snapshots.set(uri, snap);
    return snap;
  }

  documentContent(filename: string) {
    const doc = this.files.get(filename);
    if (doc.version > -2) {
      return doc.getText();
    }
    return this.readFile(filename);
  }

  public getDocument(uri: string): VueDocument | TextDocument | undefined {
    const filepath = uriToPath(uri);
    return this.files.get(filepath);
  }
}

export function isVueDocument(doc: TextDocument): doc is VueDocument {
  return doc.languageId === "vue";
}
