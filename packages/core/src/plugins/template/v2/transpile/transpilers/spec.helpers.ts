import { NodeTypes } from "@vue/compiler-core";
import { MagicString, parse as sfcParse } from "@vue/compiler-sfc";
import { reducePlugins, type TranspilerPlugin } from "../utils";
import { TranspileOptions, transpile } from "../transpile";

export function fromTranspiler<T extends NodeTypes>(
  transpiler: TranspilerPlugin<T>,
  content: string,
  plugins = [] as TranspilerPlugin[],
  options: Omit<TranspileOptions, "plugins"> = {},
  doNotWrap = false
) {
  const source = doNotWrap ? content : `<template>${content}</template>`;

  const sfc = sfcParse(source);

  const template = sfc.descriptor.template;
  const ast = template.ast!;
  const s = new MagicString(ast.source);
  const c = transpile(ast, s, {
    plugins: reducePlugins([...plugins, transpiler]),
    ...options,
  });

  const contentIndex = source.indexOf(content);

  const result = s.toString();

  return {
    sfc,
    s,
    c,

    fullResult: s.toString(),
    fullOriginal: s.original,

    original: s.original.slice(contentIndex, contentIndex + content.length),
    // result: s.snip(contentIndex, contentIndex + content.length).toString(),
    // result: s.toString().slice("<template>".length, -"</template>".length),

    result: result.startsWith("<template>")
      ? result.slice("<template>".length, -"</template>".length)
      : result,

    // .snip(prop.exp.loc.start.offset - 1, prop.exp.loc.end.offset + 1)
    // .slice(1, -1)
  };
}
