import { URI } from "vscode-uri";

export function uriToVerterVirtual(uri: string) {
  if (uri.endsWith(".vue")) {
    if (!uri.startsWith("file:///")) {
      uri = pathToUrl(uri);
    }
    uri = uri.replace("file:", "verter-virtual:");
    return uri + ".tsx";
  }
  return uri;
}

export function pathToUrl(path: string) {
  return URI.file(path).toString();
}

export function urlToPath(stringUrl: string): string | null {
  const url = URI.parse(stringUrl);

  if (url.scheme !== "file" && url.scheme !== "verter-virtual") {
    return null;
  }
  const p = url.fsPath.replace(/\\/g, "/");
  if (url.scheme === "verter-virtual" && p.endsWith('.vue.tsx')) {
    return p.slice(0, -4);
  }
  return p;
}

export function urlToFileUri(stringUrl: string): string {
  if (isVerterVirtual(stringUrl)) {
    return stringUrl.replace("verter-virtual:", "file:").slice(0, -4);
  }
  if (stringUrl.startsWith("file:")) {
    return stringUrl;
  }

  return pathToUrl(stringUrl);
}

export function isVerterVirtual(path: string) {
  return path.startsWith("verter-virtual:");
}
