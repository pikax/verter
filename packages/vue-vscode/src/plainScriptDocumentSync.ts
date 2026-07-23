import { BASE_TYPESCRIPT_LANGUAGE_IDS } from "@verter/language-shared";

export interface PlainScriptDocumentSnapshot {
  uri: string;
  scheme: string;
  languageId: string;
  version: number;
  text: string;
}

export interface PlainScriptTextDocument {
  uri: {
    scheme: string;
    path: string;
    toString(skipEncoding?: boolean): string;
  };
  languageId: string;
  version: number;
  getText(): string;
}

export interface LspNotificationSender {
  sendNotification(method: string, params: unknown): unknown;
}

const PLAIN_SCRIPT_LANGUAGE_IDS = new Set(BASE_TYPESCRIPT_LANGUAGE_IDS);
const PLAIN_SCRIPT_FILE_SUFFIX = /\.(?:[cm]?[jt]sx?)$/i;

export function isPlainScriptLanguageId(languageId: string): boolean {
  return PLAIN_SCRIPT_LANGUAGE_IDS.has(languageId);
}

export function isPlainScriptFileUri(uri: { scheme: string; path: string }): boolean {
  return uri.scheme === "file" && PLAIN_SCRIPT_FILE_SUFFIX.test(uri.path);
}

export function shouldSuppressPlainScriptDiagnostics(
  uri: { scheme: string; path: string },
  openLanguageId?: string,
): boolean {
  return (
    isPlainScriptFileUri(uri) ||
    (uri.scheme === "file" &&
      openLanguageId !== undefined &&
      isPlainScriptLanguageId(openLanguageId))
  );
}

export function snapshotPlainScriptDocument(
  document: PlainScriptTextDocument,
): PlainScriptDocumentSnapshot {
  return {
    uri: document.uri.toString(),
    scheme: document.uri.scheme,
    languageId: document.languageId,
    version: document.version,
    text: document.getText(),
  };
}

function isPlainScriptDocument(document: PlainScriptDocumentSnapshot): boolean {
  return document.scheme === "file" && PLAIN_SCRIPT_LANGUAGE_IDS.has(document.languageId);
}

/**
 * Keeps plain TS/JS documents synchronized as dependency state without
 * registering Verter editor features for them.
 */
export class PlainScriptDocumentSync {
  private readonly documents = new Map<string, PlainScriptDocumentSnapshot>();
  private readonly remoteOpenUris = new Set<string>();
  private sender: LspNotificationSender | undefined;

  observeOpen(document: PlainScriptDocumentSnapshot): void {
    if (!isPlainScriptDocument(document)) return;
    this.documents.set(document.uri, document);
    this.sendOpen(document);
  }

  observeChange(document: PlainScriptDocumentSnapshot): void {
    if (!isPlainScriptDocument(document)) return;
    this.documents.set(document.uri, document);
    if (!this.sender) return;
    if (this.sendOpen(document)) return;
    this.sender.sendNotification("textDocument/didChange", {
      textDocument: { uri: document.uri, version: document.version },
      contentChanges: [{ text: document.text }],
    });
  }

  observeClose(document: PlainScriptDocumentSnapshot): void {
    if (!isPlainScriptDocument(document)) return;
    this.documents.delete(document.uri);
    if (!this.sender || !this.remoteOpenUris.delete(document.uri)) return;
    this.sender.sendNotification("textDocument/didClose", {
      textDocument: { uri: document.uri },
    });
  }

  connect(sender: LspNotificationSender): void {
    this.sender = sender;
    this.remoteOpenUris.clear();
    for (const document of this.documents.values()) {
      this.sendOpen(document);
    }
  }

  disconnect(): void {
    this.sender = undefined;
    this.remoteOpenUris.clear();
  }

  observeClientState(
    state: "stopped" | "starting" | "running",
    sender: LspNotificationSender,
  ): void {
    if (state === "stopped") {
      this.disconnect();
    } else if (state === "running") {
      this.connect(sender);
    }
  }

  private sendOpen(document: PlainScriptDocumentSnapshot): boolean {
    if (!this.sender || this.remoteOpenUris.has(document.uri)) return false;
    this.remoteOpenUris.add(document.uri);
    this.sender.sendNotification("textDocument/didOpen", {
      textDocument: {
        uri: document.uri,
        languageId: document.languageId,
        version: document.version,
        text: document.text,
      },
    });
    return true;
  }
}
