import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "./vue/index.js";

export const VerterVirtualFileScheme = "verter-virtual";

export function isVueDocument(doc: TextDocument): doc is VueDocument {
  return doc instanceof VueDocument;
  return doc.languageId === "vue";
}

export function isVerterVirtual(
  uri: string
): uri is `${typeof VerterVirtualFileScheme}://${string}` {
  return uri.startsWith(VerterVirtualFileScheme);
}

export function isVueFile(uri: string): uri is `${string}.vue` {
  return uri.endsWith(".vue");
}
export function pathToUri(filepath: string) {
  if (filepath.startsWith("file:///")) {
    return filepath;
  }
  return URI.file(filepath).toString();
}

export function pathToVerterVirtual(filepath: string) {
  return uriToVerterVirtual(pathToUri(filepath));
}

const VueSubDocRegex = /(?:\.vue)\._VERTER_\.(\w+)\.(\w{2,4})$/;
export function isVueSubDocument(uri: string) {
  return VueSubDocRegex.test(uri);
}

export function uriToVerterVirtual(uri: string) {
  if (isVueFile(uri) || isVueSubDocument(uri)) {
    if (!isVerterVirtual(uri)) {
      const parsed = URI.parse(uri);
      uri = URI.from({
        ...parsed,
        scheme: "file",
      }).toString();
    }
    return uri;
  }
  return uri;
}

export function uriToPath(uri: string) {
  const url = URI.parse(uri);

  if (url.scheme !== "file" && url.scheme !== VerterVirtualFileScheme) {
    return uri;
  }
  // normalise path separators for windows
  const p = url.fsPath.replace(/\\/g, "/");
  return p;
}
