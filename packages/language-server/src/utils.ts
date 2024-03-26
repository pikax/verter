import { URI } from "vscode-uri";
import { dirname } from 'path'
import ts from 'typescript';


export function pathToUrl(path: string) {
  return URI.file(path).toString();
}

export function urlToPath(stringUrl: string): string | null {
  const url = URI.parse(stringUrl);
  if (url.scheme !== 'file') {
    return null;
  }
  return url.fsPath.replace(/\\/g, '/');
}

export type GetCanonicalFileName = (fileName: string) => string;

export function findTsConfigPath(
  fileName: string,
  rootUris: string[],
  fileExists: (path: string) => boolean,
  getCanonicalFileName: GetCanonicalFileName
) {
  const searchDir = dirname(fileName);

  const tsconfig = ts.findConfigFile(searchDir, fileExists, 'tsconfig.json') || '';
  const jsconfig = ts.findConfigFile(searchDir, fileExists, 'jsconfig.json') || '';
  // Prefer closest config file
  const config = tsconfig.length >= jsconfig.length ? tsconfig : jsconfig;

  // Don't return config files that exceed the current workspace context or cross a node_modules folder
  return !!config &&
    rootUris.some((rootUri) => isSubPath(rootUri, config, getCanonicalFileName)) &&
    !fileName
      .substring(config.length - 13)
      .split('/')
      .includes('node_modules')
    ? config
    : '';
}

export function isSubPath(
  uri: string,
  possibleSubPath: string,
  getCanonicalFileName: GetCanonicalFileName
): boolean {
  // URL escape codes are in upper-case
  // so getCanonicalFileName should be called after converting to file url
  return getCanonicalFileName(pathToUrl(possibleSubPath)).startsWith(getCanonicalFileName(uri));
}

