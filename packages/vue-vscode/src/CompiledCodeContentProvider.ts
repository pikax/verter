// Pretty much https://github.com/sveltejs/language-tools/blob/ceb4f7065e4471d75fac7c0191ec7ad67446e81f/packages/svelte-vscode/src/CompiledCodeContentProvider.ts

import { LanguageClient } from "vscode-languageclient/node";
import { debounce } from "lodash";
import {
  Uri,
  TextDocumentContentProvider,
  EventEmitter,
  workspace,
  window,
  Disposable,
} from "vscode";
import { RequestType, type PatchClient } from "@verter/language-shared";
import { isFrameworkCarrierLanguageId } from "./frameworkWiring";

// ContentProvider for "verter-compiled://" files
export default class CompiledCodeContentProvider implements TextDocumentContentProvider {
  static previewWindowUri = Uri.parse("verter-compiled:///preview.tsx");
  static scheme = "verter-compiled";

  private didChangeEmitter = new EventEmitter<Uri>();
  private selectedCarrierFile: string | undefined;
  private subscriptions: Disposable[] = [];
  private disposed = false;

  get onDidChange() {
    return this.didChangeEmitter.event;
  }

  // This function triggers a refresh of the preview window's content
  // by emitting an event to the didChangeEmitter. VSCode listens to
  // this.onDidChange and will call provideTextDocumentContent
  private refresh() {
    this.didChangeEmitter.fire(CompiledCodeContentProvider.previewWindowUri);
  }

  constructor(private getLanguageClient: () => PatchClient<LanguageClient>) {
    this.subscriptions.push(
      // This event triggers a refresh of the preview window's content
      // whenever the selected framework carrier file's content changes
      // (debounced to prevent too many recompilations)
      workspace.onDidChangeTextDocument(
        debounce(async (event) => {
          if (isFrameworkCarrierLanguageId(event.document.languageId) && this.selectedCarrierFile) {
            this.refresh();
          }
        }, 500),
      ),
    );

    this.subscriptions.push(
      // This event sets the selected carrier file when a different framework
      // carrier file is selected and triggers a refresh of the preview content.
      window.onDidChangeActiveTextEditor((editor) => {
        if (!isFrameworkCarrierLanguageId(editor?.document?.languageId)) {
          return;
        }

        const newFile = editor!.document.uri.toString();

        if (newFile !== this.selectedCarrierFile) {
          this.selectedCarrierFile = newFile;
          this.refresh();
        }
      }),
    );
  }

  // This is called when VSCode needs to get the content of the preview window
  // we can trigger this using the didChangeEmitter
  async provideTextDocumentContent(): Promise<string | undefined> {
    // If there is no selected carrier file, try to get it from the activeTextEditor.
    // This should only happen when showCompiledCodeToSide is called the first time.
    if (!this.selectedCarrierFile && window.activeTextEditor) {
      this.selectedCarrierFile = window.activeTextEditor.document.uri.toString();
    }

    // Should not be possible but handle it anyway
    if (!this.selectedCarrierFile) {
      window.setStatusBarMessage("Verter: no framework component file selected");
      return;
    }

    const response = await this.getLanguageClient().sendRequest(RequestType.GetCompiledCode, {
      uri: this.selectedCarrierFile,
    });

    const path = this.selectedCarrierFile.replace("file://", "");

    if (response?.js?.code) {
      // return `/* Compiled: ${path} */\n${response.js.code}`;
      return response.js.code;
    } else {
      window.setStatusBarMessage(`Verter: fail to compile ${path}`, 3000);
    }
  }

  dispose() {
    if (this.disposed) {
      return;
    }

    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
    this.disposed = true;
  }
}
