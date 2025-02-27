import oxc, { ParserOptions } from "oxc-parser";
import { parse as acornLooseParse, LooseParser } from "acorn-loose";

import { parse as acornParse } from "acorn";

import type AcornTypes from "acorn";
import { babelParse } from "@vue/compiler-sfc";
import { VerterAST } from "./types";

export function parseAcornLoose(
  source: string
): ReturnType<typeof acornLooseParse> {
  const ast = acornLooseParse(source, {
    ecmaVersion: "latest",
    allowAwaitOutsideFunction: true,
    sourceType: "module",
  });
  return ast;
}

export function parseAcorn(source: string) {
  const ast = acornParse(source, {
    ecmaVersion: "latest",
  });
  return ast;
}

export function parseOXC(source: string, options?: ParserOptions) {
  return oxc.parseSync("index.ts", source, options);
}

export function parseBabel(
  source: string,
  options?: Parameters<typeof babelParse>[1]
) {
  return babelParse(source, options);
}

export function sanitisePosition(source: string) {
  // handling non-ascii characters gives some odd nodes position in the AST
  return source.replace(/[^\x00-\x7F]/g, "*");
}

function langFilename(filename: string) {
  const ext = filename.split(".").pop();
  switch (ext) {
    case "tsx":
    case "jsx":
    case "ts":
    case "js":
      return ext;
  }
  throw new Error("Unknown extension: " + ext);
}
export function parseAST(
  source: string,
  sourceFilename: string = "index.ts"
): VerterAST {
  // return parseOXC(source).program;
  // const normaliseSource = sanitisePosition(source);
  const normaliseSource = source;
  let ast: VerterAST;
  try {
    const result = oxc.parseSync(sourceFilename, normaliseSource, {
      lang: langFilename(sourceFilename),
    });

    if (result.errors.length) {
      // fallback to acorn parser if oxc parser failed
      // because acorn-loose is more lenient in handling errors
      throw result.errors;
    } else {
      ast = result.program;
    }
  } catch (e) {
    console.error("oxc parser failed :(", e, sourceFilename);
    // @ts-expect-error
    ast = parseAcornLoose(normaliseSource);
  }

  return ast;
}
