import { TextDocumentContentProvider, EventEmitter, Uri, Event, Disposable } from "vscode";

/**
 * Content provider for virtual files using a custom `verter-virtual://` URI scheme.
 * This avoids writing to disk (which would trigger the LSP file watcher and crash).
 *
 * URI format: verter-virtual:///kind/lang?sourceUri=<encoded-source-uri>
 * Example:    verter-virtual:///tsx/tsx?sourceUri=file%3A%2F%2F%2Fhome%2Fuser%2FApp.vue
 */
export class VirtualFileContentProvider implements TextDocumentContentProvider, Disposable {
  static scheme = "verter-virtual";

  private _onDidChange = new EventEmitter<Uri>();
  readonly onDidChange: Event<Uri> = this._onDidChange.event;

  /** Cache: URI string → content */
  private contentMap = new Map<string, string>();

  /**
   * Store content for a virtual file and return its URI.
   */
  setContent(kind: string, lang: string, sourceUri: string, code: string): Uri {
    const uri = VirtualFileContentProvider.buildUri(kind, lang, sourceUri);
    this.contentMap.set(uri.toString(), code);
    this._onDidChange.fire(uri);
    return uri;
  }

  /**
   * Build a virtual file URI.
   */
  static buildUri(kind: string, lang: string, sourceUri: string): Uri {
    // Use the lang as the "file extension" in the path so VS Code applies syntax highlighting
    const safeName = kind.replace(":", ".");
    return Uri.parse(
      `${VirtualFileContentProvider.scheme}:///${safeName}.${lang}?sourceUri=${encodeURIComponent(sourceUri)}`,
    );
  }

  /**
   * Get the language ID that VS Code should use for syntax highlighting.
   */
  static langToLanguageId(lang: string): string {
    switch (lang) {
      case "tsx":
        return "typescriptreact";
      case "ts":
        return "typescript";
      case "js":
        return "javascript";
      case "jsx":
        return "javascriptreact";
      case "css":
        return "css";
      case "scss":
        return "scss";
      case "sass":
        return "sass";
      case "less":
        return "less";
      default:
        return lang;
    }
  }

  provideTextDocumentContent(uri: Uri): string | undefined {
    return this.contentMap.get(uri.toString());
  }

  /**
   * Invalidate all cached content (e.g., when the source file changes).
   */
  invalidateAll(): void {
    for (const key of this.contentMap.keys()) {
      this._onDidChange.fire(Uri.parse(key));
    }
  }

  /**
   * Clear all cached content.
   */
  clear(): void {
    this.contentMap.clear();
  }

  dispose(): void {
    this._onDidChange.dispose();
    this.contentMap.clear();
  }
}
