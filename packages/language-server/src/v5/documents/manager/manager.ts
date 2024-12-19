import { Disposable, TextDocuments } from "vscode-languageserver";
import { TextDocument } from "vscode-languageserver-textdocument";
import type { VerterDocument } from "../verter/index.js";
import type { IScriptSnapshot } from "typescript";

import { isVueFile, isVueSubDocument, uriToPath } from "../utils.js";

export type VersionScriptSnapshot = IScriptSnapshot & { version: number };

export class DocumentManager implements Disposable {
  private _dispose: Disposable;

  readonly _textDocuments: TextDocuments<VerterDocument>;

  readonly _files = new Map<string, VerterDocument>();
  readonly _snapshots = new Map<string, VersionScriptSnapshot>();

  constructor() {}

  dispose(): void {
    return this.dispose();
  }

  fileExists(filepath: string) {}

  readFile(filepath: string, encoding: string = "utf-8") {}
}
