import {
  TextDocuments,
  Disposable,
  Connection,
  TextDocumentChangeEvent,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocumentManager } from "./VueDocumentManager";
import { VueDocument } from "./VueDocument";

import { readFileSync, existsSync } from 'node:fs'

export type ListenerType = 'open' | 'change' | 'close'

export type ListenerCb = (type: ListenerType, params: TextDocumentChangeEvent<VueDocument>) => void
// | ((type: 'open', params: TextDocumentChangeEvent<VueDocument>) => void)
// | ((type: 'change', params: TextDocumentChangeEvent<VueDocument>) => void)
// | ((type: 'close', params: TextDocumentChangeEvent<VueDocument>) => void)

class DocumentManager {
  //   readonly #documents = new Map<string, TextDocument>();
  readonly _textDocuments = new TextDocuments(TextDocument) as TextDocuments<VueDocument>;
  #disposable: Disposable | undefined;

  private readonly manager = new VueDocumentManager();

  private compiledDocs = new Map<string, VueDocument>()
  private externalDocs = new Map<string, TextDocument>()

  private _listeners = new Array<ListenerCb>

  constructor() { }

  public addListener(cb: ListenerCb) {
    this._listeners.push(cb)
  }

  public listen(connection: Connection) {
    const dispose = this._textDocuments.listen(connection);

    connection.workspace.onDidDeleteFiles((params) => {
      console.log("deleted files", params);
      params.files.forEach((x) => this.manager.removeDocument(x.uri));
    });

    connection.onDidChangeWatchedFiles((params) => {
      console.log("watched files", params);
    });
    connection.workspace.onWillDeleteFiles((params) => {
      console.log("will delete files", params);
      return null;
    });
    // setTimeout(() => {
    //   connection.workspace.getWorkspaceFolders().then((x) => {
    //     console.log("workspace folders", x);
    //   });
    // }, 1000);

    const docs = this._textDocuments;

    docs.onDidOpen((e) => {
      if (e.document.languageId !== "vue") return;
      const vueDoc = this.manager.openDocument(e.document);

      this._listeners.forEach(x => x('open', {
        document: vueDoc
      }))

      // vueDoc.blocks.forEach((block)=> {
      //   switch(block) {
      //     case 'script': {

      //     }
      //   }
      // })


      // e.document.uri.startsWith('file:')


      connection.window.showInformationMessage(
        "Document changed: " + e.document.uri
      );
    });

    docs.onDidClose((e) => {
      if (e.document.languageId !== "vue") return;
      const vueDoc = this.manager.openDocument(e.document);
      this._listeners.forEach(x => x('close', {
        document: vueDoc
      }))
      this.manager.closeDocument(e.document.uri);
    });

    docs.onDidChangeContent((change) => {
      // ignore non-vue documents
      if (change.document.languageId !== "vue") return;


      const virtualUrl = change.document.uri.replace('file:', 'verter-virtual:').replace('.vue', '.vue.tsx')

      const doc = this.getDocument(virtualUrl);
      doc!.setText(change.document.getText())

      ++doc!.version


      this._listeners.forEach(x => x('change', {
        document: doc!
      }))
      connection.window.showInformationMessage(
        "Document changed: " + change.document.uri
      );
      //   this.#documents.set(change.document.uri, change.document());
    });

    docs.onDidClose((event) => {
      connection.window.showInformationMessage(
        "Document close: " + event.document.uri
      );
    });

    return dispose;
  }

  public getDocument(uri: string): VueDocument | TextDocument | undefined {
    // if (!uri.startsWith('file:') && !uri.startsWith('verter-virtual')) {
    //   const l = uri
    //   uri = `file://${encodeURIComponent(uri)}`

    //   console.log('update uri', {
    //     from: l,
    //     to: uri
    //   })
    // }

    const doc = this._textDocuments.get(uri);
    if (doc) return doc;
    if (uri.startsWith('verter-virtual:')) {
      if (this.compiledDocs.has(uri)) return this.compiledDocs.get(uri)!
      const originalUri = uri.replace('verter-virtual:', "file:").replace('.vue.tsx', '.vue')

      const dd = this._textDocuments.get(originalUri)
      if (dd) {
        const vueDoc = new VueDocument(uri, dd.getText())
        this.compiledDocs.set(uri, vueDoc)
        return vueDoc
      }
    } else {
      let external = this.externalDocs.get(uri);
      if (!external && existsSync(uri)) {
        external = TextDocument.create(uri, 'typescript', 1, readFileSync(uri, { encoding: 'utf-8' }))

        this.externalDocs.set(uri, external)
      }

      return external;
    }



    // if (!doc) {
    //   if
    //   if (!uri.startsWith('verter-virtual:')) return
    //   if (this.compiledDocs.has(uri)) return this.compiledDocs.get(uri)!
    //   const originalUri = uri.replace('verter-virtual:', "file:").replace('.vue.tsx', '.vue')

    //   const dd = this._textDocuments.get(originalUri)
    //   if (dd) {
    //     const vueDoc = new VueDocument(uri, dd.getText())
    //     this.compiledDocs.set(uri, vueDoc)
    //     return vueDoc
    //   }

    // } else if (uri.indexOf('///') === -1) {
    //   let external = this.externalDocs.get(uri);
    //   if (!external) {
    //     external = TextDocument.create(uri, 'typescript', 1, readFileSync(uri, { encoding: 'utf-8' }))

    //     this.externalDocs.set(uri, external)
    //   }

    //   return external;
    //   // TODO we should watch these for changes
    // }
    // return doc;
  }

  public getAllOpened(): string[] {
    // TODO add virtual files
    return this._textDocuments.all().map((x) => x.uri);
  }

  public dispose() {
    return this.#disposable?.dispose();
  }

  //   public openDocument(uri: string, text: string): TextDocument {
  //     const document = TextDocument.create(uri, "plaintext", 0, text);
  //     return document;
  //   }

  //   public closeDocument(uri: string): void {
  //     this.#documents.delete(uri);
  //   }

  //   public getDocument(uri: string): TextDocument | undefined {
  //     return this.#documents.get(uri);
  //   }
}

export const documentManager = new DocumentManager();
