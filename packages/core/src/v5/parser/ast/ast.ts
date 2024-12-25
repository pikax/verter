import oxc, { EcmaScriptModule } from "oxc-parser";
import { parse as acornLooseParse, LooseParser } from "acorn-loose";

import { parse as acornParse } from "acorn";

import type AcornTypes from "acorn";
import { babelParse } from "@vue/compiler-sfc";
import tsPlugin from "acorn-typescript";

export type VerterAST = ReturnType<typeof acornLooseParse>;

export type VerterASTNode = AcornTypes.AnyNode;

const looserParser = LooseParser.extend(tsPlugin());

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

export function parseAcornLooseTS(
  source: string
): ReturnType<typeof acornLooseParse> {
  const ast = looserParser.parse(source, {
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

export function parseOXC(source: string) {
  return oxc.parseSync("index.ts", source);
}

export function parseBabel(source: string) {
  return babelParse(source);
}

export function sanitisePosition(source: string) {
  // handling non-ascii characters gives some odd nodes position in the AST
  return source.replace(/[^\x00-\x7F]/g, "*");
}

export function parseAST(
  source: string,
  sourceFilename: string = "index.ts"
): VerterAST {
  return parseAcornLoose(source);
  // const normaliseSource = sanitisePosition(source);
  // let ast: VerterAST;
  // try {
  //   const result = oxc.parseSync(sourceFilename, normaliseSource);

  //   const ms = result.magicString;

  //   if (result.errors.length) {
  //     // fallback to acorn parser if oxc parser failed
  //     // because acorn-loose is more lenient in handling errors
  //     throw result.errors;
  //   } else {
  //     // @ts-expect-error
  //     ast = result.program;
  //   }
  // } catch (e) {
  //   console.error("oxc parser failed", e);
  //   ast = parseAcornLoose(normaliseSource);
  // }

  // return ast;
}
