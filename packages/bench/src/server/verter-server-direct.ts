// Test wrapper for the language server that doesn't auto-start
import { createConnection, Connection } from "vscode-languageserver/node";
import { DocumentManager } from "@verter/language-server/src/v5/documents/manager/manager";
import { VerterManager } from "@verter/language-server/src/v5/documents/verter/manager";
import { isVueDocument } from "@verter/language-server/src/v5/documents";
import {
  formatQuickInfo,
  mapDefinitionInfo,
} from "@verter/language-server/src/v5/helpers";
import * as path from "node:path";
import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";

let documents: Map<string, TextDocument> = new Map();

export const testWorkspacePath = path.resolve(
  __dirname,
  "../../test-workspace"
);

export interface VerterServer {
  openDocument: (
    uri: string,
    languageId: string,
    content: string
  ) => Promise<TextDocument>;
  closeDocument: (uri: string) => Promise<void>;
  getCompletions: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
  getHover: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
  getDefinition: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
}

// Test subclass to expose protected methods
class TestDocumentManager extends DocumentManager {
  createDocumentPublic(
    uri: string,
    languageId: string,
    content: string,
    version: number
  ) {
    return this.createDocument(uri, languageId, content, version);
  }
}

export async function getVerterServer(): Promise<VerterServer> {
  // Import the managers directly
  // const { VerterManager } = await import('@verter/language-server/src/v5/documents/verter/manager');

  const documentManager = new TestDocumentManager();
  const verterManager = new VerterManager(documentManager);

  // Initialize with a mock params object
  verterManager.init({
    processId: process.pid,
    rootUri: URI.file(testWorkspacePath).toString(),
    capabilities: {},
    workspaceFolders: [
      {
        uri: URI.file(testWorkspacePath).toString(),
        name: "test-workspace",
      },
    ],
  } as any);

  return {
    openDocument: async (uri: string, languageId: string, content: string) => {
    //   if (documents.has(uri)) {
    //     return documents.get(uri)!;
    //   }
      // Use the public wrapper method to create the document
      const doc = documentManager.createDocumentPublic(
        uri,
        languageId,
        content,
        1
      );
      documents.set(uri, TextDocument.create(uri, languageId, 1, content));
      return doc as any;
    },
    closeDocument: async (uri: string) => {
      documents.delete(uri);
      // The document manager will handle cleanup
    },
    getCompletions: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      const doc = documentManager.getDocument(uri);
      if (!doc) {
        throw new Error(`Document not found: ${uri}`);
      }

      if (!isVueDocument(doc)) {
        return { items: [] };
      }

      const tsService = verterManager.getTsService(uri);
      if (!tsService) {
        return { items: [] };
      }

      const subDocs = doc.docsForPos(position);
      const items: any[] = [];

      for (const d of subDocs) {
        const offset = d.offset;

        if (d.doc.languageId === "tsx" || d.doc.languageId === "ts") {
          const completions = tsService.getCompletionsAtPosition(
            d.doc.uri,
            offset,
            {
              includeAutomaticOptionalChainCompletions: true,
              jsxAttributeCompletionStyle: "auto",
            }
          );

          if (completions) {
            items.push(
              ...completions.entries
                .filter((x: any) => !x.name.startsWith("___VERTER___"))
                .map((x: any) => ({
                  label: x.name,
                  kind: x.kind,
                  sortText: x.sortText,
                  data: {
                    virtualUrl: d.doc.uri,
                    index: offset,
                  },
                }))
            );
          }
        }
      }

      return { items, isIncomplete: false };
    },
    getHover: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      const doc = documentManager.getDocument(uri);
      if (!doc) {
        return undefined;
      }

      if (!isVueDocument(doc)) {
        return undefined;
      }

      const tsService = verterManager.getTsService(uri);
      if (!tsService) {
        return undefined;
      }

      const docs = doc.docsForPos(position);

      for (const { doc: subDoc, offset } of docs) {
        const quickInfo = tsService.getQuickInfoAtPosition(subDoc.uri, offset);
        if (quickInfo) {
          return formatQuickInfo(quickInfo, subDoc);
        }
      }

      return undefined;
    },
    getDefinition: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      const doc = documentManager.getDocument(uri);
      if (!doc) {
        return undefined;
      }

      if (!isVueDocument(doc)) {
        return undefined;
      }

      const tsService = verterManager.getTsService(uri);
      if (!tsService) {
        return undefined;
      }

      const docs = doc.docsForPos(position);
      const definitions: any[] = [];

      for (const d of docs) {
        for (const x of tsService.getDefinitionAtPosition(
          d.doc.uri,
          d.offset
        ) ?? []) {
          const r = mapDefinitionInfo(x, documentManager, true);
          if (r) definitions.push(r);
        }
      }

      return definitions;
    },
  };
}

export async function closeVerterServer() {
  documents.clear();
}
