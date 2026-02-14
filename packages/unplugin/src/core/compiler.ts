import { createHash } from "crypto";
import { createRequire } from "module";
import type { Compiler } from "./types";

const require = createRequire(import.meta.url);

let compiler: Compiler | null = null;

export function loadCompiler(): Compiler {
  if (compiler) return compiler;

  const native = require("@verter/native") as typeof import("@verter/native");
  compiler = {
    compileForVite: (input, opts) => native.compileForVite(input, opts),
    processStyle: (css, opts) => native.processStyle(css, opts),
  };
  return compiler;
}

export function getHash(text: string): string {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

export function generateComponentId(filename: string, source: string, isProd: boolean): string {
  const normalized = filename.replace(/\\/g, "/");
  return isProd ? getHash(normalized) : getHash(normalized + source);
}
