import { TextDocuments } from "vscode-languageserver";
import { TextDocument } from "vscode-languageserver-textdocument";
import type { VueDocument } from "../vue/index.js";
import type { IScriptSnapshot } from "typescript";

import { isVueFile, isVueSubDocument, uriToPath } from "../utils.js";

export type VersionScriptSnapshot = IScriptSnapshot & { version: number };

export class DocumentManager {
  readonly _textDocuments: TextDocuments<TextDocument>;

  readonly _files = new Map<string, VueDocument>();
  readonly _snapshots = new Map<string, VersionScriptSnapshot>();

  constructor() {}

  fileExists(filepath: string) {}

  readFile(filepath: string, encoding: string = "utf-8") {}
}
