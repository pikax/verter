import ts from "typescript";
import {
  TextDocuments,
  Disposable,
  Connection,
  TextDocumentChangeEvent,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "./document";
import { isVerterVirtual, urlToFileUri, urlToPath } from "./utils";
import { URI } from "vscode-uri";
// import { workspace } from "vscode";

export class DocumentManager {
  readonly _textDocuments = new TextDocuments(TextDocument);
  #disposable: Disposable | undefined;

  files = new Map<string, VueDocument>();

  snapshots = new Map<string, ts.IScriptSnapshot & { version: number }>();

  public listen(connection: Connection) {
    const dispose = this._textDocuments.listen(connection);

    // connection.workspace.onDidChangeTextDocument((e) => {
    //   if (e.document.languageId !== "vue") return;

    //   console.log("sdee", e);
    // });

    this._textDocuments.onDidOpen((e) => {
      if (e.document.languageId !== "vue") return;
    });

    this._textDocuments.onDidChangeContent((e) => {
      if (e.document.languageId !== "vue") return;

      // const changes = e.document.getText();

      // const doc = this.getDocument(e.document.uri);

      // doc?.overrideDoc(e.document)
    });

    return (this.#disposable = dispose);
  }

  public getDocument<IsVue extends true>(
    uri: string
  ): (VueDocument & { languageId: "vue" }) | undefined;
  public getDocument(
    uri: string
  ): (VueDocument & { languageId: "vue" }) | TextDocument | undefined;
  public getDocument(
    uri: string
  ): (VueDocument & { languageId: "vue" }) | TextDocument | undefined {
    const filePath = urlToPath(uri);
    const fileUri = urlToFileUri(uri);
    const doc = this._textDocuments.get(fileUri);

    if (doc) {
      if (doc.languageId === "vue") {
        const key = filePath ?? fileUri;
        let vueDoc = this.files.get(key);
        if (!vueDoc) {
          vueDoc = VueDocument.fromTextDocument(doc);
          this.files.set(key, vueDoc);
        }
        return vueDoc;
      }

      return doc;
    }

    const parsedUri = URI.parse(uri);
  }

  public getSnapshotIfExists(
    uri: string
  ): (ts.IScriptSnapshot & { version: number }) | undefined {
    if (isVerterVirtual(uri)) {
      uri = urlToPath(uri)!;
    }

    const doc = this.files.get(uri) ?? this._textDocuments.get(uri);
    if (!doc) {
      // todo maybe resolve this?
      // let snap = this.snapshots.get(uri);

      // if (!snap) {
      // }

      return undefined;
    }

    let snap = this.snapshots.get(uri);
    if (snap?.version === doc.version) {
      return snap;
    }

    const content =
      "getParsedText" in doc && doc.languageId === "vue"
        ? (doc as VueDocument).getParsedText()
        : doc.getText();

    snap = Object.assign(ts.ScriptSnapshot.fromString(content), {
      version: doc.version,
    });

    this.snapshots.set(uri, snap);

    return snap;
  }
}
