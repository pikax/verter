import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "./verter/index.js";

export const VerterVirtualFileScheme = "verter-virtual";

export function isVueDocument(doc: TextDocument): doc is VueDocument {
  return doc instanceof VueDocument;
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

const VueSubDocRegex = /(?:\.vue)(\._VERTER_\.\w+\.\w{2,4})$/;
export function isVueSubDocument(uri: string) {
  return VueSubDocRegex.test(uri);
}
export function toVueParentDocument(uri: string) {
  return uri.replace(VueSubDocRegex, ".vue");
}

export function createSubDocumentUri(uri: string, ending: string) {
  if (!uri.endsWith(".vue")) {
    throw new Error("URI is not a Vue file:" + uri);
  }
  return `${uri}._VERTER_.${ending}`;
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
  let url = URI.parse(uri);
  if (url.scheme === VerterVirtualFileScheme) {
    const match = uri.match(VueSubDocRegex);
    if (match) {
      // url = URI.parse(uri.replace(match[1], ""));
      url = URI.parse(
        uri.replace(match[1], "").replace(VerterVirtualFileScheme, "file")
      );
    }
  } else if (url.scheme !== "file" && url.scheme !== VerterVirtualFileScheme) {
    return uri;
  }
  // normalise path separators for windows
  const p = url.fsPath.replace(/\\/g, "/");
  return p;
}


// export function retrieveFileExtension(uri: string) {
//   const url = URI.parse(uri);
//   url.
//   return url.path.split(".").pop() ?? "";
// }