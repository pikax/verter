import { dirname, resolve } from "path";

function getRequire(packageName: string) {
  return require(packageName);
}

export function getPackage(packageName: string, path: string) {
  const paths = [__dirname, path];

  // TODO handle untrusted workspacesF

  const pkgPath = require.resolve(`${packageName}/package.json`, {
    paths,
  });

  return {
    path: dirname(pkgPath),
  };
}

export function importVueCompiler(fromPath: string): typeof import("vue/compiler-sfc") {
  const pkg = getPackage("vue", fromPath);
  const main = resolve(pkg.path, "compiler-sfc");
  return getRequire(main);
}
