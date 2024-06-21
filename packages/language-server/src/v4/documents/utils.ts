import { URI } from "vscode-uri";
import { isFileInVueBlock } from "../processor/utils.js";

import fsPath from "path/posix";

const virtualFileScheme = "verter-virtual";

export function uriToVerterVirtual(uri: string) {
  if (isVueFile(uri) || isFileInVueBlock(uri)) {
    // return URI.file(uri).with({ scheme: virtualFileScheme }).toString();
    // URI.from({
    //   scheme: "file",
    // });
    if (!uri.startsWith("file:///")) {
      uri = pathToUri(uri);
      console.warn("please use pathToVerterVirtual instead");
    }
    uri = virtualFileScheme + uri.substring("file".length);
    return uri;
  }
  return uri;
}

export function isVerterVirtual(path: string) {
  return path.startsWith(virtualFileScheme);
}

export function isVueFile(uri: string): uri is `${string}.vue` {
  return uri.endsWith(".vue");
}

export function pathToUri(filepath: string) {
  return URI.file(filepath).toString();
}
export function pathToVerterVirtual(filepath: string) {
  return uriToVerterVirtual(pathToUri(filepath));
}

export function uriToPath(uri: string) {
  const url = URI.parse(uri);

  if (url.scheme !== "file" && url.scheme !== virtualFileScheme) {
    return uri;
  }
  // normalise path separators for windows
  const p = url.fsPath.replace(/\\/g, "/");
  return p;
}
