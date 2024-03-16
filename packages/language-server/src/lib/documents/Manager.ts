import {
  TextDocuments,
  Disposable,
  Connection,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocumentManager } from "./VueDocumentManager";
import { VueDocument } from "./VueDocument";

class DocumentManager {
  //   readonly #documents = new Map<string, TextDocument>();
  readonly #textDocuments = new TextDocuments(TextDocument) as TextDocuments<VueDocument>;
  #disposable: Disposable | undefined;

  private readonly manager = new VueDocumentManager();

  constructor() { }

  public listen(connection: Connection) {
    const dispose = this.#textDocuments.listen(connection);

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

    const docs = this.#textDocuments;

    docs.onDidOpen((e) => {
      if (e.document.languageId !== "vue") return;
      const vueDoc = this.manager.openDocument(e.document);

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
      this.manager.closeDocument(e.document.uri);
    });

    docs.onDidChangeContent((change) => {
      // ignore non-vue documents
      if (change.document.languageId !== "vue") return;

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

  public getDocument(uri: string): VueDocument | undefined {
    // if (uri.startsWith('virtual:')) {
    //   uri = uri.replace('virtual:', "file:")

    //   if (uri.endsWith('.vue.d.tsx')) {
    //     uri = uri.replace('.vue.d.tsx', '.vue')
    //   }
    //   console.log('getting doc', uri)
    // }
    if (uri.endsWith('.vue.d.tsx')) {
      if (uri.endsWith('.vue.d.tsx')) {
        uri = uri.replace('.vue.d.tsx', '.vue')
      }
    }


    return this.#textDocuments.get(uri);
  }

  public getAllOpened(): string[] {
    // TODO add virtual files
    return this.#textDocuments.all().map((x) => x.uri);
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
